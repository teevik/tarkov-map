//! Fetches and processes Tarkov map assets from the tarkov-dev repository.
//!
//! Downloads map metadata, SVG files, and tile pyramids, then generates a local
//! `maps.ron` file for the viewer application.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use image::{ImageBuffer, RgbaImage};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use resvg::tiny_skia::Pixmap;
use resvg::usvg::{Options, Transform, Tree};
use ron::ser::PrettyConfig;
use serde::Deserialize;
use thiserror::Error;
use tokio::fs as async_fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use tarkov_map::{
    BossChance, BossSpawn, BtrStop, Extract, Label, Map, Minefield, SniperZone, Spawn, Switch,
    TarkovMaps, Transit,
};

/// Errors that can occur during the fetch_maps process.
#[derive(Error, Debug)]
pub enum FetchError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to fetch {resource}: HTTP {status}")]
    HttpStatus { resource: String, status: u16 },

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SVG parse error: {0}")]
    SvgParse(String),

    #[error("failed to create pixmap for rendering")]
    PixmapCreation,

    #[error("failed to save PNG: {0}")]
    PngSave(String),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("RON serialization error: {0}")]
    Ron(#[from] ron::Error),

    #[error("progress bar template error: {0}")]
    ProgressTemplate(#[from] indicatif::style::TemplateError),

    #[error("semaphore acquire error: {0}")]
    Semaphore(#[from] tokio::sync::AcquireError),

    #[error("task join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("map '{name}' has no human-readable name")]
    MissingMapName { name: String },

    #[error("map '{name}' has no svgPath or tilePath")]
    MissingMapSource { name: String },

    #[error("map '{name}' is missing minZoom")]
    MissingMinZoom { name: String },

    #[error("map '{name}' is missing maxZoom")]
    MissingMaxZoom { name: String },

    #[error("bc7z container error: {0}")]
    Bc7z(#[from] tarkov_map::bc7z::Bc7zError),

    #[error("missing PNG source for '{path}' — run a full fetch first")]
    MissingPngSource { path: String },
}

/// Result of downloading a single tile.
type TileResult = Result<(u32, u32, Vec<u8>), FetchError>;

/// Fetch Tarkov map assets from tarkov-dev
#[derive(Parser, Debug)]
#[command(name = "fetch_maps", version, about)]
struct Args {
    /// Force re-download of all assets, ignoring cached files
    #[arg(short, long)]
    force: bool,

    /// Reduce tile map zoom level from max (0 = max quality, higher = smaller files)
    #[arg(long, default_value = "2")]
    tile_zoom_offset: i32,

    /// Skip fetching: re-encode the existing PNGs referenced by maps.ron into
    /// .bc7z containers and rewrite maps.ron to point at them.
    #[arg(long)]
    convert_only: bool,
}

const MAPS_JSON_URL: &str =
    "https://raw.githubusercontent.com/the-hideout/tarkov-dev/main/src/data/maps.json";
const TARKOV_DEV_MAPS_URL: &str = "https://json.tarkov.dev/regular/maps";
const TARKOV_DEV_MAPS_EN_URL: &str = "https://json.tarkov.dev/regular/maps_en";
const USER_AGENT: &str = "tarkov-map";
const MAPS_RON_PATH: &str = "assets/maps.ron";
/// Physical directory for storing map images on disk
const MAPS_DIR: &str = "assets/maps";
/// Path prefix for maps.ron (relative to assets/ for rust-embed)
const MAPS_PATH_PREFIX: &str = "maps";
const TILE_DOWNLOAD_CONCURRENCY: usize = 32;
const SVG_RENDER_SCALE: f32 = 2.0;

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
    tile_path: Option<String>,
    #[serde(default)]
    labels: Option<Vec<FetchedLabel>>,
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

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct ApiMapsData {
    maps: HashMap<String, ApiMap>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiMap {
    id: String,
    name: String,
    normalized_name: String,
    #[serde(default)]
    spawns: Vec<ApiSpawn>,
    #[serde(default)]
    extracts: Vec<ApiExtract>,
    #[serde(default)]
    hazards: Vec<ApiHazard>,
    #[serde(default)]
    bosses: Vec<ApiBoss>,
    #[serde(default)]
    transits: Vec<ApiTransit>,
    #[serde(default)]
    switches: Vec<ApiSwitch>,
    #[serde(default)]
    btr_stops: Vec<ApiBtrStop>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiHazard {
    #[serde(default)]
    hazard_type: Option<String>,
    #[serde(default)]
    position: Option<ApiPosition>,
    #[serde(default)]
    outline: Vec<ApiPosition>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiBoss {
    mob: String,
    spawn_chance: f64,
    #[serde(default)]
    spawn_locations: Vec<ApiBossLocation>,
}

#[derive(Clone, Debug, Deserialize)]
struct ApiBossLocation {
    #[serde(default)]
    positions: Vec<ApiPosition>,
}

#[derive(Clone, Debug, Deserialize)]
struct ApiTransit {
    #[serde(default)]
    map: Option<String>,
    #[serde(default)]
    position: Option<ApiPosition>,
}

#[derive(Clone, Debug, Deserialize)]
struct ApiSwitch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    position: Option<ApiPosition>,
}

#[derive(Clone, Debug, Deserialize)]
struct ApiBtrStop {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    z: Option<f64>,
}

#[derive(Clone, Debug, Default)]
struct ApiOverlayMap {
    hazards: Vec<ApiHazard>,
    bosses: Vec<ApiBoss>,
    transits: Vec<ApiTransit>,
    switches: Vec<ApiSwitch>,
    btr_stops: Vec<ApiBtrStop>,
}

#[derive(Debug, Deserialize)]
struct ApiSpawn {
    position: ApiPosition,
    #[serde(default)]
    sides: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiExtract {
    name: String,
    faction: String,
    #[serde(default)]
    position: Option<ApiPosition>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiPosition {
    x: f64,
    y: f64,
    z: f64,
}

struct ApiMapData {
    names: HashMap<String, String>,
    spawns: HashMap<String, Vec<Spawn>>,
    extracts: HashMap<String, Vec<Extract>>,
    overlay_maps: HashMap<String, ApiOverlayMap>,
    id_to_normalized_name: HashMap<String, String>,
    translations: HashMap<String, String>,
}

struct LocatedArea<T> {
    area: T,
}

struct HazardSplit {
    sniper_zones: Vec<LocatedArea<SniperZone>>,
    minefields: Vec<LocatedArea<Minefield>>,
}

#[derive(Default)]
struct OverlayData {
    sniper_zones: Vec<SniperZone>,
    minefields: Vec<Minefield>,
    boss_spawns: Vec<BossSpawn>,
    transits: Vec<Transit>,
    switches: Vec<Switch>,
    btr_stops: Vec<BtrStop>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PositionKey(i64, i64);

impl PositionKey {
    fn new(position: [f64; 2]) -> Self {
        Self(
            (position[0] * 100.0).round() as i64,
            (position[1] * 100.0).round() as i64,
        )
    }
}

fn round_coordinate(value: f64) -> f64 {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 { 0.0 } else { rounded }
}

fn game_position(position: &ApiPosition) -> [f64; 2] {
    [round_coordinate(position.x), round_coordinate(position.z)]
}

fn translated_name(
    translations: &HashMap<String, String>,
    key: &str,
    warnings: &mut Vec<String>,
) -> String {
    translations.get(key).cloned().unwrap_or_else(|| {
        warnings.push(format!("missing English translation for '{key}'"));
        key.to_owned()
    })
}

fn group_boss_spawns(
    bosses: Vec<ApiBoss>,
    translations: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> Vec<BossSpawn> {
    struct Group {
        position: [f64; 2],
        chances: HashMap<String, f64>,
    }

    let mut indexes = HashMap::<PositionKey, usize>::new();
    let mut groups = Vec::<Group>::new();

    for boss in bosses {
        let positions: Vec<_> = boss
            .spawn_locations
            .iter()
            .flat_map(|location| &location.positions)
            .collect();
        if positions.is_empty() {
            continue;
        }
        let name = translated_name(translations, &boss.mob, warnings);

        for raw_position in positions {
            let position = game_position(raw_position);
            let key = PositionKey::new(position);
            let index = *indexes.entry(key).or_insert_with(|| {
                groups.push(Group {
                    position,
                    chances: HashMap::new(),
                });
                groups.len() - 1
            });
            groups[index]
                .chances
                .entry(name.clone())
                .and_modify(|chance| *chance = chance.max(boss.spawn_chance))
                .or_insert(boss.spawn_chance);
        }
    }

    groups
        .into_iter()
        .map(|group| {
            let mut mobs: Vec<_> = group
                .chances
                .into_iter()
                .map(|(name, chance)| BossChance { name, chance })
                .collect();
            mobs.sort_by(|a, b| {
                b.chance
                    .total_cmp(&a.chance)
                    .then_with(|| a.name.cmp(&b.name))
            });
            BossSpawn {
                position: group.position,
                mobs,
            }
        })
        .collect()
}

fn convert_transits(
    transits: Vec<ApiTransit>,
    id_to_normalized_name: &HashMap<String, String>,
    canonical_by_alias: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> Vec<Transit> {
    let mut seen = HashSet::new();
    let mut converted = Vec::new();

    for transit in transits {
        let Some(position) = transit.position.as_ref() else {
            warnings.push("dropping transit without a position".into());
            continue;
        };
        let Some(target) = transit
            .map
            .as_ref()
            .and_then(|id| id_to_normalized_name.get(id))
            .and_then(|normalized_name| canonical_by_alias.get(normalized_name))
            .cloned()
        else {
            warnings.push(format!(
                "dropping transit with unknown target {:?}",
                transit.map
            ));
            continue;
        };

        let position = game_position(position);
        if seen.insert((PositionKey::new(position), target.clone())) {
            converted.push(Transit { position, target });
        }
    }

    converted
}

fn split_hazards(hazards: Vec<ApiHazard>, warnings: &mut Vec<String>) -> HazardSplit {
    let mut split = HazardSplit {
        sniper_zones: Vec::new(),
        minefields: Vec::new(),
    };
    let mut seen_sniper_zones = HashSet::new();
    let mut seen_minefields = HashSet::new();

    for hazard in hazards {
        let Some(position) = hazard.position.as_ref() else {
            warnings.push("dropping hazard without a position".into());
            continue;
        };
        if hazard.outline.len() < 3 {
            warnings.push("dropping hazard with fewer than three outline vertices".into());
            continue;
        }

        let position = game_position(position);
        let outline = hazard.outline.iter().map(game_position).collect();
        match hazard.hazard_type.as_deref() {
            Some("sniper") if seen_sniper_zones.insert(PositionKey::new(position)) => {
                split.sniper_zones.push(LocatedArea {
                    area: SniperZone { outline },
                });
            }
            Some("minefield" | "hazard") if seen_minefields.insert(PositionKey::new(position)) => {
                split.minefields.push(LocatedArea {
                    area: Minefield { outline },
                });
            }
            Some("sniper" | "minefield" | "hazard") => {}
            kind => warnings.push(format!("dropping unknown hazard type {kind:?}")),
        }
    }

    split
}

fn convert_switches(
    switches: Vec<ApiSwitch>,
    translations: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> Vec<Switch> {
    let mut seen = HashSet::new();
    let mut converted = Vec::new();

    for raw_switch in switches {
        let Some(position) = raw_switch.position.as_ref() else {
            warnings.push("dropping switch without a position".into());
            continue;
        };
        let Some(name_key) = raw_switch.name.as_deref() else {
            warnings.push("dropping switch without a name".into());
            continue;
        };
        let position = game_position(position);
        let name = translated_name(translations, name_key, warnings);
        if seen.insert((PositionKey::new(position), name.clone())) {
            converted.push(Switch { position, name });
        }
    }

    converted
}

fn convert_btr_stops(
    stops: Vec<ApiBtrStop>,
    translations: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> Vec<BtrStop> {
    let mut seen = HashSet::new();
    let mut converted = Vec::new();

    for stop in stops {
        let (Some(x), Some(z)) = (stop.x, stop.z) else {
            warnings.push("dropping BTR stop without a position".into());
            continue;
        };
        let Some(name_key) = stop.name.as_deref() else {
            warnings.push("dropping BTR stop without a name".into());
            continue;
        };
        let position = [round_coordinate(x), round_coordinate(z)];
        let name = translated_name(translations, name_key, warnings);
        if seen.insert((PositionKey::new(position), name.clone())) {
            converted.push(BtrStop { position, name });
        }
    }

    converted
}

fn collect_overlay_data(
    source_names: &[String],
    maps: &HashMap<String, ApiOverlayMap>,
    translations: &HashMap<String, String>,
    id_to_normalized_name: &HashMap<String, String>,
    canonical_by_alias: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> OverlayData {
    let mut hazards = Vec::new();
    let mut bosses = Vec::new();
    let mut transits = Vec::new();
    let mut switches = Vec::new();
    let mut btr_stops = Vec::new();

    for source_name in source_names {
        let Some(map) = maps.get(source_name) else {
            continue;
        };
        hazards.extend(map.hazards.clone());
        bosses.extend(map.bosses.clone());
        transits.extend(map.transits.clone());
        switches.extend(map.switches.clone());
        btr_stops.extend(map.btr_stops.clone());
    }

    let hazards = split_hazards(hazards, warnings);
    OverlayData {
        sniper_zones: hazards
            .sniper_zones
            .into_iter()
            .map(|located| located.area)
            .collect(),
        minefields: hazards
            .minefields
            .into_iter()
            .map(|located| located.area)
            .collect(),
        boss_spawns: group_boss_spawns(bosses, translations, warnings),
        transits: convert_transits(
            transits,
            id_to_normalized_name,
            canonical_by_alias,
            warnings,
        ),
        switches: convert_switches(switches, translations, warnings),
        btr_stops: convert_btr_stops(btr_stops, translations, warnings),
    }
}

fn canonical_map_by_alias(groups: &[FetchedMapGroup]) -> HashMap<String, String> {
    let mut aliases = HashMap::new();

    for group in groups {
        let Some(interactive) = group
            .maps
            .iter()
            .find(|map| map.projection == "interactive")
        else {
            continue;
        };
        aliases.insert(group.normalized_name.clone(), group.normalized_name.clone());
        for alt_map in interactive.alt_maps.iter().flatten() {
            aliases.insert(alt_map.clone(), group.normalized_name.clone());
        }
    }

    aliases
}

fn deserialize_rotation<'de, D>(deserializer: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => Ok(n.as_f64()),
        Some(serde_json::Value::String(s)) => s
            .parse()
            .map(Some)
            .map_err(|_| D::Error::custom(format!("invalid rotation string: {s}"))),
        Some(other) => Err(D::Error::custom(format!(
            "expected number or string for rotation, got: {other:?}"
        ))),
    }
}

impl From<FetchedLabel> for Label {
    fn from(f: FetchedLabel) -> Self {
        Self {
            position: [
                round_coordinate(f.position[0]),
                round_coordinate(f.position[1]),
            ],
            text: f.text,
            rotation: f.rotation,
            size: f.size,
            top: f.top.map(round_coordinate),
            bottom: f.bottom.map(round_coordinate),
        }
    }
}

fn round_bounds(bounds: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    bounds.map(|corner| corner.map(round_coordinate))
}

async fn fetch_api_map_data(client: &reqwest::Client) -> Result<ApiMapData, FetchError> {
    let maps_response = client
        .get(TARKOV_DEV_MAPS_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;
    if !maps_response.status().is_success() {
        return Err(FetchError::HttpStatus {
            resource: "tarkov.dev map data".into(),
            status: maps_response.status().as_u16(),
        });
    }
    let api_maps: ApiResponse<ApiMapsData> = maps_response.json().await?;

    let names_response = client
        .get(TARKOV_DEV_MAPS_EN_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;
    if !names_response.status().is_success() {
        return Err(FetchError::HttpStatus {
            resource: "tarkov.dev English translations".into(),
            status: names_response.status().as_u16(),
        });
    }
    let translations: ApiResponse<HashMap<String, String>> = names_response.json().await?;

    let mut names = HashMap::new();
    let mut spawns = HashMap::new();
    let mut extracts = HashMap::new();
    let mut overlay_maps = HashMap::new();
    let mut id_to_normalized_name = HashMap::new();
    let mut warnings = Vec::new();

    for map in api_maps.data.maps.into_values() {
        let ApiMap {
            id,
            name,
            normalized_name,
            spawns: raw_spawns,
            extracts: raw_extracts,
            hazards,
            bosses,
            transits,
            switches,
            btr_stops,
        } = map;
        id_to_normalized_name.insert(id, normalized_name.clone());
        names.insert(
            normalized_name.clone(),
            translated_name(&translations.data, &name, &mut warnings),
        );
        spawns.insert(
            normalized_name.clone(),
            raw_spawns
                .into_iter()
                .filter(|spawn| {
                    spawn
                        .sides
                        .iter()
                        .any(|side| side == "pmc" || side == "all")
                        && spawn.categories.iter().any(|category| category == "player")
                })
                .map(|spawn| Spawn {
                    position: [
                        round_coordinate(spawn.position.x),
                        round_coordinate(spawn.position.y),
                        round_coordinate(spawn.position.z),
                    ],
                    sides: spawn.sides,
                    categories: spawn.categories,
                })
                .collect(),
        );
        extracts.insert(
            normalized_name.clone(),
            raw_extracts
                .into_iter()
                .map(|extract| Extract {
                    name: translated_name(&translations.data, &extract.name, &mut warnings),
                    faction: extract.faction,
                    position: extract.position.map(|p| {
                        [
                            round_coordinate(p.x),
                            round_coordinate(p.y),
                            round_coordinate(p.z),
                        ]
                    }),
                })
                .collect(),
        );
        overlay_maps.insert(
            normalized_name,
            ApiOverlayMap {
                hazards,
                bosses,
                transits,
                switches,
                btr_stops,
            },
        );
    }

    for warning in warnings {
        eprintln!("Warning: {warning}");
    }

    Ok(ApiMapData {
        names,
        spawns,
        extracts,
        overlay_maps,
        id_to_normalized_name,
        translations: translations.data,
    })
}

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// Encodes one PNG into a `.bc7z` container: premultiply alpha (the app's
/// texture convention; also stops BC7 blocks leaking color into transparent
/// regions), pad to multiples of 4, BC7-compress, zstd-pack.
fn encode_png_to_bc7z(png_path: &std::path::Path) -> Result<Vec<u8>, FetchError> {
    let mut rgba = image::open(png_path)?.to_rgba8();
    let (width, height) = rgba.dimensions();

    for px in rgba.chunks_exact_mut(4) {
        let alpha = px[3] as u32;
        px[0] = (px[0] as u32 * alpha / 255) as u8;
        px[1] = (px[1] as u32 * alpha / 255) as u8;
        px[2] = (px[2] as u32 * alpha / 255) as u8;
    }

    let padded = [width.div_ceil(4) * 4, height.div_ceil(4) * 4];
    if padded != [width, height] {
        let mut canvas = RgbaImage::new(padded[0], padded[1]);
        image::imageops::overlay(&mut canvas, &rgba, 0, 0);
        rgba = canvas;
    }

    let blocks = intel_tex_2::bc7::compress_blocks(
        &intel_tex_2::bc7::alpha_basic_settings(),
        &intel_tex_2::RgbaSurface {
            width: padded[0],
            height: padded[1],
            stride: padded[0] * 4,
            data: &rgba,
        },
    );

    Ok(tarkov_map::bc7z::pack(&tarkov_map::bc7z::Bc7Image {
        pixel_size: [width, height],
        padded_size: padded,
        blocks,
    })?)
}

/// Maps an `image_path` entry (either `maps/X.png` or `maps/X.bc7z`) to its
/// PNG source stem, e.g. `maps/customs`.
fn image_path_stem(entry: &str) -> &str {
    entry
        .strip_suffix(".png")
        .or_else(|| entry.strip_suffix(".bc7z"))
        .unwrap_or(entry)
}

/// Encodes every image referenced by `maps` into a `.bc7z` next to its PNG
/// source and rewrites the `image_path` entries to point at the containers.
/// Existing up-to-date containers are skipped unless `force` is set.
fn encode_all_assets(maps: &mut TarkovMaps, force: bool) -> Result<(), FetchError> {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    let mut stems = BTreeSet::new();
    for map in maps.iter() {
        stems.insert(image_path_stem(&map.image_path).to_owned());
    }

    let jobs: Vec<String> = stems
        .iter()
        .filter(|stem| force || !repo_path(&format!("assets/{stem}.bc7z")).exists())
        .cloned()
        .collect();

    if !jobs.is_empty() {
        println!("Encoding {} images to BC7+zstd...", jobs.len());
        let progress = ProgressBar::new(jobs.len() as u64);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} encoded ({eta})")?
                .progress_chars("=>-"),
        );

        let queue = Mutex::new(jobs.into_iter());
        let failures: Mutex<Vec<FetchError>> = Mutex::new(Vec::new());
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let Some(stem) = queue.lock().unwrap().next() else {
                            return;
                        };
                        let png = repo_path(&format!("assets/{stem}.png"));
                        let result = if png.exists() {
                            encode_png_to_bc7z(&png).and_then(|container| {
                                Ok(std::fs::write(
                                    repo_path(&format!("assets/{stem}.bc7z")),
                                    container,
                                )?)
                            })
                        } else {
                            Err(FetchError::MissingPngSource {
                                path: format!("assets/{stem}.png"),
                            })
                        };
                        if let Err(error) = result {
                            failures.lock().unwrap().push(error);
                        }
                        progress.inc(1);
                    }
                });
            }
        });
        progress.finish_and_clear();

        let failures = failures.into_inner().unwrap();
        if !failures.is_empty() {
            for error in failures.iter().skip(1) {
                eprintln!("  encoding also failed: {error}");
            }
            return Err(failures.into_iter().next().unwrap());
        }
    }

    // Rewrite entries to the containers.
    for map in maps.iter_mut() {
        map.image_path = format!("{}.bc7z", image_path_stem(&map.image_path));
    }

    Ok(())
}

fn write_maps_ron(maps: &TarkovMaps) -> Result<(), FetchError> {
    let pretty_config = PrettyConfig::new()
        .depth_limit(10)
        .indentor("  ".to_owned())
        .struct_names(true)
        .enumerate_arrays(false);

    let ron_string = ron::ser::to_string_pretty(maps, pretty_config)?;
    println!("Serialized to {} bytes of RON", ron_string.len());

    std::fs::create_dir_all(repo_path(MAPS_DIR))?;

    let output_path = repo_path(MAPS_RON_PATH);
    std::fs::write(&output_path, &ron_string)?;
    println!("Wrote maps to {}", output_path.display());
    Ok(())
}

struct ImageResult {
    image_path: String,
    image_size: [f32; 2],
}

async fn process_svg_map(
    client: &reqwest::Client,
    normalized_name: &str,
    svg_url: &str,
    force: bool,
) -> Result<ImageResult, FetchError> {
    let image_relative = format!("{MAPS_PATH_PREFIX}/{normalized_name}.png");
    let image_disk_path = repo_path(&format!("{MAPS_DIR}/{normalized_name}.png"));

    if !force && image_disk_path.exists() {
        let img = image::open(&image_disk_path)?;
        let source_size = [
            img.width() as f32 / SVG_RENDER_SCALE,
            img.height() as f32 / SVG_RENDER_SCALE,
        ];
        return Ok(ImageResult {
            image_path: image_relative,
            image_size: source_size,
        });
    }

    let response = client
        .get(svg_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(FetchError::HttpStatus {
            resource: "SVG".into(),
            status: response.status().as_u16(),
        });
    }

    let svg_bytes = response.bytes().await?;
    let tree = Tree::from_data(&svg_bytes, &Options::default())
        .map_err(|e| FetchError::SvgParse(e.to_string()))?;

    let source_size = [tree.size().width(), tree.size().height()];
    let render_w = (source_size[0] * SVG_RENDER_SCALE) as u32;
    let render_h = (source_size[1] * SVG_RENDER_SCALE) as u32;

    let mut pixmap = Pixmap::new(render_w, render_h).ok_or(FetchError::PixmapCreation)?;

    resvg::render(
        &tree,
        Transform::from_scale(SVG_RENDER_SCALE, SVG_RENDER_SCALE),
        &mut pixmap.as_mut(),
    );

    if let Some(parent) = image_disk_path.parent() {
        async_fs::create_dir_all(parent).await?;
    }
    pixmap
        .save_png(&image_disk_path)
        .map_err(|e| FetchError::PngSave(e.to_string()))?;

    Ok(ImageResult {
        image_path: image_relative,
        image_size: source_size,
    })
}

#[allow(clippy::too_many_arguments)]
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
) -> Result<ImageResult, FetchError> {
    let image_relative = format!("{MAPS_PATH_PREFIX}/{normalized_name}.png");
    let image_disk_path = repo_path(&format!("{MAPS_DIR}/{normalized_name}.png"));

    let zoom = (max_zoom - zoom_offset).max(min_zoom);
    let tiles_per_axis = 1u32 << zoom;
    let full_size = tiles_per_axis * tile_size as u32;
    let source_size = [tile_size as f32, tile_size as f32];

    if !force && image_disk_path.exists() {
        return Ok(ImageResult {
            image_path: image_relative,
            image_size: source_size,
        });
    }

    let tile_pb = multi_progress.add(ProgressBar::new((tiles_per_axis * tiles_per_axis) as u64));
    tile_pb.set_style(
        ProgressStyle::default_bar()
            .template("    {spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} tiles ({eta})")?
            .progress_chars("=>-"),
    );

    let semaphore = Arc::new(Semaphore::new(TILE_DOWNLOAD_CONCURRENCY));
    let tile_pb = Arc::new(tile_pb);
    let mut join_set: JoinSet<TileResult> = JoinSet::new();

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
                    return Err(FetchError::HttpStatus {
                        resource: "tile".into(),
                        status: response.status().as_u16(),
                    });
                }

                let bytes = response.bytes().await?.to_vec();
                tile_pb.inc(1);
                Ok((x, y, bytes))
            });
        }
    }

    let mut tiles = Vec::new();
    while let Some(result) = join_set.join_next().await {
        tiles.push(result??);
    }
    tile_pb.finish_and_clear();

    let compose_pb = multi_progress.add(ProgressBar::new(tiles.len() as u64));
    compose_pb.set_style(
        ProgressStyle::default_bar()
            .template("    {spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} composing")?
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

    if let Some(parent) = image_disk_path.parent() {
        async_fs::create_dir_all(parent).await?;
    }
    full_image.save(&image_disk_path)?;

    Ok(ImageResult {
        image_path: image_relative,
        image_size: source_size,
    })
}

