use ron::ser::PrettyConfig;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tarkov_map::{Extent, ExtentBound, Label, Layer, Map, MapSource, TarkovMaps};
use tokio::fs as tokio_fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// GitHub raw content URL for maps.json
const MAPS_JSON_URL: &str =
    "https://raw.githubusercontent.com/the-hideout/tarkov-dev/main/src/data/maps.json";

/// tarkov.dev GraphQL API endpoint
const TARKOV_DEV_GRAPHQL_URL: &str = "https://api.tarkov.dev/graphql";

const USER_AGENT: &str = "tarkov-map";

const MAPS_RON_PATH: &str = "assets/maps.ron";

// Map assets are stored locally under `assets/maps/`.
const MAP_ASSETS_DIR: &str = "assets/maps";
const SVG_DIR: &str = "assets/maps/svg";
const TILES_DIR: &str = "assets/maps/tiles";

const TILE_DOWNLOAD_CONCURRENCY: usize = 32;

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
    /// Bounds are arrays like [[x1, y1], [x2, y2], "name"]
    #[serde(default)]
    bounds: Option<Vec<FetchedExtentBound>>,
}

/// Custom deserializer for extent bounds which are heterogeneous arrays
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
    /// Rotation can be a number or a string in the JSON
    #[serde(default, deserialize_with = "deserialize_rotation")]
    rotation: Option<f64>,
    #[serde(default)]
    size: Option<i32>,
    #[serde(default)]
    top: Option<f64>,
    #[serde(default)]
    bottom: Option<f64>,
}

/// Deserialize rotation which can be either a number or a string
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

// ============================================================================
// Asset download
// ============================================================================

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

async fn download_url_to_path(
    client: &reqwest::Client,
    url: &str,
    output_path: &Path,
) -> color_eyre::Result<()> {
    if tokio_fs::try_exists(output_path).await.unwrap_or(false) {
        return Ok(());
    }

    if let Some(parent) = output_path.parent() {
        tokio_fs::create_dir_all(parent).await?;
    }

    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(color_eyre::eyre::eyre!(
            "Failed to fetch {}: {}",
            url,
            response.status()
        ));
    }

    let bytes = response.bytes().await?;
    tokio_fs::write(output_path, &bytes).await?;

    Ok(())
}

async fn ensure_svg_asset(
    client: &reqwest::Client,
    normalized_name: &str,
    svg_url: &str,
) -> color_eyre::Result<String> {
    let relative_path = format!("{}/{}.svg", SVG_DIR, normalized_name);
    let output_path = repo_path(&relative_path);

    download_url_to_path(client, svg_url, &output_path).await?;

    Ok(relative_path)
}

fn apply_tile_template(template: &str, z: i32, x: u32, y: u32) -> String {
    template
        .replace("{z}", &z.to_string())
        .replace("{x}", &x.to_string())
        .replace("{y}", &y.to_string())
}

async fn ensure_tile_assets(
    client: &reqwest::Client,
    normalized_name: &str,
    remote_template: &str,
    min_zoom: i32,
    max_zoom: i32,
) -> color_eyre::Result<String> {
    let relative_template = format!("{}/{}/{{z}}/{{x}}/{{y}}.png", TILES_DIR, normalized_name);

    let output_root = repo_path(TILES_DIR).join(normalized_name);

    let semaphore = Arc::new(Semaphore::new(TILE_DOWNLOAD_CONCURRENCY));
    let mut join_set: JoinSet<color_eyre::Result<()>> = JoinSet::new();

    for z in min_zoom..=max_zoom {
        let tiles_per_axis = 1u32
            .checked_shl(z.try_into()?)
            .ok_or_else(|| color_eyre::eyre::eyre!("Invalid zoom level: {}", z))?;

        for x in 0..tiles_per_axis {
            for y in 0..tiles_per_axis {
                let remote_url = apply_tile_template(remote_template, z, x, y);
                let output_path = output_root
                    .join(z.to_string())
                    .join(x.to_string())
                    .join(format!("{}.png", y));

                if output_path.exists() {
                    continue;
                }

                let client = client.clone();
                let semaphore = semaphore.clone();

                join_set.spawn(async move {
                    let _permit = semaphore
                        .acquire_owned()
                        .await
                        .map_err(|err| color_eyre::eyre::eyre!(err))?;

                    download_url_to_path(&client, &remote_url, &output_path).await
                });
            }
        }
    }

    while let Some(result) = join_set.join_next().await {
        result??;
    }

    Ok(relative_template)
}

