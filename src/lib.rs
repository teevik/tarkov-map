use serde::{Deserialize, Serialize};

/// A single interactive map for a Tarkov location.
///
/// Derived from the upstream `tarkov-dev` `maps.json` (interactive variants only) and enriched with
/// the human-readable map name from the tarkov.dev GraphQL API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Map {
    /// Normalized map name / slug (e.g., "customs", "streets-of-tarkov").
    pub normalized_name: String,
    /// Human-readable map name (e.g., "Customs").
    pub name: String,
    /// Alternative map keys that use this same map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_maps: Option<Vec<String>>,
    /// Map author name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Link to author's page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_link: Option<String>,
    /// Transform matrix [scaleX, translateX, scaleY, translateY].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[f64; 4]>,
    /// Coordinate rotation in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinate_rotation: Option<f64>,
    /// Map bounds [[maxX, minY], [minX, maxY]].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[[f64; 2]; 2]>,
    /// Map image source (either a local SVG or a local tile set).
    pub source: MapSource,
    /// Height range for the default layer [min, max].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_range: Option<[f64; 2]>,
    /// Map layers (floors, underground, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<Layer>>,
    /// Map labels/annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<Label>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MapSource {
    #[serde(rename = "Svg")]
    Svg {
        /// Local path to the SVG file.
        path: String,
        /// Default SVG layer to display.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        svg_layer: Option<String>,
    },
    #[serde(rename = "Tiles")]
    Tiles {
        /// Local tile path template (e.g. `assets/maps/tiles/customs/{z}/{x}/{y}.png`).
        template: String,
        tile_size: i32,
        min_zoom: i32,
        max_zoom: i32,
    },
}

/// A map layer (floor level).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// Display name for the layer.
    pub name: String,
    /// SVG layer identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svg_layer: Option<String>,
    /// Tile path for this layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_path: Option<String>,
    /// Whether this layer is shown by default.
    #[serde(default)]
    pub show: bool,
    /// Height/bounds extents that trigger this layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extents: Vec<Extent>,
}

/// Defines when a layer should be displayed based on height and optional bounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extent {
    /// Height range [min, max] for this extent.
    pub height: [f64; 2],
    /// Optional bounds within this extent (areas that trigger the layer).
    /// Each bound is [[x1, y1], [x2, y2], "name"].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Vec<ExtentBound>>,
}

/// A bound area within an extent.
/// Represented as [[x1, y1], [x2, y2], "name"] in JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtentBound {
    /// First corner point [x, y].
    pub point1: [f64; 2],
    /// Second corner point [x, y].
    pub point2: [f64; 2],
    /// Name/identifier for this bound area.
    pub name: String,
}

/// A label/annotation on the map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    /// Position [x, y] on the map.
    pub position: [f64; 2],
    /// Label text.
    pub text: String,
    /// Rotation in degrees (can be number or string in source JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    /// Font size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,
    /// Top height limit for visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    /// Bottom height limit for visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
}

/// Root type for the maps data file.
pub type TarkovMaps = Vec<Map>;
