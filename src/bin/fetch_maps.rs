use clap::Parser;
use image::{ImageBuffer, RgbaImage};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use resvg::tiny_skia::Pixmap;
use resvg::usvg::{Options, Transform, Tree};
use ron::ser::PrettyConfig;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tarkov_map::{Extent, ExtentBound, Label, Layer, Map, Spawn, TarkovMaps};
use tokio::fs as tokio_fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Fetch Tarkov map assets from tarkov-dev
#[derive(Parser, Debug)]
#[command(name = "fetch_maps", version, about)]
struct Args {
    /// Force re-download of all assets, ignoring cached files
    #[arg(short, long)]
    force: bool,

    /// Reduce tile map zoom level by this amount from max (0 = max quality, 1 = half, 2 = quarter, etc.)
    /// Default is 2 for reasonable file sizes. Use 0 for highest quality (warning: very large files).
    #[arg(long, default_value = "2")]
    tile_zoom_offset: i32,
}

/// GitHub raw content URL for maps.json
const MAPS_JSON_URL: &str =
    "https://raw.githubusercontent.com/the-hideout/tarkov-dev/main/src/data/maps.json";

/// tarkov.dev GraphQL API endpoint
const TARKOV_DEV_GRAPHQL_URL: &str = "https://api.tarkov.dev/graphql";

const USER_AGENT: &str = "tarkov-map";

const MAPS_RON_PATH: &str = "assets/maps.ron";

// Map images are stored under `assets/maps/`.
const MAPS_DIR: &str = "assets/maps";

const TILE_DOWNLOAD_CONCURRENCY: usize = 32;

/// Scale factor for rendering SVGs to high-res PNGs
const SVG_RENDER_SCALE: f32 = 2.0;

