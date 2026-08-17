//! Fetches and processes Tarkov map assets from the tarkov-dev repository.
//!
//! Downloads map metadata, SVG files, and tile pyramids, then generates a local
//! `maps.ron` file for the viewer application.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use image::{ImageBuffer, RgbaImage};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use resvg::tiny_skia::Pixmap;
use resvg::usvg::{Options, Transform, Tree};
use ron::ser::PrettyConfig;
use serde::Deserialize;
use thiserror::Error;
use tokio::fs as async_fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use tarkov_map::{Extent, ExtentBound, Extract, Label, Layer, Map, Spawn, TarkovMaps};

/// Errors that can occur during the fetch_maps process.
#[derive(Error, Debug)]
pub enum FetchError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to fetch {resource}: HTTP {status}")]
    HttpStatus { resource: String, status: u16 },

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SVG parse error: {0}")]
    SvgParse(String),

    #[error("failed to create pixmap for rendering")]
    PixmapCreation,

    #[error("failed to save PNG: {0}")]
    PngSave(String),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("RON serialization error: {0}")]
    Ron(#[from] ron::Error),

    #[error("progress bar template error: {0}")]
    ProgressTemplate(#[from] indicatif::style::TemplateError),

    #[error("semaphore acquire error: {0}")]
    Semaphore(#[from] tokio::sync::AcquireError),

    #[error("task join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("map '{name}' has no human-readable name")]
    MissingMapName { name: String },

    #[error("map '{name}' has no svgPath or tilePath")]
    MissingMapSource { name: String },

    #[error("map '{name}' is missing minZoom")]
    MissingMinZoom { name: String },

    #[error("map '{name}' is missing maxZoom")]
    MissingMaxZoom { name: String },

    #[error("bc7z container error: {0}")]
    Bc7z(#[from] tarkov_map::bc7z::Bc7zError),

    #[error("missing PNG source for '{path}' — run a full fetch first")]
    MissingPngSource { path: String },
}

/// Result of downloading a single tile.
type TileResult = Result<(u32, u32, Vec<u8>), FetchError>;

/// Fetch Tarkov map assets from tarkov-dev
#[derive(Parser, Debug)]
#[command(name = "fetch_maps", version, about)]
struct Args {
    /// Force re-download of all assets, ignoring cached files
    #[arg(short, long)]
    force: bool,

    /// Reduce tile map zoom level from max (0 = max quality, higher = smaller files)
    #[arg(long, default_value = "2")]
    tile_zoom_offset: i32,

    /// Skip fetching: re-encode the existing PNGs referenced by maps.ron into
    /// .bc7z containers and rewrite maps.ron to point at them.
    #[arg(long)]
    convert_only: bool,
}

const MAPS_JSON_URL: &str =
    "https://raw.githubusercontent.com/the-hideout/tarkov-dev/main/src/data/maps.json";
const TARKOV_DEV_MAPS_URL: &str = "https://json.tarkov.dev/regular/maps";
const TARKOV_DEV_MAPS_EN_URL: &str = "https://json.tarkov.dev/regular/maps_en";
const USER_AGENT: &str = "tarkov-map";
const MAPS_RON_PATH: &str = "assets/maps.ron";
/// Physical directory for storing map images on disk
const MAPS_DIR: &str = "assets/maps";
/// Path prefix for maps.ron (relative to assets/ for rust-embed)
const MAPS_PATH_PREFIX: &str = "maps";
const TILE_DOWNLOAD_CONCURRENCY: usize = 32;
const SVG_RENDER_SCALE: f32 = 2.0;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedMapGroup {
    normalized_name: String,
    maps: Vec<FetchedMap>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedMap {
    #[serde(default)]
    alt_maps: Option<Vec<String>>,
    projection: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    author_link: Option<String>,
    #[serde(default)]
    tile_size: Option<i32>,
    #[serde(default)]
    min_zoom: Option<i32>,
    #[serde(default)]
    max_zoom: Option<i32>,
    #[serde(default)]
    transform: Option<[f64; 4]>,
    #[serde(default)]
    coordinate_rotation: Option<f64>,
    #[serde(default)]
    bounds: Option<[[f64; 2]; 2]>,
    #[serde(default)]
    svg_path: Option<String>,
    #[serde(default)]
    tile_path: Option<String>,
    #[serde(default)]
    height_range: Option<[f64; 2]>,
    #[serde(default)]
    layers: Option<Vec<FetchedLayer>>,
    #[serde(default)]
    labels: Option<Vec<FetchedLabel>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedLayer {
    name: String,
    #[serde(skip)]
    image_path: Option<String>,
    #[serde(default)]
    svg_layer: Option<String>,
    #[serde(default)]
    tile_path: Option<String>,
    #[serde(default)]
    show: bool,
    #[serde(default)]
    extents: Vec<FetchedExtent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedExtent {
    height: [f64; 2],
    #[serde(default)]
    bounds: Option<Vec<FetchedExtentBound>>,
}

#[derive(Debug, Deserialize)]
#[serde(from = "Vec<serde_json::Value>")]
struct FetchedExtentBound {
    point1: [f64; 2],
    point2: [f64; 2],
    name: String,
}

impl From<Vec<serde_json::Value>> for FetchedExtentBound {
    fn from(values: Vec<serde_json::Value>) -> Self {
        let parse_point = |idx: usize| -> [f64; 2] {
            values
                .get(idx)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    [
                        arr.first().and_then(|v| v.as_f64()).unwrap_or(0.0),
                        arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                    ]
                })
                .unwrap_or([0.0, 0.0])
        };

        Self {
            point1: parse_point(0),
            point2: parse_point(1),
            name: values
                .get(2)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedLabel {
    position: [f64; 2],
    text: String,
    #[serde(default, deserialize_with = "deserialize_rotation")]
    rotation: Option<f64>,
    #[serde(default)]
    size: Option<i32>,
    #[serde(default)]
    top: Option<f64>,
    #[serde(default)]
    bottom: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct ApiMapsData {
    maps: HashMap<String, ApiMap>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiMap {
    name: String,
    normalized_name: String,
    #[serde(default)]
    spawns: Vec<ApiSpawn>,
    #[serde(default)]
    extracts: Vec<ApiExtract>,
}

#[derive(Debug, Deserialize)]
struct ApiSpawn {
    position: ApiPosition,
    #[serde(default)]
    sides: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiExtract {
    name: String,
    faction: String,
    #[serde(default)]
    position: Option<ApiPosition>,
}

#[derive(Debug, Deserialize)]
struct ApiPosition {
    x: f64,
    y: f64,
    z: f64,
}

struct ApiMapData {
    names: HashMap<String, String>,
    spawns: HashMap<String, Vec<Spawn>>,
    extracts: HashMap<String, Vec<Extract>>,
}

fn deserialize_rotation<'de, D>(deserializer: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => Ok(n.as_f64()),
        Some(serde_json::Value::String(s)) => s
            .parse()
            .map(Some)
            .map_err(|_| D::Error::custom(format!("invalid rotation string: {s}"))),
        Some(other) => Err(D::Error::custom(format!(
            "expected number or string for rotation, got: {other:?}"
        ))),
    }
}

impl From<FetchedLayer> for Layer {
    fn from(f: FetchedLayer) -> Self {
        Self {
            name: f.name,
            image_path: f.image_path,
            svg_layer: f.svg_layer,
            tile_path: f.tile_path,
            show: f.show,
            extents: f.extents.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<FetchedExtent> for Extent {
    fn from(f: FetchedExtent) -> Self {
        Self {
            height: f.height,
            bounds: f.bounds.map(|b| b.into_iter().map(Into::into).collect()),
        }
    }
}

impl From<FetchedExtentBound> for ExtentBound {
    fn from(f: FetchedExtentBound) -> Self {
        Self {
            point1: f.point1,
            point2: f.point2,
            name: f.name,
        }
    }
}

impl From<FetchedLabel> for Label {
    fn from(f: FetchedLabel) -> Self {
        Self {
            position: f.position,
            text: f.text,
            rotation: f.rotation,
            size: f.size,
            top: f.top,
            bottom: f.bottom,
        }
    }
}

async fn fetch_api_map_data(client: &reqwest::Client) -> Result<ApiMapData, FetchError> {
    let maps_response = client
        .get(TARKOV_DEV_MAPS_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;
    if !maps_response.status().is_success() {
        return Err(FetchError::HttpStatus {
            resource: "tarkov.dev map data".into(),
            status: maps_response.status().as_u16(),
        });
    }
    let api_maps: ApiResponse<ApiMapsData> = maps_response.json().await?;

    let names_response = client
        .get(TARKOV_DEV_MAPS_EN_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;
    if !names_response.status().is_success() {
        return Err(FetchError::HttpStatus {
            resource: "tarkov.dev English translations".into(),
            status: names_response.status().as_u16(),
        });
    }
    let translations: ApiResponse<HashMap<String, String>> = names_response.json().await?;

    let mut names = HashMap::new();
    let mut spawns = HashMap::new();
    let mut extracts = HashMap::new();

    for map in api_maps.data.maps.into_values() {
        let normalized_name = map.normalized_name;
        names.insert(
            normalized_name.clone(),
            translations
                .data
                .get(&map.name)
                .cloned()
                .unwrap_or(map.name),
        );
        spawns.insert(
            normalized_name.clone(),
            map.spawns
                .into_iter()
                .filter(|spawn| {
                    spawn
                        .sides
                        .iter()
                        .any(|side| side == "pmc" || side == "all")
                        && spawn.categories.iter().any(|category| category == "player")
                })
                .map(|spawn| Spawn {
                    position: [spawn.position.x, spawn.position.y, spawn.position.z],
                    sides: spawn.sides,
                    categories: spawn.categories,
                })
                .collect(),
        );
        extracts.insert(
            normalized_name,
            map.extracts
                .into_iter()
                .map(|extract| Extract {
                    name: translations
                        .data
                        .get(&extract.name)
                        .cloned()
                        .unwrap_or(extract.name),
                    faction: extract.faction,
                    position: extract.position.map(|p| [p.x, p.y, p.z]),
                })
                .collect(),
        );
    }

    Ok(ApiMapData {
        names,
        spawns,
        extracts,
    })
}

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// Encodes one PNG into a `.bc7z` container: premultiply alpha (the app's
/// texture convention; also stops BC7 blocks leaking color into transparent
/// regions), pad to multiples of 4, BC7-compress, zstd-pack.
fn encode_png_to_bc7z(png_path: &std::path::Path) -> Result<Vec<u8>, FetchError> {
    let mut rgba = image::open(png_path)?.to_rgba8();
    let (width, height) = rgba.dimensions();

    for px in rgba.chunks_exact_mut(4) {
        let alpha = px[3] as u32;
        px[0] = (px[0] as u32 * alpha / 255) as u8;
        px[1] = (px[1] as u32 * alpha / 255) as u8;
        px[2] = (px[2] as u32 * alpha / 255) as u8;
    }

    let padded = [width.div_ceil(4) * 4, height.div_ceil(4) * 4];
    if padded != [width, height] {
        let mut canvas = RgbaImage::new(padded[0], padded[1]);
        image::imageops::overlay(&mut canvas, &rgba, 0, 0);
        rgba = canvas;
    }

    let blocks = intel_tex_2::bc7::compress_blocks(
        &intel_tex_2::bc7::alpha_basic_settings(),
        &intel_tex_2::RgbaSurface {
            width: padded[0],
            height: padded[1],
            stride: padded[0] * 4,
            data: &rgba,
        },
    );

    Ok(tarkov_map::bc7z::pack(&tarkov_map::bc7z::Bc7Image {
        pixel_size: [width, height],
        padded_size: padded,
        blocks,
    })?)
}

/// Maps an `image_path` entry (either `maps/X.png` or `maps/X.bc7z`) to its
/// PNG source stem, e.g. `maps/customs`.
fn image_path_stem(entry: &str) -> &str {
    entry
        .strip_suffix(".png")
        .or_else(|| entry.strip_suffix(".bc7z"))
        .unwrap_or(entry)
}

/// Encodes every image referenced by `maps` into a `.bc7z` next to its PNG
/// source and rewrites the `image_path` entries to point at the containers.
/// Existing up-to-date containers are skipped unless `force` is set.
fn encode_all_assets(maps: &mut TarkovMaps, force: bool) -> Result<(), FetchError> {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    let mut stems = BTreeSet::new();
    for map in maps.iter() {
        stems.insert(image_path_stem(&map.image_path).to_owned());
        for layer in map.layers.iter().flatten() {
            if let Some(path) = &layer.image_path {
                stems.insert(image_path_stem(path).to_owned());
            }
        }
    }

    let jobs: Vec<String> = stems
        .iter()
        .filter(|stem| force || !repo_path(&format!("assets/{stem}.bc7z")).exists())
        .cloned()
        .collect();

    if !jobs.is_empty() {
        println!("Encoding {} images to BC7+zstd...", jobs.len());
        let progress = ProgressBar::new(jobs.len() as u64);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} encoded ({eta})")?
                .progress_chars("=>-"),
        );

        let queue = Mutex::new(jobs.into_iter());
        let failures: Mutex<Vec<FetchError>> = Mutex::new(Vec::new());
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let Some(stem) = queue.lock().unwrap().next() else {
                            return;
                        };
                        let png = repo_path(&format!("assets/{stem}.png"));
                        let result = if png.exists() {
                            encode_png_to_bc7z(&png).and_then(|container| {
                                Ok(std::fs::write(
                                    repo_path(&format!("assets/{stem}.bc7z")),
                                    container,
                                )?)
                            })
                        } else {
                            Err(FetchError::MissingPngSource {
                                path: format!("assets/{stem}.png"),
                            })
                        };
                        if let Err(error) = result {
                            failures.lock().unwrap().push(error);
                        }
                        progress.inc(1);
                    }
                });
            }
        });
        progress.finish_and_clear();

        let failures = failures.into_inner().unwrap();
        if !failures.is_empty() {
            for error in failures.iter().skip(1) {
                eprintln!("  encoding also failed: {error}");
            }
            return Err(failures.into_iter().next().unwrap());
        }
    }

    // Rewrite entries to the containers.
    for map in maps.iter_mut() {
        map.image_path = format!("{}.bc7z", image_path_stem(&map.image_path));
        for layer in map.layers.iter_mut().flatten() {
            if let Some(path) = &mut layer.image_path {
                *path = format!("{}.bc7z", image_path_stem(path));
            }
        }
    }

    Ok(())
}

fn write_maps_ron(maps: &TarkovMaps) -> Result<(), FetchError> {
    let pretty_config = PrettyConfig::new()
        .depth_limit(10)
        .indentor("  ".to_owned())
        .struct_names(true)
        .enumerate_arrays(false);

    let ron_string = ron::ser::to_string_pretty(maps, pretty_config)?;
    println!("Serialized to {} bytes of RON", ron_string.len());

    std::fs::create_dir_all(repo_path(MAPS_DIR))?;

    let output_path = repo_path(MAPS_RON_PATH);
    std::fs::write(&output_path, &ron_string)?;
    println!("Wrote maps to {}", output_path.display());
    Ok(())
}

struct ImageResult {
    image_path: String,
    image_size: [f32; 2],
}

async fn process_svg_map(
    client: &reqwest::Client,
    normalized_name: &str,
    svg_url: &str,
    force: bool,
) -> Result<ImageResult, FetchError> {
    let image_relative = format!("{MAPS_PATH_PREFIX}/{normalized_name}.png");
    let image_disk_path = repo_path(&format!("{MAPS_DIR}/{normalized_name}.png"));

    if !force && image_disk_path.exists() {
        let img = image::open(&image_disk_path)?;
        let source_size = [
            img.width() as f32 / SVG_RENDER_SCALE,
            img.height() as f32 / SVG_RENDER_SCALE,
        ];
        return Ok(ImageResult {
            image_path: image_relative,
            image_size: source_size,
        });
    }

    let response = client
        .get(svg_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(FetchError::HttpStatus {
            resource: "SVG".into(),
            status: response.status().as_u16(),
        });
    }

    let svg_bytes = response.bytes().await?;
    let tree = Tree::from_data(&svg_bytes, &Options::default())
        .map_err(|e| FetchError::SvgParse(e.to_string()))?;

    let source_size = [tree.size().width(), tree.size().height()];
    let render_w = (source_size[0] * SVG_RENDER_SCALE) as u32;
    let render_h = (source_size[1] * SVG_RENDER_SCALE) as u32;

    let mut pixmap = Pixmap::new(render_w, render_h).ok_or(FetchError::PixmapCreation)?;

    resvg::render(
        &tree,
        Transform::from_scale(SVG_RENDER_SCALE, SVG_RENDER_SCALE),
        &mut pixmap.as_mut(),
    );

    if let Some(parent) = image_disk_path.parent() {
        async_fs::create_dir_all(parent).await?;
    }
    pixmap
        .save_png(&image_disk_path)
        .map_err(|e| FetchError::PngSave(e.to_string()))?;

    Ok(ImageResult {
        image_path: image_relative,
        image_size: source_size,
    })
}

#[allow(clippy::too_many_arguments)]
async fn process_tile_map(
    client: &reqwest::Client,
    normalized_name: &str,
    remote_template: &str,
    tile_size: i32,
    min_zoom: i32,
    max_zoom: i32,
    zoom_offset: i32,
    multi_progress: &MultiProgress,
    force: bool,
) -> Result<ImageResult, FetchError> {
    let image_relative = format!("{MAPS_PATH_PREFIX}/{normalized_name}.png");
    let image_disk_path = repo_path(&format!("{MAPS_DIR}/{normalized_name}.png"));

    let zoom = (max_zoom - zoom_offset).max(min_zoom);
    let tiles_per_axis = 1u32 << zoom;
    let full_size = tiles_per_axis * tile_size as u32;
    let source_size = [tile_size as f32, tile_size as f32];

    if !force && image_disk_path.exists() {
        return Ok(ImageResult {
            image_path: image_relative,
            image_size: source_size,
        });
    }

    let tile_pb = multi_progress.add(ProgressBar::new((tiles_per_axis * tiles_per_axis) as u64));
    tile_pb.set_style(
        ProgressStyle::default_bar()
            .template("    {spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} tiles ({eta})")?
            .progress_chars("=>-"),
    );

    let semaphore = Arc::new(Semaphore::new(TILE_DOWNLOAD_CONCURRENCY));
    let tile_pb = Arc::new(tile_pb);
    let mut join_set: JoinSet<TileResult> = JoinSet::new();

    for x in 0..tiles_per_axis {
        for y in 0..tiles_per_axis {
            let remote_url = remote_template
                .replace("{z}", &zoom.to_string())
                .replace("{x}", &x.to_string())
                .replace("{y}", &y.to_string());

            let client = client.clone();
            let semaphore = semaphore.clone();
            let tile_pb = tile_pb.clone();

            join_set.spawn(async move {
                let _permit = semaphore.acquire_owned().await?;

                let response = client
                    .get(&remote_url)
                    .header(reqwest::header::USER_AGENT, USER_AGENT)
                    .send()
                    .await?;

                if !response.status().is_success() {
                    return Err(FetchError::HttpStatus {
                        resource: "tile".into(),
                        status: response.status().as_u16(),
                    });
                }

                let bytes = response.bytes().await?.to_vec();
                tile_pb.inc(1);
                Ok((x, y, bytes))
            });
        }
    }

    let mut tiles = Vec::new();
    while let Some(result) = join_set.join_next().await {
        tiles.push(result??);
    }
    tile_pb.finish_and_clear();

    let compose_pb = multi_progress.add(ProgressBar::new(tiles.len() as u64));
    compose_pb.set_style(
        ProgressStyle::default_bar()
            .template("    {spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} composing")?
            .progress_chars("=>-"),
    );

    let mut full_image: RgbaImage = ImageBuffer::new(full_size, full_size);

    for (x, y, bytes) in tiles {
        if let Ok(tile) = image::load_from_memory(&bytes) {
            let tile_rgba = tile.to_rgba8();
            let offset_x = x * tile_size as u32;
            let offset_y = y * tile_size as u32;

            for (tx, ty, pixel) in tile_rgba.enumerate_pixels() {
                let fx = offset_x + tx;
                let fy = offset_y + ty;
                if fx < full_size && fy < full_size {
                    full_image.put_pixel(fx, fy, *pixel);
                }
            }
        }
        compose_pb.inc(1);
    }

    compose_pb.finish_and_clear();

    if let Some(parent) = image_disk_path.parent() {
        async_fs::create_dir_all(parent).await?;
    }
    full_image.save(&image_disk_path)?;

    Ok(ImageResult {
        image_path: image_relative,
        image_size: source_size,
    })
}

#[allow(clippy::too_many_arguments)]
async fn convert_group(
    client: &reqwest::Client,
    fetched: FetchedMapGroup,
    map_names: &HashMap<String, String>,
    map_spawns: &HashMap<String, Vec<Spawn>>,
    map_extracts: &HashMap<String, Vec<Extract>>,
    multi_progress: &MultiProgress,
    force: bool,
    tile_zoom_offset: i32,
) -> Result<Option<Map>, FetchError> {
    let FetchedMapGroup {
        normalized_name,
        maps,
    } = fetched;

    let Some(mut interactive) = maps.into_iter().find(|m| m.projection == "interactive") else {
        return Ok(None);
    };

    let name =
        map_names
            .get(&normalized_name)
            .cloned()
            .ok_or_else(|| FetchError::MissingMapName {
                name: normalized_name.clone(),
            })?;

    let try_tiles =
        |interactive: &FetchedMap| -> Result<Option<(String, i32, i32, i32)>, FetchError> {
            match &interactive.tile_path {
                Some(tile_template) => {
                    let min_zoom =
                        interactive
                            .min_zoom
                            .ok_or_else(|| FetchError::MissingMinZoom {
                                name: normalized_name.clone(),
                            })?;
                    let max_zoom =
                        interactive
                            .max_zoom
                            .ok_or_else(|| FetchError::MissingMaxZoom {
                                name: normalized_name.clone(),
                            })?;
                    let tile_size = interactive.tile_size.unwrap_or(256);
                    Ok(Some((tile_template.clone(), tile_size, min_zoom, max_zoom)))
                }
                None => Ok(None),
            }
        };

    let result = match &interactive.svg_path {
        Some(svg_url) => {
            match process_svg_map(client, &normalized_name, svg_url, force).await {
                Ok(r) => r,
                Err(svg_err) => {
                    // SVG failed — try falling back to tiles if available
                    if let Some((tile_template, tile_size, min_zoom, max_zoom)) =
                        try_tiles(&interactive)?
                    {
                        multi_progress.println(format!(
                            "  Warning: SVG failed for {normalized_name} ({svg_err}), falling back to tiles"
                        ))?;
                        process_tile_map(
                            client,
                            &normalized_name,
                            &tile_template,
                            tile_size,
                            min_zoom,
                            max_zoom,
                            tile_zoom_offset,
                            multi_progress,
                            force,
                        )
                        .await?
                    } else {
                        return Err(svg_err);
                    }
                }
            }
        }
        None => {
            if let Some((tile_template, tile_size, min_zoom, max_zoom)) = try_tiles(&interactive)? {
                process_tile_map(
                    client,
                    &normalized_name,
                    &tile_template,
                    tile_size,
                    min_zoom,
                    max_zoom,
                    tile_zoom_offset,
                    multi_progress,
                    force,
                )
                .await?
            } else {
                return Err(FetchError::MissingMapSource {
                    name: normalized_name,
                });
            }
        }
    };

    let logical_size = interactive
        .bounds
        .map(|bounds| {
            let width = (bounds[0][0] - bounds[1][0]).abs() as f32;
            let height = (bounds[1][1] - bounds[0][1]).abs() as f32;
            [width, height]
        })
        .unwrap_or(result.image_size);

    if let Some(layers) = &mut interactive.layers {
        for (index, layer) in layers.iter_mut().enumerate() {
            let Some(tile_template) = layer.tile_path.as_deref() else {
                continue;
            };

            if interactive.tile_path.as_deref() == Some(tile_template) {
                layer.image_path = Some(result.image_path.clone());
                continue;
            }

            let min_zoom = interactive
                .min_zoom
                .ok_or_else(|| FetchError::MissingMinZoom {
                    name: normalized_name.clone(),
                })?;
            let max_zoom = interactive
                .max_zoom
                .ok_or_else(|| FetchError::MissingMaxZoom {
                    name: normalized_name.clone(),
                })?;
            let tile_size = interactive.tile_size.unwrap_or(256);
            let asset_name = format!("{normalized_name}-layer-{index}");

            match process_tile_map(
                client,
                &asset_name,
                tile_template,
                tile_size,
                min_zoom,
                max_zoom,
                tile_zoom_offset,
                multi_progress,
                force,
            )
            .await
            {
                Ok(layer_result) => layer.image_path = Some(layer_result.image_path),
                Err(error) => multi_progress.println(format!(
                    "  Warning: failed to process {normalized_name} layer '{}': {error}",
                    layer.name
                ))?,
            }
        }
    }

    Ok(Some(Map {
        normalized_name: normalized_name.clone(),
        name,
        image_path: result.image_path,
        image_size: result.image_size,
        logical_size,
        alt_maps: interactive.alt_maps,
        author: interactive.author,
        author_link: interactive.author_link,
        transform: interactive.transform,
        coordinate_rotation: interactive.coordinate_rotation,
        bounds: interactive.bounds,
        height_range: interactive.height_range,
        layers: interactive
            .layers
            .map(|l| l.into_iter().map(Into::into).collect()),
        labels: interactive
            .labels
            .map(|l| l.into_iter().map(Into::into).collect()),
        spawns: map_spawns.get(&normalized_name).cloned(),
        extracts: map_extracts.get(&normalized_name).cloned(),
    }))
}

#[tokio::main]
async fn main() -> Result<(), FetchError> {
    env_logger::init();

    let args = Args::parse();

    if args.convert_only {
        println!("Convert-only: encoding existing PNGs referenced by maps.ron");
        let ron_string = std::fs::read_to_string(repo_path(MAPS_RON_PATH))?;
        let mut maps: TarkovMaps =
            ron::from_str(&ron_string).map_err(|e| e.code)?;
        encode_all_assets(&mut maps, args.force)?;
        write_maps_ron(&maps)?;
        return Ok(());
    }

    if args.force {
        println!("Force mode enabled - re-processing all assets");
    }

    let client = reqwest::Client::new();

    println!("Fetching map data from tarkov.dev...");
    let api_data = fetch_api_map_data(&client).await?;
    let map_names = api_data.names;
    let map_spawns = api_data.spawns;
    let map_extracts = api_data.extracts;
    println!("Fetched {} map names", map_names.len());
    let total_spawns: usize = map_spawns.values().map(Vec::len).sum();
    println!("Fetched {total_spawns} PMC spawns");
    let total_extracts: usize = map_extracts.values().map(Vec::len).sum();
    println!("Fetched {total_extracts} extracts");

    println!("Fetching maps from tarkov-dev...");

    let response = client
        .get(MAPS_JSON_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(FetchError::HttpStatus {
            resource: "maps.json".into(),
            status: response.status().as_u16(),
        });
    }

    let json_text = response.text().await?;
    println!("Fetched {} bytes of JSON", json_text.len());

    let fetched_maps: Vec<FetchedMapGroup> = serde_json::from_str(&json_text)?;
    println!("Parsed {} map groups\n", fetched_maps.len());

    let multi_progress = MultiProgress::new();
    let maps_pb = multi_progress.add(ProgressBar::new(fetched_maps.len() as u64));
    maps_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} maps - {msg}")?
            .progress_chars("=>-"),
    );

    let mut skipped = 0usize;
    let mut maps: TarkovMaps = Vec::new();

    for group in fetched_maps {
        let group_name = group.normalized_name.clone();
        maps_pb.set_message(group_name.clone());

        match convert_group(
            &client,
            group,
            &map_names,
            &map_spawns,
            &map_extracts,
            &multi_progress,
            args.force,
            args.tile_zoom_offset,
        )
        .await
        {
            Ok(Some(map)) => maps.push(map),
            Ok(None) => skipped += 1,
            Err(e) => {
                multi_progress.println(format!("  Warning: skipping {group_name}: {e}"))?;
                skipped += 1;
            }
        }

        maps_pb.inc(1);
    }

    maps_pb.finish_with_message("Done");

    println!(
        "\nProcessed {} interactive maps (skipped {skipped})",
        maps.len()
    );

    encode_all_assets(&mut maps, args.force)?;
    write_maps_ron(&maps)?;

    println!("\nMaps:");
    for map in &maps {
        println!("  - {} ({})", map.name, map.normalized_name);
    }

    Ok(())
}
