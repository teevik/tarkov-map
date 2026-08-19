//! Map, its overlay collections, and the ordered, validated MapCatalog (ADR-0003).

use std::fmt;

use euclid::Angle;
use serde::{Deserialize, Serialize};

use super::spaces::{GameBox, GamePoint, ImagePoint, ImageSize, ImageVector, Projection};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MapId(pub String);

impl MapId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for MapId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque asset key resolved by the `ImageDecoder` port (e.g. `maps/customs.bc7z`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MapImageKey(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attribution {
    pub name: String,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Faction {
    Pmc,
    Scav,
    Shared,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub position: GamePoint,
    pub text: String,
    pub rotation: Angle<f64>,
    pub size: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    pub position: GamePoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extract {
    pub name: String,
    pub faction: Faction,
    pub position: GamePoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Map {
    pub id: MapId,
    pub name: String,
    pub image: MapImageKey,
    pub image_size: ImageSize,
    /// The Projection, pre-baked by fetch_maps for tiles and SVG alike.
    pub game_to_image: Projection,
    /// Playable extent — Map Suggestion's inside-test.
    pub bounds: GameBox,
    pub attribution: Option<Attribution>,
    pub labels: Vec<Label>,
    pub spawns: Vec<Spawn>,
    pub extracts: Vec<Extract>,
}

impl Map {
    pub fn project(&self, p: GamePoint) -> ImagePoint {
        self.game_to_image.transform_point(p)
    }

    pub fn contains(&self, p: GamePoint) -> bool {
        self.bounds.contains(p)
    }

    /// Heading as an image-space direction: push (sin yaw, cos yaw) through the
    /// affine's linear part — no rotation field needed.
    pub fn heading_on_image(&self, yaw: Angle<f64>) -> ImageVector {
        let (s, c) = yaw.sin_cos();
        self.game_to_image
            .transform_vector(euclid::vec2(s, c))
            .normalize()
    }

    pub fn metres_per_pixel(&self) -> f64 {
        let m = &self.game_to_image;
        1.0 / (m.m11 * m.m22 - m.m12 * m.m21).abs().sqrt()
    }
}

/// Ordered (sidebar / Ctrl+1..9 order), validated list of Maps.
#[derive(Debug, Clone, PartialEq)]
pub struct MapCatalog {
    maps: Vec<Map>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CatalogError {
    Empty,
    DuplicateId(MapId),
    NonPositiveImageSize(MapId),
    SingularProjection(MapId),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the map catalogue is empty"),
            Self::DuplicateId(id) => write!(f, "duplicate map id {id}"),
            Self::NonPositiveImageSize(id) => write!(f, "map {id} has a non-positive image size"),
            Self::SingularProjection(id) => write!(f, "map {id} has a non-invertible projection"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl MapCatalog {
    /// The one validator, run by fetch_maps pre-write and adapters post-load (ADR-0006).
    pub fn try_new(maps: Vec<Map>) -> Result<Self, CatalogError> {
        if maps.is_empty() {
            return Err(CatalogError::Empty);
        }
        let mut seen = std::collections::BTreeSet::new();
        for m in &maps {
            if !seen.insert(&m.id) {
                return Err(CatalogError::DuplicateId(m.id.clone()));
            }
            if m.image_size.width <= 0.0 || m.image_size.height <= 0.0 {
                return Err(CatalogError::NonPositiveImageSize(m.id.clone()));
            }
            if m.game_to_image.inverse().is_none() {
                return Err(CatalogError::SingularProjection(m.id.clone()));
            }
        }
        Ok(Self { maps })
    }

    pub fn get(&self, id: &MapId) -> Option<&Map> {
        self.maps.iter().find(|m| &m.id == id)
    }

    pub fn first(&self) -> &Map {
        &self.maps[0] // non-empty by construction
    }

    pub fn by_index(&self, index: usize) -> Option<&Map> {
        self.maps.get(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Map> {
        self.maps.iter()
    }

    pub fn len(&self) -> usize {
        self.maps.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// Maps other than `except` whose Bounds contain `p` — Map Suggestion's candidate set.
    pub fn containing(&self, p: GamePoint, except: &MapId) -> impl Iterator<Item = &Map> {
        self.maps
            .iter()
            .filter(move |m| &m.id != except && m.contains(p))
    }
}
