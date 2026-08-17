//! Data models for the Tarkov Map viewer.
//!
//! This crate defines the core types used to represent interactive maps from
//! the tarkov-dev project, including map metadata, labels, spawn points,
//! and extraction points.

pub mod bc7z;

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// An interactive map for a Tarkov location.
///
/// Derived from the upstream tarkov-dev `maps.json` (interactive variants only)
/// and enriched with human-readable names from the tarkov.dev GraphQL API.
#[skip_serializing_none]
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
    #[serde(default)]
    pub alt_maps: Option<Vec<String>>,

    /// Map author's name.
    #[serde(default)]
    pub author: Option<String>,

    /// URL to the author's page.
    #[serde(default)]
    pub author_link: Option<String>,

    /// Transform matrix `[scaleX, translateX, scaleY, translateY]`.
    ///
    /// Used for coordinate conversion in some maps (e.g., Labs, Labyrinth).
    #[serde(default)]
    pub transform: Option<[f64; 4]>,

    /// Coordinate rotation in degrees.
    ///
    /// Different maps use different rotations:
    /// - 180° (most maps)
    /// - 270° (Labs, Labyrinth)
    /// - 90° (Factory)
    #[serde(default)]
    pub coordinate_rotation: Option<f64>,

    /// Map bounds `[[maxX, minY], [minX, maxY]]` in game coordinates.
    #[serde(default)]
    pub bounds: Option<[[f64; 2]; 2]>,

    /// Map labels and annotations.
    #[serde(default)]
    pub labels: Option<Vec<Label>>,

    /// PMC spawn points.
    #[serde(default)]
    pub spawns: Option<Vec<Spawn>>,

    /// Extraction points.
    #[serde(default)]
    pub extracts: Option<Vec<Extract>>,
}

/// A text label/annotation on the map.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    /// Position `[x, y]` in game coordinates.
    pub position: [f64; 2],

    /// Label text content.
    pub text: String,

    /// Rotation angle in degrees.
    #[serde(default)]
    pub rotation: Option<f64>,

    /// Font size.
    #[serde(default)]
    pub size: Option<i32>,

    /// Upper height limit for visibility.
    #[serde(default)]
    pub top: Option<f64>,

    /// Lower height limit for visibility.
    #[serde(default)]
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
#[skip_serializing_none]
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
    #[serde(default)]
    pub position: Option<[f64; 3]>,
}

/// Collection of all Tarkov maps.
pub type TarkovMaps = Vec<Map>;