// ============================================================================
// Conversion from Fetched types to lib types
// ============================================================================

async fn convert_group(
    client: &reqwest::Client,
    fetched: FetchedMapGroup,
    map_names: &HashMap<String, String>,
) -> color_eyre::Result<Option<Map>> {
    let FetchedMapGroup {
        normalized_name,
        maps,
    } = fetched;

    let Some(interactive) = maps.into_iter().find(|map| map.projection == "interactive") else {
        eprintln!("Skipping group '{normalized_name}': no interactive map");
        return Ok(None);
    };

    let name = map_names.get(&normalized_name).cloned().ok_or_else(|| {
        color_eyre::eyre::eyre!("No human-readable name found for '{normalized_name}'")
    })?;

    let source = if let Some(svg_url) = interactive.svg_path.as_deref() {
        let local_svg = ensure_svg_asset(client, &normalized_name, svg_url).await?;
        MapSource::Svg {
            path: local_svg,
            svg_layer: interactive.svg_layer,
        }
    } else if let Some(tile_template) = interactive.tile_path.as_deref() {
        let min_zoom = interactive
            .min_zoom
            .ok_or_else(|| color_eyre::eyre::eyre!("Missing minZoom for '{normalized_name}'"))?;
        let max_zoom = interactive
            .max_zoom
            .ok_or_else(|| color_eyre::eyre::eyre!("Missing maxZoom for '{normalized_name}'"))?;
        let tile_size = interactive.tile_size.unwrap_or(256);

        let local_template =
            ensure_tile_assets(client, &normalized_name, tile_template, min_zoom, max_zoom).await?;

        MapSource::Tiles {
            template: local_template,
            tile_size,
            min_zoom,
            max_zoom,
        }
    } else {
        return Err(color_eyre::eyre::eyre!(
            "Interactive map '{normalized_name}' has no svgPath or tilePath"
        ));
    };

    Ok(Some(Map {
        normalized_name,
        name,
        alt_maps: interactive.alt_maps,
        author: interactive.author,
        author_link: interactive.author_link,
        transform: interactive.transform,
        coordinate_rotation: interactive.coordinate_rotation,
        bounds: interactive.bounds,
        source,
        height_range: interactive.height_range,
        layers: interactive
            .layers
            .map(|layers| layers.into_iter().map(Layer::from).collect()),
        labels: interactive
            .labels
            .map(|labels| labels.into_iter().map(Label::from).collect()),
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

    let client = reqwest::Client::new();

    println!("Fetching map names from tarkov.dev...");
    let map_names = fetch_map_names(&client).await?;
    println!("Fetched {} map names", map_names.len());

    println!("Fetching maps from tarkov-dev...");

    // Fetch the maps JSON from GitHub
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

    // Parse the JSON into our fetched types
    let fetched_maps: Vec<FetchedMapGroup> = serde_json::from_str(&json_text)?;
    println!("Parsed {} map groups", fetched_maps.len());

    // Convert to the library types (one interactive map per group)
    let mut skipped = 0usize;
    let mut maps: TarkovMaps = Vec::new();

    for group in fetched_maps {
        match convert_group(&client, group, &map_names).await? {
            Some(map) => maps.push(map),
            None => skipped += 1,
        }
    }

    println!(
        "Selected {} interactive maps (skipped {})",
        maps.len(),
        skipped
    );

    // Serialize to RON with pretty formatting
    let pretty_config = PrettyConfig::new()
        .depth_limit(10)
        .indentor("  ".to_string())
        .struct_names(true)
        .enumerate_arrays(false);

    let ron_string = ron::ser::to_string_pretty(&maps, pretty_config)?;
    println!("Serialized to {} bytes of RON", ron_string.len());

    // Ensure assets directory exists
    fs::create_dir_all(repo_path(MAP_ASSETS_DIR))?;

    // Write to file
    let output_path = repo_path(MAPS_RON_PATH);
    fs::write(&output_path, &ron_string)?;
    println!("Wrote maps to {}", output_path.display());

    // Print summary
    println!("\nInteractive maps:");
    for map in &maps {
        println!("  - {} ({})", map.name, map.normalized_name);
    }

    Ok(())
}