#[allow(clippy::too_many_arguments)]
async fn convert_group(
    client: &reqwest::Client,
    fetched: FetchedMapGroup,
    api_data: &ApiMapData,
    canonical_by_alias: &HashMap<String, String>,
    multi_progress: &MultiProgress,
    force: bool,
    tile_zoom_offset: i32,
) -> Result<Option<Map>, FetchError> {
    let FetchedMapGroup {
        normalized_name,
        maps,
    } = fetched;

    let Some(interactive) = maps.into_iter().find(|m| m.projection == "interactive") else {
        return Ok(None);
    };

    let name = api_data
        .names
        .get(&normalized_name)
        .cloned()
        .ok_or_else(|| FetchError::MissingMapName {
            name: normalized_name.clone(),
        })?;

    let try_tiles =
        |interactive: &FetchedMap| -> Result<Option<(String, i32, i32, i32)>, FetchError> {
            match &interactive.tile_path {
                Some(tile_template) => {
                    let min_zoom =
                        interactive
                            .min_zoom
                            .ok_or_else(|| FetchError::MissingMinZoom {
                                name: normalized_name.clone(),
                            })?;
                    let max_zoom =
                        interactive
                            .max_zoom
                            .ok_or_else(|| FetchError::MissingMaxZoom {
                                name: normalized_name.clone(),
                            })?;
                    let tile_size = interactive.tile_size.unwrap_or(256);
                    Ok(Some((tile_template.clone(), tile_size, min_zoom, max_zoom)))
                }
                None => Ok(None),
            }
        };

    let result = match &interactive.svg_path {
        Some(svg_url) => {
            match process_svg_map(client, &normalized_name, svg_url, force).await {
                Ok(r) => r,
                Err(svg_err) => {
                    // SVG failed — try falling back to tiles if available
                    if let Some((tile_template, tile_size, min_zoom, max_zoom)) =
                        try_tiles(&interactive)?
                    {
                        multi_progress.println(format!(
                            "  Warning: SVG failed for {normalized_name} ({svg_err}), falling back to tiles"
                        ))?;
                        process_tile_map(
                            client,
                            &normalized_name,
                            &tile_template,
                            tile_size,
                            min_zoom,
                            max_zoom,
                            tile_zoom_offset,
                            multi_progress,
                            force,
                        )
                        .await?
                    } else {
                        return Err(svg_err);
                    }
                }
            }
        }
        None => {
            if let Some((tile_template, tile_size, min_zoom, max_zoom)) = try_tiles(&interactive)? {
                process_tile_map(
                    client,
                    &normalized_name,
                    &tile_template,
                    tile_size,
                    min_zoom,
                    max_zoom,
                    tile_zoom_offset,
                    multi_progress,
                    force,
                )
                .await?
            } else {
                return Err(FetchError::MissingMapSource {
                    name: normalized_name,
                });
            }
        }
    };

    let logical_size = interactive
        .bounds
        .map(|bounds| {
            let width = (bounds[0][0] - bounds[1][0]).abs() as f32;
            let height = (bounds[1][1] - bounds[0][1]).abs() as f32;
            [width, height]
        })
        .unwrap_or(result.image_size);

    let mut source_names = vec![normalized_name.clone()];
    source_names.extend(interactive.alt_maps.iter().flatten().cloned());
    let mut warnings = Vec::new();
    let overlays = collect_overlay_data(
        &source_names,
        &api_data.overlay_maps,
        &api_data.translations,
        &api_data.id_to_normalized_name,
        canonical_by_alias,
        &mut warnings,
    );
    for warning in warnings {
        multi_progress.println(format!("  Warning: {normalized_name}: {warning}"))?;
    }

    Ok(Some(Map {
        normalized_name: normalized_name.clone(),
        name,
        image_path: result.image_path,
        image_size: result.image_size,
        logical_size,
        alt_maps: interactive.alt_maps,
        author: interactive.author,
        author_link: interactive.author_link,
        transform: interactive.transform,
        coordinate_rotation: interactive.coordinate_rotation,
        bounds: interactive.bounds.map(round_bounds),
        labels: interactive
            .labels
            .map(|l| l.into_iter().map(Into::into).collect()),
        spawns: api_data.spawns.get(&normalized_name).cloned(),
        extracts: api_data.extracts.get(&normalized_name).cloned(),
        sniper_zones: overlays.sniper_zones,
        minefields: overlays.minefields,
        boss_spawns: overlays.boss_spawns,
        transits: overlays.transits,
        switches: overlays.switches,
        btr_stops: overlays.btr_stops,
    }))
}

