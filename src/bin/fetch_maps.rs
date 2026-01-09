use ron::ser::PrettyConfig;
use serde::Deserialize;
use std::fs;
use tarkov_map::{Extent, ExtentBound, Label, Layer, Map, MapGroup, TarkovMaps};

/// GitHub raw content URL for maps.json
const MAPS_JSON_URL: &str =
    "https://raw.githubusercontent.com/the-hideout/tarkov-dev/main/src/data/maps.json";

// ============================================================================
// Fetched types - match the JSON structure exactly
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedMapGroup {
    normalized_name: String,
    primary_path: String,
    maps: Vec<FetchedMap>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchedMap {
    key: String,
    #[serde(default)]
    alt_maps: Option<Vec<String>>,
    projection: String,
    #[serde(default)]
    specific: Option<String>,
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
// Conversion from Fetched types to lib types
// ============================================================================

impl From<FetchedMapGroup> for MapGroup {
    fn from(fetched: FetchedMapGroup) -> Self {
        Self {
            normalized_name: fetched.normalized_name,
            primary_path: fetched.primary_path,
            maps: fetched.maps.into_iter().map(Map::from).collect(),
        }
    }
}

impl From<FetchedMap> for Map {
    fn from(fetched: FetchedMap) -> Self {
        Self {
            key: fetched.key,
            alt_maps: fetched.alt_maps,
            projection: fetched.projection,
            specific: fetched.specific,
            author: fetched.author,
            author_link: fetched.author_link,
            tile_size: fetched.tile_size,
            min_zoom: fetched.min_zoom,
            max_zoom: fetched.max_zoom,
            transform: fetched.transform,
            coordinate_rotation: fetched.coordinate_rotation,
            bounds: fetched.bounds,
            svg_path: fetched.svg_path,
            svg_layer: fetched.svg_layer,
            tile_path: fetched.tile_path,
            height_range: fetched.height_range,
            layers: fetched
                .layers
                .map(|layers| layers.into_iter().map(Layer::from).collect()),
            labels: fetched
                .labels
                .map(|labels| labels.into_iter().map(Label::from).collect()),
        }
    }
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

    println!("Fetching maps from tarkov-dev...");

    // Fetch the maps JSON from GitHub
    let client = reqwest::Client::new();
    let response = client
        .get(MAPS_JSON_URL)
        .header(reqwest::header::USER_AGENT, "tarkov-map")
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

    // Convert to the library types
    let maps: TarkovMaps = fetched_maps.into_iter().map(MapGroup::from).collect();

    // Serialize to RON with pretty formatting
    let pretty_config = PrettyConfig::new()
        .depth_limit(10)
        .indentor("  ".to_string())
        .struct_names(true)
        .enumerate_arrays(false);

    let ron_string = ron::ser::to_string_pretty(&maps, pretty_config)?;
    println!("Serialized to {} bytes of RON", ron_string.len());

    // Ensure assets directory exists
    fs::create_dir_all("assets")?;

    // Write to file
    let output_path = "assets/maps.ron";
    fs::write(output_path, &ron_string)?;
    println!("Wrote maps to {}", output_path);

    // Print summary
    println!("\nMap groups:");
    for group in &maps {
        println!(
            "  - {} ({} variants)",
            group.normalized_name,
            group.maps.len()
        );
    }

    Ok(())
}
