use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::Map;

/// A collection of Maps whose domain invariants have been validated.
#[derive(Debug, Clone)]
pub struct MapCatalog {
    maps: Vec<Map>,
}

impl MapCatalog {
    /// Validates `maps` and constructs a catalogue when every invariant holds.
    pub fn try_new(maps: Vec<Map>) -> Result<Self, CatalogError> {
        let mut diagnostics = Vec::new();
        if maps.is_empty() {
            diagnostics.push(CatalogDiagnostic::new(
                "<catalogue>",
                "maps",
                "collection",
                "must contain at least one Map",
            ));
        }
        let mut first_index_by_name = HashMap::new();
        let map_names: HashSet<&str> = maps
            .iter()
            .map(|map| map.normalized_name.as_str())
            .collect();

        for (index, map) in maps.iter().enumerate() {
            if let Some(first_index) = first_index_by_name.insert(&map.normalized_name, index) {
                diagnostics.push(CatalogDiagnostic::new(
                    &map.normalized_name,
                    "maps",
                    index.to_string(),
                    format!("duplicate normalized_name; first used by entry {first_index}"),
                ));
            }

            if !map
                .projection
                .image_size
                .to_array()
                .iter()
                .all(|dimension| dimension.is_finite() && *dimension > 0.0)
            {
                diagnostics.push(CatalogDiagnostic::new(
                    &map.normalized_name,
                    "metadata",
                    "image_size",
                    "must contain finite, positive dimensions",
                ));
            }
            if !map
                .projection
                .game_to_image
                .to_array()
                .iter()
                .all(|n| n.is_finite())
                || map.projection.game_to_image.inverse().is_none()
                || !map.projection.metres_per_pixel().is_finite()
            {
                diagnostics.push(CatalogDiagnostic::new(
                    &map.normalized_name,
                    "metadata",
                    "projection",
                    "must contain a finite, invertible affine",
                ));
            }

            match map.bounds {
                None => diagnostics.push(CatalogDiagnostic::new(
                    &map.normalized_name,
                    "metadata",
                    "bounds",
                    "bounds are required",
                )),
                Some(bounds)
                    if !bounds
                        .iter()
                        .flatten()
                        .all(|coordinate| coordinate.is_finite()) =>
                {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "metadata",
                        "bounds",
                        "every coordinate must be finite",
                    ));
                }
                Some([[first_x, first_y], [second_x, second_y]])
                    if first_x == second_x || first_y == second_y =>
                {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "metadata",
                        "bounds",
                        "must describe a non-zero-area box",
                    ));
                }
                Some(_) => {}
            }

            for (entry_index, label) in map.labels.iter().flatten().enumerate() {
                if !coordinates_are_finite(label.position) {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "labels",
                        named_entry(entry_index, &label.text),
                        "position coordinates must be finite",
                    ));
                }
            }
            for (entry_index, spawn) in map.spawns.iter().flatten().enumerate() {
                if !coordinates_are_finite(spawn.position) {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "spawns",
                        entry_index.to_string(),
                        "position coordinates must be finite",
                    ));
                }
            }
            for (entry_index, extract) in map.extracts.iter().flatten().enumerate() {
                let entry = named_entry(entry_index, &extract.name);
                if !matches!(extract.faction.as_str(), "pmc" | "scav" | "shared") {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "extracts",
                        &entry,
                        format!(
                            "unsupported faction `{}`; expected `pmc`, `scav`, or `shared`",
                            extract.faction
                        ),
                    ));
                }
                if extract
                    .position
                    .is_some_and(|position| !coordinates_are_finite(position))
                {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "extracts",
                        entry,
                        "position coordinates must be finite",
                    ));
                }
            }

            for (entry_index, zone) in map.sniper_zones.iter().enumerate() {
                if zone.outline.len() < 3 {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "sniper_zones",
                        entry_index.to_string(),
                        "outline must contain at least 3 vertices",
                    ));
                }
            }
            for (entry_index, minefield) in map.minefields.iter().enumerate() {
                if minefield.outline.len() < 3 {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "minefields",
                        entry_index.to_string(),
                        "outline must contain at least 3 vertices",
                    ));
                }
            }
            for (entry_index, spawn) in map.boss_spawns.iter().enumerate() {
                if spawn.mobs.is_empty() {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "boss_spawns",
                        entry_index.to_string(),
                        "mobs must not be empty",
                    ));
                }
                for (mob_index, mob) in spawn.mobs.iter().enumerate() {
                    if !(0.0..=1.0).contains(&mob.chance) {
                        diagnostics.push(CatalogDiagnostic::new(
                            &map.normalized_name,
                            "boss_spawns",
                            format!("{entry_index}, mob {mob_index} (`{}`)", mob.name),
                            format!("chance {} must be within 0..=1", mob.chance),
                        ));
                    }
                }
            }
            for (entry_index, transit) in map.transits.iter().enumerate() {
                if !map_names.contains(transit.target.as_str()) {
                    diagnostics.push(CatalogDiagnostic::new(
                        &map.normalized_name,
                        "transits",
                        format!("{entry_index} (target `{}`)", transit.target),
                        "target does not name a bundled Map",
                    ));
                }
            }
        }

        if diagnostics.is_empty() {
            Ok(Self { maps })
        } else {
            Err(CatalogError { diagnostics })
        }
    }

    /// Checks the Map image references against the `.bc7z` files in a Bundle.
    pub fn validate_bundle_images<I, P>(&self, paths: I) -> Result<(), CatalogError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<str>,
    {
        let bundle_images: BTreeSet<String> = paths
            .into_iter()
            .map(|path| path.as_ref().to_owned())
            .filter(|path| path.starts_with("maps/") && path.ends_with(".bc7z"))
            .collect();
        let referenced_images: BTreeSet<String> = self
            .maps
            .iter()
            .filter(|map| map.image_path.ends_with(".bc7z"))
            .map(|map| map.image_path.clone())
            .collect();
        let mut diagnostics = Vec::new();

        for map in &self.maps {
            if !map.image_path.ends_with(".bc7z") {
                diagnostics.push(CatalogDiagnostic::new(
                    &map.normalized_name,
                    "images",
                    "image_path",
                    format!("`{}` must reference a `.bc7z` Bundle image", map.image_path),
                ));
            } else if !bundle_images.contains(&map.image_path) {
                diagnostics.push(CatalogDiagnostic::new(
                    &map.normalized_name,
                    "images",
                    "image_path",
                    format!(
                        "referenced Bundle image `{}` does not exist",
                        map.image_path
                    ),
                ));
            }
        }

        for path in bundle_images.difference(&referenced_images) {
            diagnostics.push(CatalogDiagnostic::new(
                "<bundle>",
                "images",
                format!("`{path}`"),
                "bundled `.bc7z` is not referenced by any Map",
            ));
        }

        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(CatalogError { diagnostics })
        }
    }

    /// Returns the validated Maps in bundle order, consuming the catalogue.
    pub fn into_maps(self) -> Vec<Map> {
        self.maps
    }
}

