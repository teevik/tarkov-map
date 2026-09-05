//! Transactional staging, provenance, and validation for a Bundle Refresh.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};
use tarkov_map::{MapCatalog, TarkovMaps};
use thiserror::Error;

pub const PROVENANCE_FILE: &str = "maps.provenance.ron";

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceSource {
    pub role: String,
    pub identifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationOptions {
    pub tile_zoom_offset: i32,
    pub svg_render_scale: f32,
    pub force: bool,
    pub convert_only: bool,
    pub image_format: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshProvenance {
    pub format_version: u32,
    pub sources: Vec<ProvenanceSource>,
    pub options: GenerationOptions,
}

impl RefreshProvenance {
    pub fn new(mut sources: Vec<ProvenanceSource>, options: GenerationOptions) -> Self {
        sources.sort();
        sources.dedup();
        Self {
            format_version: 1,
            sources,
            options,
        }
    }
}

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("failed to read staged maps.ron: {0}")]
    ReadMetadata(#[source] std::io::Error),
    #[error("failed to parse staged maps.ron: {0}")]
    ParseMetadata(#[source] ron::de::SpannedError),
    #[error("staged Bundle validation failed: {0}")]
    InvalidBundle(#[from] tarkov_map::CatalogError),
    #[error("failed to read staged Bundle images: {0}")]
    ReadImages(#[source] std::io::Error),
    #[error("failed to read staged provenance: {0}")]
    ReadProvenance(#[source] std::io::Error),
    #[error("failed to parse staged provenance: {0}")]
    ParseProvenance(#[source] ron::de::SpannedError),
    #[error("invalid staged provenance: {0}")]
    InvalidProvenance(String),
    #[error("failed to serialize staged provenance: {0}")]
    SerializeProvenance(#[source] ron::Error),
    #[error("failed to write staged provenance: {0}")]
    WriteProvenance(#[source] std::io::Error),
    #[error("failed to create Refresh stage: {0}")]
    CreateStage(#[source] std::io::Error),
    #[error("failed to install staged Bundle: {0}")]
    Install(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ValidationSummary {
    pub map_count: usize,
    pub image_count: usize,
    pub source_count: usize,
}

pub struct StagedBundle {
    usable_assets: PathBuf,
    staged_assets: PathBuf,
}

impl StagedBundle {
    pub fn new(usable_assets: &Path) -> Result<Self, RefreshError> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| RefreshError::CreateStage(std::io::Error::other(error.to_string())))?
            .as_nanos();
        let staged_assets = usable_assets.with_extension(format!("refresh-stage-{nonce}"));
        fs::create_dir_all(staged_assets.join("maps")).map_err(RefreshError::CreateStage)?;
        if let Err(error) = copy_non_bundle_assets(usable_assets, &staged_assets) {
            let _ = fs::remove_dir_all(&staged_assets);
            return Err(RefreshError::CreateStage(error));
        }
        Ok(Self {
            usable_assets: usable_assets.to_owned(),
            staged_assets,
        })
    }

    pub fn path(&self) -> &Path {
        &self.staged_assets
    }

    pub fn install(self) -> Result<ValidationSummary, RefreshError> {
        install_staged_bundle(&self.usable_assets, &self.staged_assets)
    }
}

impl Drop for StagedBundle {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staged_assets);
    }
}

pub fn write_staged_provenance(
    staged_assets: &Path,
    provenance: &RefreshProvenance,
) -> Result<(), RefreshError> {
    let serialized = ron::ser::to_string_pretty(
        provenance,
        PrettyConfig::new()
            .indentor("  ".to_owned())
            .struct_names(true),
    )
    .map_err(RefreshError::SerializeProvenance)?;
    fs::write(staged_assets.join(PROVENANCE_FILE), serialized)
        .map_err(RefreshError::WriteProvenance)
}

pub fn review_summary(provenance: &RefreshProvenance, validation: &ValidationSummary) -> String {
    let cache = if provenance.options.force {
        "cache bypassed"
    } else {
        "cache allowed"
    };
    let mode = if provenance.options.convert_only {
        ", convert only"
    } else {
        ""
    };
    let source_label = if validation.source_count == 1 {
        "source"
    } else {
        "sources"
    };
    let source_lines = provenance
        .sources
        .iter()
        .map(|source| match &source.map {
            Some(map) => format!("  {map} [{}]: {}", source.role, source.identifier),
            None => format!("  {}: {}", source.role, source.identifier),
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Provenance ({} {source_label}):\n{source_lines}\nGeneration: tile zoom offset {}, SVG scale {}, {}, {cache}{mode}\nValidation: {} Maps, {} images; Bundle replaced",
        validation.source_count,
        provenance.options.tile_zoom_offset,
        provenance.options.svg_render_scale,
        provenance.options.image_format,
        validation.map_count,
        validation.image_count,
    )
}

fn copy_non_bundle_assets(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    if !source.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(name.to_str(), Some("maps" | "maps.ron" | PROVENANCE_FILE)) {
            continue;
        }
        copy_entry(&entry.path(), &destination.join(name))?;
    }
    Ok(())
}

fn copy_entry(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    if source.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        fs::copy(source, destination)?;
    }
    Ok(())
}

/// Validates a complete staged Bundle before it can replace the usable one.
pub fn install_staged_bundle(
    usable_assets: &Path,
    staged_assets: &Path,
) -> Result<ValidationSummary, RefreshError> {
    let provenance_text = fs::read_to_string(staged_assets.join(PROVENANCE_FILE))
        .map_err(RefreshError::ReadProvenance)?;
    let provenance: RefreshProvenance =
        ron::from_str(&provenance_text).map_err(RefreshError::ParseProvenance)?;
    validate_provenance(&provenance)?;
    let metadata =
        fs::read_to_string(staged_assets.join("maps.ron")).map_err(RefreshError::ReadMetadata)?;
    let maps: TarkovMaps = ron::from_str(&metadata).map_err(RefreshError::ParseMetadata)?;
    let catalog = MapCatalog::try_new(maps)?;
    let image_paths = fs::read_dir(staged_assets.join("maps"))
        .map_err(RefreshError::ReadImages)?
        .map(|entry| {
            entry
                .map(|entry| format!("maps/{}", entry.file_name().to_string_lossy()))
                .map_err(RefreshError::ReadImages)
        })
        .collect::<Result<Vec<_>, _>>()?;
    catalog.validate_bundle_images(&image_paths)?;

    let summary = ValidationSummary {
        map_count: catalog.into_maps().len(),
        image_count: image_paths
            .iter()
            .filter(|path| path.ends_with(".bc7z"))
            .count(),
        source_count: provenance.sources.len(),
    };

    replace_usable_bundle(usable_assets, staged_assets)?;
    Ok(summary)
}

fn validate_provenance(provenance: &RefreshProvenance) -> Result<(), RefreshError> {
    if provenance.format_version != 1 {
        return Err(RefreshError::InvalidProvenance(format!(
            "unsupported format version {}",
            provenance.format_version
        )));
    }
    if provenance.sources.is_empty() {
        return Err(RefreshError::InvalidProvenance(
            "at least one source identifier or URL is required".to_owned(),
        ));
    }
    if provenance
        .sources
        .iter()
        .any(|source| source.role.is_empty() || source.identifier.is_empty())
    {
        return Err(RefreshError::InvalidProvenance(
            "source roles and identifiers must not be empty".to_owned(),
        ));
    }
    Ok(())
}

fn replace_usable_bundle(usable_assets: &Path, staged_assets: &Path) -> Result<(), RefreshError> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RefreshError::Install(error.to_string()))?
        .as_nanos();
    let backup = usable_assets.with_extension(format!("refresh-backup-{nonce}"));
    let had_usable_bundle = usable_assets.exists();
    if had_usable_bundle {
        fs::rename(usable_assets, &backup).map_err(|error| {
            RefreshError::Install(format!("back up {}: {error}", usable_assets.display()))
        })?;
    }

    if let Err(error) = fs::rename(staged_assets, usable_assets) {
        if had_usable_bundle {
            fs::rename(&backup, usable_assets).map_err(|restore_error| {
                RefreshError::Install(format!(
                    "install failed ({error}) and restoring {} also failed: {restore_error}",
                    usable_assets.display()
                ))
            })?;
        }
        return Err(RefreshError::Install(format!(
            "move {} to {}: {error}",
            staged_assets.display(),
            usable_assets.display()
        )));
    }

    if had_usable_bundle {
        let _ = fs::remove_dir_all(backup);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        GenerationOptions, ProvenanceSource, RefreshProvenance, StagedBundle, ValidationSummary,
        install_staged_bundle, review_summary, write_staged_provenance,
    };
    use tarkov_map::Map;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("tarkov-map-{name}-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&path).expect("test directory should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn valid_map(name: &str) -> Map {
        Map {
            normalized_name: name.to_owned(),
            name: "Replacement".to_owned(),
            image_path: format!("maps/{name}.bc7z"),
            projection: tarkov_map::Projection {
                game_to_image: euclid::Transform2D::new(2.0, 0.0, 0.0, -2.0, 50.0, 100.0),
                image_size: euclid::Size2D::new(100.0, 200.0),
            },
            alt_maps: None,
            author: None,
            author_link: None,
            bounds: Some([[10.0, 0.0], [0.0, 20.0]]),
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

    fn provenance() -> RefreshProvenance {
        RefreshProvenance::new(
            vec![ProvenanceSource {
                role: "map-data".to_owned(),
                identifier: "https://example.test/maps.json".to_owned(),
                map: None,
            }],
            GenerationOptions {
                tile_zoom_offset: 2,
                svg_render_scale: 2.0,
                force: false,
                convert_only: false,
                image_format: "BC7+zstd".to_owned(),
            },
        )
    }

    #[test]
    fn invalid_staged_bundle_leaves_usable_bundle_unchanged() {
        let root = TestDir::new("invalid-stage");
        let usable = root.path().join("assets");
        let staged = root.path().join("stage");
        fs::create_dir_all(usable.join("maps")).unwrap();
        fs::create_dir_all(staged.join("maps")).unwrap();
        fs::write(usable.join("maps.ron"), "usable metadata").unwrap();
        fs::write(usable.join("maps/usable.bc7z"), b"usable image").unwrap();
        fs::write(staged.join("maps.ron"), "invalid staged metadata").unwrap();
        fs::write(staged.join("maps/replacement.bc7z"), b"replacement image").unwrap();
        write_staged_provenance(&staged, &provenance()).unwrap();

        let error = install_staged_bundle(&usable, &staged).unwrap_err();

        assert!(error.to_string().contains("parse staged maps.ron"));
        assert_eq!(
            fs::read_to_string(usable.join("maps.ron")).unwrap(),
            "usable metadata"
        );
        assert_eq!(
            fs::read(usable.join("maps/usable.bc7z")).unwrap(),
            b"usable image"
        );
        assert!(!usable.join("maps/replacement.bc7z").exists());
    }

    #[test]
    fn valid_staged_bundle_replaces_the_complete_usable_bundle() {
        let root = TestDir::new("valid-stage");
        let usable = root.path().join("assets");
        let staged = root.path().join("stage");
        fs::create_dir_all(usable.join("maps")).unwrap();
        fs::create_dir_all(staged.join("maps")).unwrap();
        fs::write(usable.join("maps.ron"), "old metadata").unwrap();
        fs::write(usable.join("maps/old.bc7z"), b"old image").unwrap();
        fs::write(usable.join("tarkov-map-icon.ico"), b"old icon").unwrap();
        let maps = vec![valid_map("replacement")];
        fs::write(staged.join("maps.ron"), ron::ser::to_string(&maps).unwrap()).unwrap();
        fs::write(staged.join("maps/replacement.bc7z"), b"new image").unwrap();
        fs::write(staged.join("tarkov-map-icon.ico"), b"staged icon").unwrap();
        write_staged_provenance(&staged, &provenance()).unwrap();
        let expected_provenance = fs::read_to_string(staged.join(super::PROVENANCE_FILE)).unwrap();

        let summary = install_staged_bundle(&usable, &staged).unwrap();

        assert_eq!(summary.map_count, 1);
        assert_eq!(summary.image_count, 1);
        assert_eq!(summary.source_count, 1);
        assert!(!usable.join("maps/old.bc7z").exists());
        assert_eq!(
            fs::read(usable.join("maps/replacement.bc7z")).unwrap(),
            b"new image"
        );
        assert_eq!(
            fs::read_to_string(usable.join("maps.ron")).unwrap(),
            ron::ser::to_string(&maps).unwrap()
        );
        assert_eq!(
            fs::read_to_string(usable.join(super::PROVENANCE_FILE)).unwrap(),
            expected_provenance
        );
        assert_eq!(
            fs::read(usable.join("tarkov-map-icon.ico")).unwrap(),
            b"staged icon"
        );
    }

    #[test]
    fn staged_bundle_without_provenance_leaves_usable_bundle_unchanged() {
        let root = TestDir::new("missing-provenance");
        let usable = root.path().join("assets");
        let staged = root.path().join("stage");
        fs::create_dir_all(usable.join("maps")).unwrap();
        fs::create_dir_all(staged.join("maps")).unwrap();
        fs::write(usable.join("maps.ron"), "usable metadata").unwrap();
        fs::write(usable.join("maps/usable.bc7z"), b"usable image").unwrap();
        let maps = vec![valid_map("replacement")];
        fs::write(staged.join("maps.ron"), ron::ser::to_string(&maps).unwrap()).unwrap();
        fs::write(staged.join("maps/replacement.bc7z"), b"new image").unwrap();

        let error = install_staged_bundle(&usable, &staged).unwrap_err();

        assert!(error.to_string().contains("staged provenance"));
        assert_eq!(
            fs::read_to_string(usable.join("maps.ron")).unwrap(),
            "usable metadata"
        );
        assert!(usable.join("maps/usable.bc7z").exists());
    }

    #[test]
    fn refresh_stage_is_separate_and_cleaned_up_when_generation_fails() {
        let root = TestDir::new("stage-lifecycle");
        let usable = root.path().join("assets");
        fs::create_dir_all(&usable).unwrap();

        let staged_path = {
            let staged = StagedBundle::new(&usable).unwrap();
            assert_ne!(staged.path(), usable);
            fs::write(staged.path().join("partial-output"), b"partial").unwrap();
            staged.path().to_owned()
        };

        assert!(!staged_path.exists());
        assert!(usable.exists());
    }

    #[test]
    fn review_summary_reports_provenance_options_and_validation_counts() {
        let summary = review_summary(
            &provenance(),
            &ValidationSummary {
                map_count: 14,
                image_count: 14,
                source_count: 1,
            },
        );

        assert_eq!(
            summary,
            "Provenance (1 source):\n  map-data: https://example.test/maps.json\nGeneration: tile zoom offset 2, SVG scale 2, BC7+zstd, cache allowed\nValidation: 14 Maps, 14 images; Bundle replaced"
        );
    }

    #[test]
    fn captured_sources_have_stable_order_independent_of_discovery_order() {
        let first = ProvenanceSource {
            role: "main-floor-svg".to_owned(),
            identifier: "https://example.test/customs.svg".to_owned(),
            map: Some("customs".to_owned()),
        };
        let second = ProvenanceSource {
            role: "map-data".to_owned(),
            identifier: "https://example.test/maps.json".to_owned(),
            map: None,
        };

        let forward =
            RefreshProvenance::new(vec![first.clone(), second.clone()], provenance().options);
        let reverse = RefreshProvenance::new(vec![second, first], provenance().options);

        assert_eq!(forward, reverse);
        assert_eq!(
            forward
                .sources
                .iter()
                .map(|source| source.role.as_str())
                .collect::<Vec<_>>(),
            vec!["main-floor-svg", "map-data"]
        );
    }
}
