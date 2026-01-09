//! Data models for the Tarkov Map viewer.
//!
//! This crate defines the core types used to represent interactive maps from
//! the tarkov-dev project, including map metadata, layers, labels, spawn points,
//! and extraction points.

use serde::{Deserialize, Serialize};

/// An interactive map for a Tarkov location.
///
/// Derived from the upstream tarkov-dev `maps.json` (interactive variants only)
/// and enriched with human-readable names from the tarkov.dev GraphQL API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Map {
    /// Normalized map name/slug (e.g., "customs", "streets-of-tarkov").
    pub normalized_name: String,

    /// Human-readable display name (e.g., "Customs").
    pub name: String,

    /// Path to the pre-rendered high-resolution PNG image.
    pub image_path: String,

    /// Original image dimensions `[width, height]` in pixels.
    pub image_size: [f32; 2],

    /// Logical dimensions `[width, height]` in game units (meters).
    ///
    /// Used for consistent zoom scaling across maps.
    pub logical_size: [f32; 2],

    /// Alternative map keys that share this map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_maps: Option<Vec<String>>,

    /// Map author's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    /// URL to the author's page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_link: Option<String>,

    /// Transform matrix `[scaleX, translateX, scaleY, translateY]`.
    ///
    /// Used for coordinate conversion in some maps (e.g., Labs, Labyrinth).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[f64; 4]>,

    /// Coordinate rotation in degrees.
    ///
    /// Different maps use different rotations:
    /// - 180° (most maps)
    /// - 270° (Labs, Labyrinth)
    /// - 90° (Factory)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinate_rotation: Option<f64>,

    /// Map bounds `[[maxX, minY], [minX, maxY]]` in game coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[[f64; 2]; 2]>,

    /// Default height range `[min, max]` for layer visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_range: Option<[f64; 2]>,

    /// Map layers (floors, underground areas, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<Layer>>,

    /// Map labels and annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<Label>>,

    /// PMC spawn points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawns: Option<Vec<Spawn>>,

    /// Extraction points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracts: Option<Vec<Extract>>,
}

/// A map layer representing a floor level or area.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// Display name for the layer.
    pub name: String,

    /// SVG layer identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svg_layer: Option<String>,

    /// Tile path template for this layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_path: Option<String>,

    /// Whether this layer is visible by default.
    #[serde(default)]
    pub show: bool,

    /// Height/bounds extents that trigger this layer's visibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extents: Vec<Extent>,
}

/// Defines visibility conditions for a layer based on height and bounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extent {
    /// Height range `[min, max]` for this extent.
    pub height: [f64; 2],

    /// Optional bounds within this extent that trigger layer visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Vec<ExtentBound>>,
}

/// A rectangular bound area within an extent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtentBound {
    /// First corner point `[x, y]`.
    pub point1: [f64; 2],

    /// Second corner point `[x, y]`.
    pub point2: [f64; 2],

    /// Name/identifier for this bound area.
    pub name: String,
}

/// A text label/annotation on the map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    /// Position `[x, y]` in game coordinates.
    pub position: [f64; 2],

    /// Label text content.
    pub text: String,

    /// Rotation angle in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,

    /// Font size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,

    /// Upper height limit for visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,

    /// Lower height limit for visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
}

/// A spawn point on the map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spawn {
    /// Position `[x, y, z]` in game coordinates.
    pub position: [f64; 3],

    /// Spawn sides (e.g., "pmc", "scav", "all").
    pub sides: Vec<String>,

    /// Spawn categories (e.g., "player", "bot").
    pub categories: Vec<String>,
}

/// An extraction point on the map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extract {
    /// Extract name (e.g., "ZB-1011", "Crossroads").
    pub name: String,

    /// Faction that can use this extract.
    ///
    /// Values: "pmc", "scav", or "shared".
    pub faction: String,

    /// Position `[x, y, z]` in game coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 3]>,
}

/// Collection of all Tarkov maps.
pub type TarkovMaps = Vec<Map>;