#[tokio::main]
async fn main() -> Result<(), FetchError> {
    env_logger::init();

    let args = Args::parse();

    if args.convert_only {
        println!("Convert-only: encoding existing PNGs referenced by maps.ron");
        let ron_string = std::fs::read_to_string(repo_path(MAPS_RON_PATH))?;
        let mut maps: TarkovMaps = ron::from_str(&ron_string).map_err(|e| e.code)?;
        encode_all_assets(&mut maps, args.force)?;
        write_maps_ron(&maps)?;
        return Ok(());
    }

    if args.force {
        println!("Force mode enabled - re-processing all assets");
    }

    let client = reqwest::Client::new();

    println!("Fetching map data from tarkov.dev...");
    let api_data = fetch_api_map_data(&client).await?;
    println!("Fetched {} map names", api_data.names.len());
    let total_spawns: usize = api_data.spawns.values().map(Vec::len).sum();
    println!("Fetched {total_spawns} PMC spawns");
    let total_extracts: usize = api_data.extracts.values().map(Vec::len).sum();
    println!("Fetched {total_extracts} extracts");

    println!("Fetching maps from tarkov-dev...");

    let response = client
        .get(MAPS_JSON_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(FetchError::HttpStatus {
            resource: "maps.json".into(),
            status: response.status().as_u16(),
        });
    }

    let json_text = response.text().await?;
    println!("Fetched {} bytes of JSON", json_text.len());

    let fetched_maps: Vec<FetchedMapGroup> = serde_json::from_str(&json_text)?;
    let canonical_by_alias = canonical_map_by_alias(&fetched_maps);
    println!("Parsed {} map groups\n", fetched_maps.len());

    let multi_progress = MultiProgress::new();
    let maps_pb = multi_progress.add(ProgressBar::new(fetched_maps.len() as u64));
    maps_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} maps - {msg}")?
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
            &api_data,
            &canonical_by_alias,
            &multi_progress,
            args.force,
            args.tile_zoom_offset,
        )
        .await
        {
            Ok(Some(map)) => maps.push(map),
            Ok(None) => skipped += 1,
            Err(e) => {
                multi_progress.println(format!("  Warning: skipping {group_name}: {e}"))?;
                skipped += 1;
            }
        }

        maps_pb.inc(1);
    }

    maps_pb.finish_with_message("Done");

    println!(
        "\nProcessed {} interactive maps (skipped {skipped})",
        maps.len()
    );

    encode_all_assets(&mut maps, args.force)?;
    write_maps_ron(&maps)?;

    println!("\nMaps:");
    for map in &maps {
        println!("  - {} ({})", map.name, map.normalized_name);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_game_coordinates_to_two_decimal_places() {
        assert_eq!(round_coordinate(12.345), 12.35);
        assert_eq!(round_coordinate(-12.345), -12.35);
        assert_eq!(round_coordinate(12.344), 12.34);
        assert_eq!(round_coordinate(-0.001), 0.0);
    }

    #[test]
    fn missing_translation_falls_back_to_the_key_with_a_warning() {
        let mut warnings = Vec::new();

        let name = translated_name(&HashMap::new(), "missing-key", &mut warnings);

        assert_eq!(name, "missing-key");
        assert_eq!(
            warnings,
            vec!["missing English translation for 'missing-key'"]
        );
    }

    #[test]
    fn splits_valid_hazards_and_drops_invalid_entries_with_warnings() {
        let position = Some(ApiPosition {
            x: 10.126,
            y: 4.0,
            z: -20.125,
        });
        let outline = vec![
            ApiPosition {
                x: 1.0,
                y: 0.0,
                z: 2.0,
            },
            ApiPosition {
                x: 3.0,
                y: 0.0,
                z: 4.0,
            },
            ApiPosition {
                x: 5.0,
                y: 0.0,
                z: 6.0,
            },
        ];
        let hazards = vec![
            ApiHazard {
                hazard_type: Some("sniper".into()),
                position: position.clone(),
                outline: outline.clone(),
            },
            ApiHazard {
                hazard_type: Some("hazard".into()),
                position,
                outline,
            },
            ApiHazard {
                hazard_type: Some("minefield".into()),
                position: None,
                outline: Vec::new(),
            },
            ApiHazard {
                hazard_type: Some("minefield".into()),
                position: Some(ApiPosition {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                outline: vec![
                    ApiPosition {
                        x: 1.0,
                        y: 0.0,
                        z: 2.0,
                    },
                    ApiPosition {
                        x: 3.0,
                        y: 0.0,
                        z: 4.0,
                    },
                ],
            },
        ];
        let mut warnings = Vec::new();

        let split = split_hazards(hazards, &mut warnings);

        assert_eq!(split.sniper_zones.len(), 1);
        assert_eq!(split.minefields.len(), 1);
        assert_eq!(split.sniper_zones[0].area.outline[0], [1.0, 2.0]);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn groups_boss_spawns_by_rounded_position_and_translated_mob_name() {
        let at = |x, z| ApiPosition { x, y: 0.0, z };
        let location = |positions| ApiBossLocation { positions };
        let bosses = vec![
            ApiBoss {
                mob: "raw-af-a".into(),
                spawn_chance: 0.3,
                spawn_locations: vec![location(vec![at(10.001, 20.001)])],
            },
            ApiBoss {
                mob: "raw-af-b".into(),
                spawn_chance: 0.4,
                spawn_locations: vec![location(vec![at(10.004, 20.004)])],
            },
            ApiBoss {
                mob: "raw-zed".into(),
                spawn_chance: 0.4,
                spawn_locations: vec![location(vec![at(10.0, 20.0)])],
            },
            ApiBoss {
                mob: "raw-alpha".into(),
                spawn_chance: 0.4,
                spawn_locations: vec![location(vec![at(10.0, 20.0)])],
            },
            ApiBoss {
                mob: "positionless".into(),
                spawn_chance: 1.0,
                spawn_locations: Vec::new(),
            },
        ];
        let translations = HashMap::from([
            ("raw-af-a".into(), "AF".into()),
            ("raw-af-b".into(), "AF".into()),
            ("raw-zed".into(), "Zed".into()),
            ("raw-alpha".into(), "Alpha".into()),
            ("positionless".into(), "Nobody".into()),
        ]);
        let mut warnings = Vec::new();

        let spawns = group_boss_spawns(bosses, &translations, &mut warnings);

        assert_eq!(spawns.len(), 1);
        assert_eq!(spawns[0].position, [10.0, 20.0]);
        assert_eq!(
            spawns[0].mobs,
            vec![
                BossChance {
                    name: "AF".into(),
                    chance: 0.4
                },
                BossChance {
                    name: "Alpha".into(),
                    chance: 0.4
                },
                BossChance {
                    name: "Zed".into(),
                    chance: 0.4
                },
            ]
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn resolves_transits_to_canonical_maps_and_drops_unknown_or_positionless_entries() {
        let transit = |target: &str, position| ApiTransit {
            map: Some(target.into()),
            position,
        };
        let position = || {
            Some(ApiPosition {
                x: 1.234,
                y: 9.0,
                z: 5.678,
            })
        };
        let transits = vec![
            transit("night-id", position()),
            transit("night-id", position()),
            transit("unknown-id", position()),
            transit("night-id", None),
        ];
        let ids = HashMap::from([("night-id".into(), "night-factory".into())]);
        let canonical = HashMap::from([
            ("factory".into(), "factory".into()),
            ("night-factory".into(), "factory".into()),
        ]);
        let mut warnings = Vec::new();

        let converted = convert_transits(transits, &ids, &canonical, &mut warnings);

        assert_eq!(
            converted,
            vec![Transit {
                position: [1.23, 5.68],
                target: "factory".into()
            }]
        );
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn unions_canonical_and_alt_map_overlay_data_with_deduplication() {
        let position = || ApiPosition {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let hazard = ApiHazard {
            hazard_type: Some("sniper".into()),
            position: Some(position()),
            outline: vec![
                ApiPosition {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                ApiPosition {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                ApiPosition {
                    x: 1.0,
                    y: 0.0,
                    z: 1.0,
                },
            ],
        };
        let overlay_map = |chance| ApiOverlayMap {
            hazards: vec![hazard.clone()],
            bosses: vec![ApiBoss {
                mob: "boss".into(),
                spawn_chance: chance,
                spawn_locations: vec![ApiBossLocation {
                    positions: vec![position()],
                }],
            }],
            transits: vec![ApiTransit {
                map: Some("woods-id".into()),
                position: Some(position()),
            }],
            switches: vec![ApiSwitch {
                name: Some("switch".into()),
                position: Some(position()),
            }],
            btr_stops: vec![ApiBtrStop {
                name: Some("stop".into()),
                x: Some(1.0),
                z: Some(3.0),
            }],
        };
        let raw_maps = HashMap::from([
            ("factory".into(), overlay_map(0.3)),
            ("night-factory".into(), overlay_map(0.5)),
        ]);
        let translations = HashMap::from([
            ("boss".into(), "Tagilla".into()),
            ("switch".into(), "Power".into()),
            ("stop".into(), "Checkpoint".into()),
        ]);
        let ids = HashMap::from([("woods-id".into(), "woods".into())]);
        let canonical = HashMap::from([
            ("factory".into(), "factory".into()),
            ("night-factory".into(), "factory".into()),
            ("woods".into(), "woods".into()),
        ]);
        let sources = vec!["factory".into(), "night-factory".into()];
        let mut warnings = Vec::new();

        let data = collect_overlay_data(
            &sources,
            &raw_maps,
            &translations,
            &ids,
            &canonical,
            &mut warnings,
        );

        assert_eq!(data.sniper_zones.len(), 1);
        assert_eq!(data.boss_spawns.len(), 1);
        assert_eq!(data.boss_spawns[0].mobs[0].chance, 0.5);
        assert_eq!(data.transits.len(), 1);
        assert_eq!(data.switches.len(), 1);
        assert_eq!(data.btr_stops.len(), 1);
        assert!(warnings.is_empty());
    }

    #[test]
    fn maps_upstream_alt_names_to_their_canonical_catalogue_map() {
        let groups: Vec<FetchedMapGroup> = serde_json::from_str(
            r#"[
                {
                    "normalizedName": "factory",
                    "maps": [{
                        "projection": "interactive",
                        "altMaps": ["night-factory"]
                    }]
                },
                {
                    "normalizedName": "unused",
                    "maps": [{"projection": "static"}]
                }
            ]"#,
        )
        .unwrap();

        let aliases = canonical_map_by_alias(&groups);

        assert_eq!(aliases.get("factory"), Some(&"factory".to_owned()));
        assert_eq!(aliases.get("night-factory"), Some(&"factory".to_owned()));
        assert!(!aliases.contains_key("unused"));
    }
}
