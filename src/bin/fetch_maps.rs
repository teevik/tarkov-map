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

    #[error("GraphQL error: {0}")]
    GraphQL(String),

    #[error("GraphQL response missing data")]
    GraphQLMissingData,

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
}

/// Result of downloading a single tile.
type TileResult = Result<(u32, u32, Vec<u8>), FetchError>;

#[cynic::schema("tarkov")]
pub mod schema {}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
struct MapNamesQuery {
    #[cynic(flatten)]
    maps: Vec<MapNameFragment>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Map")]
struct MapNameFragment {
    normalized_name: String,
    name: String,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
struct MapSpawnsQuery {
    #[cynic(flatten)]
    maps: Vec<MapSpawnsFragment>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Map")]
struct MapSpawnsFragment {
    normalized_name: String,
    #[cynic(flatten)]
    spawns: Vec<MapSpawnFragment>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "MapSpawn")]
struct MapSpawnFragment {
    position: MapPositionFragment,
    #[cynic(flatten)]
    sides: Vec<String>,
    #[cynic(flatten)]
    categories: Vec<String>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "MapPosition")]
struct MapPositionFragment {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
struct MapExtractsQuery {
    #[cynic(flatten)]
    maps: Vec<MapExtractsFragment>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Map")]
struct MapExtractsFragment {
    normalized_name: String,
    #[cynic(flatten)]
    extracts: Vec<MapExtractFragment>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "MapExtract")]
struct MapExtractFragment {
    name: Option<String>,
    faction: Option<String>,
    position: Option<MapPositionFragment>,
}

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
}

const MAPS_JSON_URL: &str =
    "https://raw.githubusercontent.com/the-hideout/tarkov-dev/main/src/data/maps.json";
const TARKOV_DEV_GRAPHQL_URL: &str = "https://api.tarkov.dev/graphql";
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

async fn fetch_graphql<Q, T>(
    client: &reqwest::Client,
    operation: cynic::Operation<Q, ()>,
) -> Result<T, FetchError>
where
    Q: serde::de::DeserializeOwned,
    T: From<Q>,
{
    let response: cynic::GraphQlResponse<Q> = client
        .post(TARKOV_DEV_GRAPHQL_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(&operation)
        .send()
        .await?
        .json()
        .await?;

    if let Some(errors) = response.errors.filter(|e| !e.is_empty()) {
        let messages: Vec<_> = errors.into_iter().map(|e| e.message).collect();
        return Err(FetchError::GraphQL(messages.join("; ")));
    }

    response
        .data
        .map(Into::into)
        .ok_or(FetchError::GraphQLMissingData)
}

async fn fetch_map_names(client: &reqwest::Client) -> Result<HashMap<String, String>, FetchError> {
    use cynic::QueryBuilder;

    let data: MapNamesQuery = fetch_graphql(client, MapNamesQuery::build(())).await?;

    Ok(data
        .maps
        .into_iter()
        .map(|m| (m.normalized_name, m.name))
        .collect())
}

async fn fetch_map_spawns(
    client: &reqwest::Client,
) -> Result<HashMap<String, Vec<Spawn>>, FetchError> {
    use cynic::QueryBuilder;

    let data: MapSpawnsQuery = fetch_graphql(client, MapSpawnsQuery::build(())).await?;

    Ok(data
        .maps
        .into_iter()
        .map(|map| {
            let spawns = map
                .spawns
                .into_iter()
                .filter(|s| {
                    s.sides.iter().any(|side| side == "pmc" || side == "all")
                        && s.categories.iter().any(|cat| cat == "player")
                })
                .map(|s| Spawn {
                    position: [s.position.x, s.position.y, s.position.z],
                    sides: s.sides,
                    categories: s.categories,
                })
                .collect();
            (map.normalized_name, spawns)
        })
        .collect())
}

async fn fetch_map_extracts(
    client: &reqwest::Client,
) -> Result<HashMap<String, Vec<Extract>>, FetchError> {
    use cynic::QueryBuilder;

    let data: MapExtractsQuery = fetch_graphql(client, MapExtractsQuery::build(())).await?;

    Ok(data
        .maps
        .into_iter()
        .map(|map| {
            let extracts = map
                .extracts
                .into_iter()
                .filter_map(|e| {
                    Some(Extract {
                        name: e.name?,
                        faction: e.faction?,
                        position: e.position.map(|p| [p.x, p.y, p.z]),
                    })
                })
                .collect();
            (map.normalized_name, extracts)
        })
        .collect())
}

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
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

    let Some(interactive) = maps.into_iter().find(|m| m.projection == "interactive") else {
        return Ok(None);
    };

    let name =
        map_names
            .get(&normalized_name)
            .cloned()
            .ok_or_else(|| FetchError::MissingMapName {
                name: normalized_name.clone(),
            })?;

    let result = match (&interactive.svg_path, &interactive.tile_path) {
        (Some(svg_url), _) => process_svg_map(client, &normalized_name, svg_url, force).await?,
        (_, Some(tile_template)) => {
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

            process_tile_map(
                client,
                &normalized_name,
                tile_template,
                tile_size,
                min_zoom,
                max_zoom,
                tile_zoom_offset,
                multi_progress,
                force,
            )
            .await?
        }
        _ => {
            return Err(FetchError::MissingMapSource {
                name: normalized_name,
            });
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

    if args.force {
        println!("Force mode enabled - re-processing all assets");
    }

    let client = reqwest::Client::new();

    println!("Fetching map data from tarkov.dev...");
    let map_names = fetch_map_names(&client).await?;
    println!("Fetched {} map names", map_names.len());

    println!("Fetching PMC spawns from tarkov.dev...");
    let map_spawns = fetch_map_spawns(&client).await?;
    let total_spawns: usize = map_spawns.values().map(Vec::len).sum();
    println!("Fetched {total_spawns} PMC spawns");

    println!("Fetching extracts from tarkov.dev...");
    let map_extracts = fetch_map_extracts(&client).await?;
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
        .await?
        {
            Some(map) => maps.push(map),
            None => skipped += 1,
        }

        maps_pb.inc(1);
    }

    maps_pb.finish_with_message("Done");

    println!(
        "\nProcessed {} interactive maps (skipped {skipped})",
        maps.len()
    );

    let pretty_config = PrettyConfig::new()
        .depth_limit(10)
        .indentor("  ".to_owned())
        .struct_names(true)
        .enumerate_arrays(false);

    let ron_string = ron::ser::to_string_pretty(&maps, pretty_config)?;
    println!("Serialized to {} bytes of RON", ron_string.len());

    std::fs::create_dir_all(repo_path(MAPS_DIR))?;

    let output_path = repo_path(MAPS_RON_PATH);
    std::fs::write(&output_path, &ron_string)?;
    println!("Wrote maps to {}", output_path.display());

    println!("\nMaps:");
    for map in &maps {
        println!("  - {} ({})", map.name, map.normalized_name);
    }

    Ok(())
}