// ============================================================================
// Fetched types - match the JSON structure exactly
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedMapGroup {
    normalized_name: String,
    maps: Vec<FetchedMap>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
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
    svg_layer: Option<String>,
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
        let point1 = values
            .first()
            .and_then(|v| v.as_array())
            .map(|arr| {
                [
                    arr.first().and_then(|v| v.as_f64()).unwrap_or(0.0),
                    arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                ]
            })
            .unwrap_or([0.0, 0.0]);

        let point2 = values
            .get(1)
            .and_then(|v| v.as_array())
            .map(|arr| {
                [
                    arr.first().and_then(|v| v.as_f64()).unwrap_or(0.0),
                    arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                ]
            })
            .unwrap_or([0.0, 0.0]);

        let name = values
            .get(2)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Self {
            point1,
            point2,
            name,
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

fn deserialize_rotation<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;

    match value {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => Ok(n.as_f64()),
        Some(serde_json::Value::String(s)) => s
            .parse::<f64>()
            .map(Some)
            .map_err(|_| D::Error::custom(format!("invalid rotation string: {}", s))),
        Some(other) => Err(D::Error::custom(format!(
            "expected number or string for rotation, got: {:?}",
            other
        ))),
    }
}

// ============================================================================
// tarkov.dev GraphQL map names
// ============================================================================

#[derive(Debug, Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct MapNamesData {
    maps: Vec<MapNameEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MapNameEntry {
    normalized_name: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct MapSpawnsData {
    maps: Vec<MapSpawnsEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MapSpawnsEntry {
    normalized_name: String,
    spawns: Vec<FetchedSpawn>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedSpawn {
    position: FetchedPosition,
    sides: Vec<String>,
    categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FetchedPosition {
    x: f64,
    y: f64,
    z: f64,
}

async fn fetch_map_names(client: &reqwest::Client) -> color_eyre::Result<HashMap<String, String>> {
    let query = "{ maps { normalizedName name } }";

    let response = client
        .post(TARKOV_DEV_GRAPHQL_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(color_eyre::eyre::eyre!(
            "Failed to fetch map names: {}",
            response.status()
        ));
    }

    let gql: GraphQlResponse<MapNamesData> = response.json().await?;
    if !gql.errors.is_empty() {
        let messages = gql
            .errors
            .into_iter()
            .map(|err| err.message)
            .collect::<Vec<_>>()
            .join("; ");

        return Err(color_eyre::eyre::eyre!("GraphQL errors: {}", messages));
    }

    let data = gql
        .data
        .ok_or_else(|| color_eyre::eyre::eyre!("GraphQL response missing data"))?;

    Ok(data
        .maps
        .into_iter()
        .map(|map| (map.normalized_name, map.name))
        .collect())
}

async fn fetch_map_spawns(
    client: &reqwest::Client,
) -> color_eyre::Result<HashMap<String, Vec<Spawn>>> {
    let query = "{ maps { normalizedName spawns { position { x y z } sides categories } } }";

    let response = client
        .post(TARKOV_DEV_GRAPHQL_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(color_eyre::eyre::eyre!(
            "Failed to fetch map spawns: {}",
            response.status()
        ));
    }

    let gql: GraphQlResponse<MapSpawnsData> = response.json().await?;
    if !gql.errors.is_empty() {
        let messages = gql
            .errors
            .into_iter()
            .map(|err| err.message)
            .collect::<Vec<_>>()
            .join("; ");

        return Err(color_eyre::eyre::eyre!("GraphQL errors: {}", messages));
    }

    let data = gql
        .data
        .ok_or_else(|| color_eyre::eyre::eyre!("GraphQL response missing data"))?;

    Ok(data
        .maps
        .into_iter()
        .map(|map| {
            let spawns = map
                .spawns
                .into_iter()
                // Filter to only PMC spawns (sides contains "pmc" or "all", categories contains "player")
                .filter(|s| {
                    (s.sides.iter().any(|side| side == "pmc" || side == "all"))
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

// ============================================================================
// Asset processing
// ============================================================================

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// Result of processing a map image
struct ImageResult {
    image_path: String,
    image_size: [f32; 2],
}

/// Download SVG, render to high-res PNG
async fn process_svg_map(
    client: &reqwest::Client,
    normalized_name: &str,
    svg_url: &str,
    force: bool,
) -> color_eyre::Result<ImageResult> {
    let image_relative = format!("{}/{}.png", MAPS_DIR, normalized_name);
    let image_path = repo_path(&image_relative);

    // Check if already processed
    if !force && image_path.exists() {
        // Read dimensions from existing PNG
        let img = image::open(&image_path)?;
        let source_size = [
            img.width() as f32 / SVG_RENDER_SCALE,
            img.height() as f32 / SVG_RENDER_SCALE,
        ];
        return Ok(ImageResult {
            image_path: image_relative,
            image_size: source_size,
        });
    }

    // Download SVG
    let response = client
        .get(svg_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(color_eyre::eyre::eyre!(
            "Failed to fetch SVG: {}",
            response.status()
        ));
    }

    let svg_bytes = response.bytes().await?;

    // Parse SVG
    let options = Options::default();
    let tree = Tree::from_data(&svg_bytes, &options)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to parse SVG: {}", e))?;

    let source_size = [tree.size().width(), tree.size().height()];
    let render_w = (source_size[0] * SVG_RENDER_SCALE) as u32;
    let render_h = (source_size[1] * SVG_RENDER_SCALE) as u32;

    // Render to pixmap
    let mut pixmap = Pixmap::new(render_w, render_h)
        .ok_or_else(|| color_eyre::eyre::eyre!("Failed to create pixmap"))?;

    resvg::render(
        &tree,
        Transform::from_scale(SVG_RENDER_SCALE, SVG_RENDER_SCALE),
        &mut pixmap.as_mut(),
    );

    // Save as PNG
    tokio_fs::create_dir_all(image_path.parent().unwrap()).await?;
    pixmap
        .save_png(&image_path)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to save PNG: {}", e))?;

    Ok(ImageResult {
        image_path: image_relative,
        image_size: source_size,
    })
}

/// Download tiles and compose into single high-res PNG
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
) -> color_eyre::Result<ImageResult> {
    let image_relative = format!("{}/{}.png", MAPS_DIR, normalized_name);
    let image_path = repo_path(&image_relative);

    // Use max_zoom minus offset, but not below min_zoom
    let zoom = (max_zoom - zoom_offset).max(min_zoom);
    let tiles_per_axis = 1u32 << zoom;
    let full_size = tiles_per_axis * tile_size as u32;

    // Source size at zoom 0 (1 tile)
    let source_size = [tile_size as f32, tile_size as f32];

    // Check if already processed
    if !force && image_path.exists() {
        return Ok(ImageResult {
            image_path: image_relative,
            image_size: source_size,
        });
    }

    // Download tiles directly into memory and compose
    let tile_pb = multi_progress.add(ProgressBar::new((tiles_per_axis * tiles_per_axis) as u64));
    tile_pb.set_style(
        ProgressStyle::default_bar()
            .template("    {spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} tiles ({eta})")
            .unwrap()
            .progress_chars("=>-"),
    );

    let semaphore = Arc::new(Semaphore::new(TILE_DOWNLOAD_CONCURRENCY));
    let tile_pb = Arc::new(tile_pb);
    let mut join_set: JoinSet<color_eyre::Result<(u32, u32, Vec<u8>)>> = JoinSet::new();

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
                    return Err(color_eyre::eyre::eyre!(
                        "Failed to fetch tile: {}",
                        response.status()
                    ));
                }

                let bytes = response.bytes().await?.to_vec();
                tile_pb.inc(1);
                Ok((x, y, bytes))
            });
        }
    }

    // Collect all tiles
    let mut tiles: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    while let Some(result) = join_set.join_next().await {
        tiles.push(result??);
    }
    tile_pb.finish_and_clear();

    // Compose tiles into single image
    let compose_pb = multi_progress.add(ProgressBar::new(tiles.len() as u64));
    compose_pb.set_style(
        ProgressStyle::default_bar()
            .template("    {spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} composing")
            .unwrap()
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

    // Save composed image
    tokio_fs::create_dir_all(image_path.parent().unwrap()).await?;
    full_image.save(&image_path)?;

    Ok(ImageResult {
        image_path: image_relative,
        image_size: source_size,
    })
}

// ============================================================================
// Conversion from Fetched types to lib types
// ============================================================================

async fn convert_group(
    client: &reqwest::Client,
    fetched: FetchedMapGroup,
    map_names: &HashMap<String, String>,
    map_spawns: &HashMap<String, Vec<Spawn>>,
    multi_progress: &MultiProgress,
    force: bool,
    tile_zoom_offset: i32,
) -> color_eyre::Result<Option<Map>> {
    let FetchedMapGroup {
        normalized_name,
        maps,
    } = fetched;

    let Some(interactive) = maps.into_iter().find(|map| map.projection == "interactive") else {
        return Ok(None);
    };

    let name = map_names.get(&normalized_name).cloned().ok_or_else(|| {
        color_eyre::eyre::eyre!("No human-readable name found for '{normalized_name}'")
    })?;

    let result = if let Some(svg_url) = interactive.svg_path.as_deref() {
        process_svg_map(client, &normalized_name, svg_url, force).await?
    } else if let Some(tile_template) = interactive.tile_path.as_deref() {
        let min_zoom = interactive
            .min_zoom
            .ok_or_else(|| color_eyre::eyre::eyre!("Missing minZoom for '{normalized_name}'"))?;
        let max_zoom = interactive
            .max_zoom
            .ok_or_else(|| color_eyre::eyre::eyre!("Missing maxZoom for '{normalized_name}'"))?;
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
    } else {
        return Err(color_eyre::eyre::eyre!(
            "Interactive map '{normalized_name}' has no svgPath or tilePath"
        ));
    };

    // Calculate logical size from bounds (in game units/meters)
    // bounds format: [[maxX, minY], [minX, maxY]]
    let logical_size = if let Some(bounds) = &interactive.bounds {
        let width = (bounds[0][0] - bounds[1][0]).abs() as f32;
        let height = (bounds[1][1] - bounds[0][1]).abs() as f32;
        [width, height]
    } else {
        // Fallback to image size if no bounds
        result.image_size
    };

    // Get spawns for this map
    let spawns = map_spawns.get(&normalized_name).cloned();

    Ok(Some(Map {
        normalized_name,
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
            .map(|layers| layers.into_iter().map(Layer::from).collect()),
        labels: interactive
            .labels
            .map(|labels| labels.into_iter().map(Label::from).collect()),
        spawns,
    }))
}

impl From<FetchedLayer> for Layer {
    fn from(fetched: FetchedLayer) -> Self {
        Self {
            name: fetched.name,
            svg_layer: fetched.svg_layer,
            tile_path: fetched.tile_path,
            show: fetched.show,
            extents: fetched.extents.into_iter().map(Extent::from).collect(),
        }
    }
}

impl From<FetchedExtent> for Extent {
    fn from(fetched: FetchedExtent) -> Self {
        Self {
            height: fetched.height,
            bounds: fetched
                .bounds
                .map(|bounds| bounds.into_iter().map(ExtentBound::from).collect()),
        }
    }
}

impl From<FetchedExtentBound> for ExtentBound {
    fn from(fetched: FetchedExtentBound) -> Self {
        Self {
            point1: fetched.point1,
            point2: fetched.point2,
            name: fetched.name,
        }
    }
}

impl From<FetchedLabel> for Label {
    fn from(fetched: FetchedLabel) -> Self {
        Self {
            position: fetched.position,
            text: fetched.text,
            rotation: fetched.rotation,
            size: fetched.size,
            top: fetched.top,
            bottom: fetched.bottom,
        }
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    env_logger::init();
    color_eyre::install()?;

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
    let total_spawns: usize = map_spawns.values().map(|v| v.len()).sum();
    println!("Fetched {} PMC spawns", total_spawns);

    println!("Fetching maps from tarkov-dev...");

    let response = client
        .get(MAPS_JSON_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(color_eyre::eyre::eyre!(
            "Failed to fetch maps: {}",
            response.status()
        ));
    }

    let json_text = response.text().await?;
    println!("Fetched {} bytes of JSON", json_text.len());

    let fetched_maps: Vec<FetchedMapGroup> = serde_json::from_str(&json_text)?;
    println!("Parsed {} map groups\n", fetched_maps.len());

    let multi_progress = MultiProgress::new();

    let maps_pb = multi_progress.add(ProgressBar::new(fetched_maps.len() as u64));
    maps_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} maps - {msg}")
            .unwrap()
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
        "\nProcessed {} interactive maps (skipped {})",
        maps.len(),
        skipped
    );

    let pretty_config = PrettyConfig::new()
        .depth_limit(10)
        .indentor("  ".to_string())
        .struct_names(true)
        .enumerate_arrays(false);

    let ron_string = ron::ser::to_string_pretty(&maps, pretty_config)?;
    println!("Serialized to {} bytes of RON", ron_string.len());

    fs::create_dir_all(repo_path(MAPS_DIR))?;

    let output_path = repo_path(MAPS_RON_PATH);
    fs::write(&output_path, &ron_string)?;
    println!("Wrote maps to {}", output_path.display());

    println!("\nMaps:");
    for map in &maps {
        println!("  - {} ({})", map.name, map.normalized_name);
    }

    Ok(())
}