fn coordinates_are_finite<const N: usize>(position: [f64; N]) -> bool {
    position.into_iter().all(f64::is_finite)
}

fn named_entry(index: usize, name: &str) -> String {
    format!("{index} (`{name}`)")
}

/// One actionable Map catalogue validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogDiagnostic {
    map: String,
    collection: String,
    entry: String,
    invariant: String,
}

impl CatalogDiagnostic {
    fn new(
        map: impl Into<String>,
        collection: impl Into<String>,
        entry: impl Into<String>,
        invariant: impl Into<String>,
    ) -> Self {
        Self {
            map: map.into(),
            collection: collection.into(),
            entry: entry.into(),
            invariant: invariant.into(),
        }
    }
}

impl fmt::Display for CatalogDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Map `{}`, collection `{}`, entry {}: {}",
            self.map, self.collection, self.entry, self.invariant
        )
    }
}

/// Every catalogue validation failure discovered in one pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError {
    diagnostics: Vec<CatalogDiagnostic>,
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "invalid Map catalogue:")?;
        for diagnostic in &self.diagnostics {
            writeln!(formatter, "- {diagnostic}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CatalogError {}

#[cfg(test)]
mod tests {
    use crate::{BossChance, BossSpawn, Extract, Label, Minefield, SniperZone, Spawn, Transit};

    use super::*;

    fn valid_map(normalized_name: &str) -> Map {
        Map {
            normalized_name: normalized_name.to_owned(),
            name: normalized_name.to_owned(),
            image_path: format!("maps/{normalized_name}.bc7z"),
            projection: crate::Projection {
                game_to_image: euclid::Transform2D::new(2.56, 0.0, 0.0, -2.56, 128.0, 128.0),
                image_size: euclid::Size2D::new(256.0, 256.0),
            },
            alt_maps: None,
            author: None,
            author_link: None,
            bounds: Some([[50.0, -50.0], [-50.0, 50.0]]),
            labels: None,
            spawns: None,
            extracts: None,
            sniper_zones: Vec::new(),
            minefields: Vec::new(),
            boss_spawns: Vec::new(),
            transits: Vec::new(),
            switches: Vec::new(),
            btr_stops: Vec::new(),
        }
    }

    #[test]
    fn catalog_rejects_duplicate_map_normalized_names() {
        let error = MapCatalog::try_new(vec![valid_map("customs"), valid_map("customs")])
            .expect_err("duplicate Map normalized names must be rejected");

        let diagnostic = error.to_string();
        assert!(diagnostic.contains("Map `customs`"), "{diagnostic}");
        assert!(diagnostic.contains("collection `maps`"), "{diagnostic}");
        assert!(diagnostic.contains("entry 1"), "{diagnostic}");
        assert!(
            diagnostic.contains("duplicate normalized_name"),
            "{diagnostic}"
        );
    }

    #[test]
    fn catalog_rejects_an_empty_map_collection() {
        let error = MapCatalog::try_new(Vec::new())
            .expect_err("an empty Map collection must be rejected")
            .to_string();

        assert!(
            error.contains(
                "Map `<catalogue>`, collection `maps`, entry collection: must contain at least one Map"
            ),
            "{error}"
        );
    }

    #[test]
    fn catalog_rejects_invalid_map_sizes_and_bounds() {
        let mut missing_bounds = valid_map("missing-bounds");
        missing_bounds.projection.image_size = euclid::Size2D::new(0.0, f64::NAN);
        missing_bounds.projection.game_to_image.m11 = 0.0;
        missing_bounds.bounds = None;

        let mut non_finite_bounds = valid_map("non-finite-bounds");
        non_finite_bounds.bounds = Some([[f64::NAN, -50.0], [-50.0, f64::INFINITY]]);

        let mut degenerate_bounds = valid_map("degenerate-bounds");
        degenerate_bounds.bounds = Some([[50.0, -50.0], [50.0, 50.0]]);

        let error = MapCatalog::try_new(vec![missing_bounds, non_finite_bounds, degenerate_bounds])
            .expect_err("invalid Map metadata must be rejected")
            .to_string();

        for expected in [
            "Map `missing-bounds`, collection `metadata`, entry image_size: must contain finite, positive dimensions",
            "Map `missing-bounds`, collection `metadata`, entry projection: must contain a finite, invertible affine",
            "Map `missing-bounds`, collection `metadata`, entry bounds: bounds are required",
            "Map `non-finite-bounds`, collection `metadata`, entry bounds: every coordinate must be finite",
            "Map `degenerate-bounds`, collection `metadata`, entry bounds: must describe a non-zero-area box",
        ] {
            assert!(
                error.contains(expected),
                "missing `{expected}` in:\n{error}"
            );
        }
    }

    #[test]
    fn catalog_rejects_invalid_base_collection_entries() {
        let mut map = valid_map("customs");
        map.labels = Some(vec![Label {
            position: [f64::NAN, 2.0],
            text: "Dorms".to_owned(),
            rotation: None,
            size: None,
            top: None,
            bottom: None,
        }]);
        map.spawns = Some(vec![Spawn {
            position: [1.0, f64::INFINITY, 3.0],
            sides: vec!["pmc".to_owned()],
            categories: vec!["player".to_owned()],
        }]);
        map.extracts = Some(vec![Extract {
            name: "Crossroads".to_owned(),
            faction: "cultist".to_owned(),
            position: Some([1.0, 2.0, f64::NEG_INFINITY]),
        }]);

        let error = MapCatalog::try_new(vec![map])
            .expect_err("invalid collection entries must be rejected")
            .to_string();

        for expected in [
            "Map `customs`, collection `labels`, entry 0 (`Dorms`): position coordinates must be finite",
            "Map `customs`, collection `spawns`, entry 0: position coordinates must be finite",
            "Map `customs`, collection `extracts`, entry 0 (`Crossroads`): unsupported faction `cultist`; expected `pmc`, `scav`, or `shared`",
            "Map `customs`, collection `extracts`, entry 0 (`Crossroads`): position coordinates must be finite",
        ] {
            assert!(
                error.contains(expected),
                "missing `{expected}` in:\n{error}"
            );
        }
    }

    #[test]
    fn catalog_rejects_invalid_overlay_entries_and_transit_targets() {
        let mut map = valid_map("factory");
        map.sniper_zones = vec![SniperZone {
            outline: vec![[0.0, 0.0], [1.0, 1.0]],
        }];
        map.minefields = vec![Minefield {
            outline: vec![[0.0, 0.0]],
        }];
        map.boss_spawns = vec![
            BossSpawn {
                position: [1.0, 2.0],
                mobs: Vec::new(),
            },
            BossSpawn {
                position: [3.0, 4.0],
                mobs: vec![
                    BossChance {
                        name: "Tagilla".to_owned(),
                        chance: -0.1,
                    },
                    BossChance {
                        name: "Killa".to_owned(),
                        chance: 1.1,
                    },
                    BossChance {
                        name: "Cultist Priest".to_owned(),
                        chance: f64::NAN,
                    },
                ],
            },
        ];
        map.transits = vec![Transit {
            position: [5.0, 6.0],
            target: "missing-map".to_owned(),
        }];

        let error = MapCatalog::try_new(vec![map])
            .expect_err("invalid Overlay entries must be rejected")
            .to_string();

        for expected in [
            "Map `factory`, collection `sniper_zones`, entry 0: outline must contain at least 3 vertices",
            "Map `factory`, collection `minefields`, entry 0: outline must contain at least 3 vertices",
            "Map `factory`, collection `boss_spawns`, entry 0: mobs must not be empty",
            "Map `factory`, collection `boss_spawns`, entry 1, mob 0 (`Tagilla`): chance -0.1 must be within 0..=1",
            "Map `factory`, collection `boss_spawns`, entry 1, mob 1 (`Killa`): chance 1.1 must be within 0..=1",
            "Map `factory`, collection `boss_spawns`, entry 1, mob 2 (`Cultist Priest`): chance NaN must be within 0..=1",
            "Map `factory`, collection `transits`, entry 0 (target `missing-map`): target does not name a bundled Map",
        ] {
            assert!(
                error.contains(expected),
                "missing `{expected}` in:\n{error}"
            );
        }
    }

    #[test]
    fn catalog_rejects_missing_non_bc7z_and_orphaned_bundle_images() {
        let mut wrong_extension = valid_map("customs");
        wrong_extension.image_path = "maps/customs.png".to_owned();
        let catalog = MapCatalog::try_new(vec![wrong_extension, valid_map("woods")])
            .expect("Map metadata should be valid");

        let error = catalog
            .validate_bundle_images(["maps/customs.png", "maps/orphan.bc7z"])
            .expect_err("invalid image references must be rejected")
            .to_string();

        for expected in [
            "Map `customs`, collection `images`, entry image_path: `maps/customs.png` must reference a `.bc7z` Bundle image",
            "Map `woods`, collection `images`, entry image_path: referenced Bundle image `maps/woods.bc7z` does not exist",
            "Map `<bundle>`, collection `images`, entry `maps/orphan.bc7z`: bundled `.bc7z` is not referenced by any Map",
        ] {
            assert!(
                error.contains(expected),
                "missing `{expected}` in:\n{error}"
            );
        }
    }
}
