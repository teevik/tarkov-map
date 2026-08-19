//! Data models for the Tarkov Map viewer.
//!
//! This crate defines the core types used to represent interactive maps from
//! the tarkov-dev project, including map metadata, labels, spawn points,
//! and extraction points.

pub mod bc7z;
mod catalog;

pub use catalog::{CatalogError, MapCatalog};

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

    /// Sniper Zones drawn as area outlines on this Map.
    #[serde(default)]
    pub sniper_zones: Vec<SniperZone>,

    /// Minefields drawn as area outlines on this Map.
    #[serde(default)]
    pub minefields: Vec<Minefield>,

    /// Boss Spawns grouped by game-space position and Mob.
    #[serde(default)]
    pub boss_spawns: Vec<BossSpawn>,

    /// Transits that continue a raid on another Map.
    #[serde(default)]
    pub transits: Vec<Transit>,

    /// Switches that a player can operate on this Map.
    #[serde(default)]
    pub switches: Vec<Switch>,

    /// BTR Stops where the armoured taxi picks up or drops off players.
    #[serde(default)]
    pub btr_stops: Vec<BtrStop>,
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

/// A Sniper Zone's outline in two-dimensional game coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SniperZone {
    pub outline: Vec<[f64; 2]>,
}

/// A Minefield's outline in two-dimensional game coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Minefield {
    pub outline: Vec<[f64; 2]>,
}

/// A Boss Spawn shared by one or more Mobs at one game-space position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BossSpawn {
    pub position: [f64; 2],
    pub mobs: Vec<BossChance>,
}

/// A Mob and its map-wide chance of appearing at a Boss Spawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BossChance {
    pub name: String,
    pub chance: f64,
}

/// A Transit to another Map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transit {
    pub position: [f64; 2],
    pub target: String,
}

/// A Switch that the player can operate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Switch {
    pub position: [f64; 2],
    pub name: String,
}

/// A BTR Stop where the armoured taxi halts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtrStop {
    pub position: [f64; 2],
    pub name: String,
}

/// Collection of all Tarkov maps.
pub type TarkovMaps = Vec<Map>;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn bundled_maps() -> TarkovMaps {
        ron::from_str(include_str!("../assets/maps.ron")).expect("bundled maps.ron should parse")
    }

    #[test]
    fn older_map_data_defaults_new_overlay_collections_to_empty() {
        let map: Map = ron::from_str(
            r#"Map(
                normalizedName: "factory",
                name: "Factory",
                imagePath: "maps/factory.bc7z",
                imageSize: (256.0, 256.0),
                logicalSize: (200.0, 200.0),
            )"#,
        )
        .expect("old map data should still parse");

        assert!(map.sniper_zones.is_empty());
        assert!(map.minefields.is_empty());
        assert!(map.boss_spawns.is_empty());
        assert!(map.transits.is_empty());
        assert!(map.switches.is_empty());
        assert!(map.btr_stops.is_empty());
    }

    #[test]
    fn bundled_overlay_data_matches_known_catalogue_spot_checks() {
        let maps = bundled_maps();
        let map = |name: &str| {
            maps.iter()
                .find(|map| map.normalized_name == name)
                .unwrap_or_else(|| panic!("missing bundled Map {name}"))
        };

        assert_eq!(map("woods").btr_stops.len(), 8);
        assert_eq!(map("streets-of-tarkov").btr_stops.len(), 6);
        assert_eq!(map("the-lab").switches.len(), 15);
        assert_eq!(map("customs").boss_spawns.len(), 29);
        assert_eq!(
            map("ground-zero")
                .boss_spawns
                .iter()
                .filter(|spawn| {
                    spawn
                        .mobs
                        .iter()
                        .any(|mob| mob.name == "Cultist Priest" && mob.chance == 0.02)
                })
                .count(),
            61
        );
        assert_eq!(map("lighthouse").sniper_zones.len(), 6);
        assert_eq!(map("lighthouse").minefields.len(), 338);
        for name in ["terminal", "the-labyrinth", "icebreaker"] {
            assert!(map(name).transits.is_empty());
        }
    }

    #[test]
    fn bundled_overlay_data_is_internally_consistent() {
        let maps = bundled_maps();
        let map_names: HashSet<_> = maps
            .iter()
            .map(|map| map.normalized_name.as_str())
            .collect();

        for map in &maps {
            assert!(
                map.transits
                    .iter()
                    .all(|transit| map_names.contains(transit.target.as_str()))
            );
            assert!(map.sniper_zones.iter().all(|zone| zone.outline.len() >= 3));
            assert!(
                map.minefields
                    .iter()
                    .all(|minefield| minefield.outline.len() >= 3)
            );

            for (index, spawn) in map.boss_spawns.iter().enumerate() {
                assert!(!spawn.mobs.is_empty());
                assert!(
                    !map.boss_spawns[..index]
                        .iter()
                        .any(|earlier| earlier.position == spawn.position)
                );
                assert!(spawn.mobs.windows(2).all(|pair| {
                    pair[0].chance > pair[1].chance
                        || (pair[0].chance == pair[1].chance && pair[0].name <= pair[1].name)
                }));
            }
        }
    }

    #[test]
    fn bundled_game_coordinates_have_at_most_two_decimal_places() {
        fn assert_coordinate(value: f64) {
            assert!(
                (value * 100.0 - (value * 100.0).round()).abs() < 1e-8,
                "game coordinate {value} has more than two decimal places"
            );
        }
        fn assert_position<const N: usize>(position: [f64; N]) {
            position.into_iter().for_each(assert_coordinate);
        }

        for map in bundled_maps() {
            map.bounds.into_iter().flatten().for_each(assert_position);
            map.labels.into_iter().flatten().for_each(|label| {
                assert_position(label.position);
                label.top.into_iter().for_each(assert_coordinate);
                label.bottom.into_iter().for_each(assert_coordinate);
            });
            map.spawns
                .into_iter()
                .flatten()
                .for_each(|spawn| assert_position(spawn.position));
            map.extracts.into_iter().flatten().for_each(|extract| {
                extract.position.into_iter().for_each(assert_position);
            });
            map.sniper_zones.into_iter().for_each(|zone| {
                zone.outline.into_iter().for_each(assert_position);
            });
            map.minefields.into_iter().for_each(|minefield| {
                minefield.outline.into_iter().for_each(assert_position);
            });
            map.boss_spawns
                .into_iter()
                .for_each(|spawn| assert_position(spawn.position));
            map.transits
                .into_iter()
                .for_each(|transit| assert_position(transit.position));
            map.switches
                .into_iter()
                .for_each(|map_switch| assert_position(map_switch.position));
            map.btr_stops
                .into_iter()
                .for_each(|stop| assert_position(stop.position));
        }
    }
}
