//! Overlay visibility settings and drawing functions for map markers.

use crate::colors;
use crate::constants::{MINEFIELD_MARKER_SIZE, MINEFIELD_MIN_SIZE};
use crate::coordinates::game_to_display;
use crate::labels::{LabelCandidate, LabelKind};
use crate::markers;
use crate::screenshot_watcher::PlayerPosition;
use eframe::egui;
use geo::{
    Buffer, Coord, InteriorPoint, LineString, MultiPoint, MultiPolygon, Orient, Point, Polygon,
    TriangulateEarcut,
    algorithm::{orient::Direction, unary_union},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tarkov_map::{BtrStop, Extract, Label, Map, Spawn, Switch};

const SWITCH_CLUSTER_UNITS: f64 = 3.0;
const BOSS_SPAWN_AREA_RADIUS: f64 = 20.0;

/// Controls visibility of different overlay types on the map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayVisibility {
    pub labels: bool,
    pub spawns: bool,
    pub pmc_extracts: bool,
    pub scav_extracts: bool,
    pub shared_extracts: bool,
    pub transits: bool,
    pub btr_stops: bool,
    pub switches: bool,
    pub sniper_zones: bool,
    pub minefields: bool,
    pub mobs: BTreeSet<String>,
    pub player_marker: bool,
}
/// One toggleable Overlay offered in the sidebar.
#[derive(Clone, Copy)]
pub struct OverlayKind {
    offered: fn(&Map) -> bool,
    visible: fn(&OverlayVisibility) -> bool,
    visibility_mut: fn(&mut OverlayVisibility) -> &mut bool,
}

impl OverlayKind {
    pub const LABELS: Self = Self {
        offered: |map| map.labels.as_ref().is_some_and(|labels| !labels.is_empty()),
        visible: |visibility| visibility.labels,
        visibility_mut: |visibility| &mut visibility.labels,
    };
    pub const PMC_SPAWNS: Self = Self {
        offered: |map| map.spawns.as_ref().is_some_and(|spawns| !spawns.is_empty()),
        visible: |visibility| visibility.spawns,
        visibility_mut: |visibility| &mut visibility.spawns,
    };
    pub const PMC_EXTRACTS: Self = Self {
        offered: |map| has_positioned_extract(map, "pmc"),
        visible: |visibility| visibility.pmc_extracts,
        visibility_mut: |visibility| &mut visibility.pmc_extracts,
    };
    pub const SCAV_EXTRACTS: Self = Self {
        offered: |map| has_positioned_extract(map, "scav"),
        visible: |visibility| visibility.scav_extracts,
        visibility_mut: |visibility| &mut visibility.scav_extracts,
    };
    pub const SHARED_EXTRACTS: Self = Self {
        offered: |map| has_positioned_extract(map, "shared"),
        visible: |visibility| visibility.shared_extracts,
        visibility_mut: |visibility| &mut visibility.shared_extracts,
    };
    pub const TRANSITS: Self = Self {
        offered: |map| !map.transits.is_empty(),
        visible: |visibility| visibility.transits,
        visibility_mut: |visibility| &mut visibility.transits,
    };
    pub const BTR_STOPS: Self = Self {
        offered: |map| !map.btr_stops.is_empty(),
        visible: |visibility| visibility.btr_stops,
        visibility_mut: |visibility| &mut visibility.btr_stops,
    };
    pub const SWITCHES: Self = Self {
        offered: |map| !map.switches.is_empty(),
        visible: |visibility| visibility.switches,
        visibility_mut: |visibility| &mut visibility.switches,
    };
    pub const SNIPER_ZONES: Self = Self {
        offered: |map| !map.sniper_zones.is_empty(),
        visible: |visibility| visibility.sniper_zones,
        visibility_mut: |visibility| &mut visibility.sniper_zones,
    };
    pub const MINEFIELDS: Self = Self {
        offered: |map| !map.minefields.is_empty(),
        visible: |visibility| visibility.minefields,
        visibility_mut: |visibility| &mut visibility.minefields,
    };
    pub const PLAYER_MARKER: Self = Self {
        offered: |_| true,
        visible: |visibility| visibility.player_marker,
        visibility_mut: |visibility| &mut visibility.player_marker,
    };

    pub fn visibility_mut(self, visibility: &mut OverlayVisibility) -> &mut bool {
        (self.visibility_mut)(visibility)
    }
}

/// Whether an Overlay has at least one item to draw on this Map.
pub fn overlay_offered(overlay: OverlayKind, map: &Map) -> bool {
    (overlay.offered)(map)
}

/// Distinct Mob names offered by a Map, in sidebar order.
pub fn offered_mobs(map: &Map) -> Vec<&str> {
    map.boss_spawns
        .iter()
        .flat_map(|spawn| spawn.mobs.iter().map(|mob| mob.name.as_str()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn has_positioned_extract(map: &Map, faction: &str) -> bool {
    map.extracts.as_ref().is_some_and(|extracts| {
        extracts.iter().any(|extract| {
            extract.position.is_some() && extract.faction.eq_ignore_ascii_case(faction)
        })
    })
}

/// The visible and offered Overlay counts for one Overlay Category.
pub fn category_count(
    overlays: impl IntoIterator<Item = OverlayKind>,
    mobs: &[&str],
    map: &Map,
    visibility: &OverlayVisibility,
) -> (usize, usize) {
    let (overlay_on, overlay_total) = overlays
        .into_iter()
        .filter(|overlay| overlay_offered(*overlay, map))
        .fold((0, 0), |(on, total), overlay| {
            (on + usize::from((overlay.visible)(visibility)), total + 1)
        });
    let shown_count = mobs
        .iter()
        .filter(|name| visibility.mobs.contains(**name))
        .count();
    (overlay_on + shown_count, overlay_total + mobs.len())
}

fn boss_spawn_label(name: &str, chance: f64) -> String {
    let percent = (chance * 100.0).round() as i64;
    if percent == 100 {
        name.to_owned()
    } else {
        format!("{name} {percent}%")
    }
}

impl Default for OverlayVisibility {
    fn default() -> Self {
        Self {
            labels: false,
            spawns: true,
            pmc_extracts: true,
            scav_extracts: true,
            shared_extracts: true,
            transits: false,
            btr_stops: false,
            switches: false,
            sniper_zones: false,
            minefields: false,
            mobs: BTreeSet::new(),
            player_marker: true,
        }
    }
}

/// Whether a projected Minefield is too small to draw as an outline.
fn minefield_uses_fallback(bounds: egui::Rect) -> bool {
    bounds.width().max(bounds.height()) < MINEFIELD_MIN_SIZE
}

fn union_overlay_areas<'a>(
    outlines: impl IntoIterator<Item = &'a [[f64; 2]]>,
) -> MultiPolygon<f64> {
    let polygons: Vec<_> = outlines
        .into_iter()
        .map(|outline| {
            Polygon::new(
                LineString::new(
                    outline
                        .iter()
                        .map(|[x, y]| Coord { x: *x, y: *y })
                        .collect(),
                ),
                Vec::new(),
            )
            .orient(Direction::Default)
        })
        .collect();
    unary_union(&polygons)
}

/// Same-type area Overlay geometry merged once in game space for one Map.
pub struct HazardGeometry {
    pub sniper_zones: MultiPolygon<f64>,
    pub minefields: MultiPolygon<f64>,
}

impl HazardGeometry {
    pub fn for_map(map: &Map) -> Self {
        Self {
            sniper_zones: union_overlay_areas(
                map.sniper_zones.iter().map(|zone| zone.outline.as_slice()),
            ),
            minefields: union_overlay_areas(
                map.minefields
                    .iter()
                    .map(|minefield| minefield.outline.as_slice()),
            ),
        }
    }
}

fn project_ring(
    map: &Map,
    map_rect: egui::Rect,
    ring: &LineString<f64>,
) -> Option<Vec<egui::Pos2>> {
    let points: Vec<_> = ring
        .points()
        .filter_map(|point| game_to_display(map, map_rect, [point.x(), point.y()]))
        .collect();
    if points.len() < 3 {
        return None;
    }

    Some(points)
}

fn project_area(
    map: &Map,
    map_rect: egui::Rect,
    area: &Polygon<f64>,
) -> Option<(Vec<egui::Pos2>, Vec<Vec<egui::Pos2>>, egui::Rect)> {
    let exterior = project_ring(map, map_rect, area.exterior())?;
    let bounds = egui::Rect::from_points(&exterior);
    if !map_rect.expand(50.0).intersects(bounds) {
        return None;
    }

    let interiors = area
        .interiors()
        .iter()
        .filter_map(|ring| project_ring(map, map_rect, ring))
        .collect();
    Some((exterior, interiors, bounds))
}

fn paint_area_fill(
    painter: &egui::Painter,
    map: &Map,
    map_rect: egui::Rect,
    area: &Polygon<f64>,
    color: egui::Color32,
) {
    let mut mesh = egui::Mesh::default();
    for triangle in area.earcut_triangles_iter() {
        let projected = [triangle.v1(), triangle.v2(), triangle.v3()]
            .map(|point| game_to_display(map, map_rect, [point.x, point.y]));
        let [Some(a), Some(b), Some(c)] = projected else {
            continue;
        };

        let first = mesh.vertices.len() as u32;
        mesh.colored_vertex(a, color);
        mesh.colored_vertex(b, color);
        mesh.colored_vertex(c, color);
        mesh.add_triangle(first, first + 1, first + 2);
    }

    if !mesh.indices.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// Draws Sniper Zones as faint red polygons with solid outlines.
pub fn draw_sniper_zones(
    ui: &mut egui::Ui,
    map_rect: egui::Rect,
    map: &Map,
    sniper_zones: &MultiPolygon<f64>,
    zoom: f32,
) {
    let painter = ui.painter();
    let stroke_width = (1.0 + 0.2 * zoom).min(2.0) * 1.5;

    for zone in &sniper_zones.0 {
        let Some((exterior, interiors, _)) = project_area(map, map_rect, zone) else {
            continue;
        };
        paint_area_fill(painter, map, map_rect, zone, colors::SNIPER_ZONE_FILL);
        for ring in std::iter::once(exterior).chain(interiors) {
            painter.add(egui::Shape::line(
                ring,
                egui::Stroke::new(stroke_width, colors::SNIPER_ZONE_STROKE),
            ));
        }
    }
}

/// Draws Minefields as faint orange polygons with dashed outlines.
///
/// Small projected outlines use a square marker so narrow strips remain visible
/// when the whole Map is fitted in the Viewport.
pub fn draw_minefields(
    ui: &mut egui::Ui,
    map_rect: egui::Rect,
    map: &Map,
    minefields: &MultiPolygon<f64>,
    zoom: f32,
) {
    let painter = ui.painter();
    let stroke_width = (1.0 + 0.2 * zoom).min(2.0) * 1.2;

    for minefield in &minefields.0 {
        let Some((exterior, interiors, bounds)) = project_area(map, map_rect, minefield) else {
            continue;
        };
        if minefield_uses_fallback(bounds) {
            let marker = egui::Rect::from_center_size(
                bounds.center(),
                egui::Vec2::splat(MINEFIELD_MARKER_SIZE),
            );
            painter.rect_filled(marker, 1.0, colors::MINEFIELD_STROKE);
            continue;
        }

        paint_area_fill(painter, map, map_rect, minefield, colors::MINEFIELD_FILL);
        for ring in std::iter::once(exterior).chain(interiors) {
            painter.add(egui::Shape::dashed_line(
                &ring,
                egui::Stroke::new(stroke_width, colors::MINEFIELD_STROKE),
                4.0,
                3.0,
            ));
        }
    }
}

/// Base font size for marker Labels at the fit view.
const MARKER_LABEL_BASE: f32 = 11.0;

/// Builds a Label font that grows with the square root of zoom, so text stays
/// readable at the fit view without ballooning when zoomed in.
fn label_font(base: f32, zoom: f32, min: f32, max: f32) -> egui::FontId {
    egui::FontId::proportional((base * zoom.sqrt()).clamp(min, max))
}

#[derive(Debug, PartialEq)]
struct SwitchCluster {
    position: [f64; 2],
    names: Vec<String>,
}

/// Greedily groups Switches around the first member of each cluster.
fn cluster_switches(switches: &[Switch]) -> Vec<SwitchCluster> {
    let mut clusters: Vec<SwitchCluster> = Vec::new();

    for switch in switches {
        if let Some(cluster) = clusters.iter_mut().find(|cluster| {
            let dx = cluster.position[0] - switch.position[0];
            let dz = cluster.position[1] - switch.position[1];
            dx * dx + dz * dz <= SWITCH_CLUSTER_UNITS * SWITCH_CLUSTER_UNITS
        }) {
            if !cluster.names.contains(&switch.name) {
                cluster.names.push(switch.name.clone());
            }
        } else {
            clusters.push(SwitchCluster {
                position: switch.position,
                names: vec![switch.name.clone()],
            });
        }
    }

    clusters
}

/// Contributes map Label candidates to the shared placement pass.
pub fn contribute_place_name_labels(
    painter: &egui::Painter,
    map_rect: egui::Rect,
    map: &Map,
    labels: &[Label],
    zoom: f32,
    candidates: &mut Vec<LabelCandidate>,
) {
    for (seq, label) in labels.iter().enumerate() {
        let Some(pos) = game_to_display(map, map_rect, label.position) else {
            continue;
        };

        let size = label.size.unwrap_or(40);
        let font = label_font(size as f32 * 0.2, zoom, 11.0, 32.0);
        let measured = painter
            .layout_no_wrap(label.text.clone(), font.clone(), colors::LABEL_TEXT)
            .size();

        candidates.push(LabelCandidate {
            kind: LabelKind::PlaceName,
            within_kind_priority: f64::from(size),
            source_order: seq,
            text: label.text.clone(),
            font,
            color: colors::LABEL_TEXT,
            outline: colors::LABEL_SHADOW,
            anchor: pos,
            align: egui::Align2::CENTER_CENTER,
            measured,
        });
    }
}

/// Draws spawn point markers on the map.
pub fn draw_spawns(
    ui: &mut egui::Ui,
    map_rect: egui::Rect,
    map: &Map,
    spawns: &[Spawn],
    zoom: f32,
) {
    let painter = ui.painter();

    for spawn in spawns {
        // Use x, z for 2D position (y is height)
        let game_pos = [spawn.position[0], spawn.position[2]];
        let Some(pos) = game_to_display(map, map_rect, game_pos) else {
            continue;
        };

        if !map_rect.expand(20.0).contains(pos) {
            continue;
        }

        let radius = (4.0 * zoom).clamp(3.0, 12.0);
        painter.circle(
            pos,
            radius,
            colors::SPAWN_FILL,
            egui::Stroke::new(1.5_f32, colors::SPAWN_STROKE),
        );
    }
}

/// One shown Extract projected and styled for this frame.
pub struct ExtractMarker<'a> {
    name: &'a str,
    source_order: usize,
    position: egui::Pos2,
    size: f32,
    label_font: egui::FontId,
    fill_color: egui::Color32,
    stroke_color: egui::Color32,
}

/// Builds the shown Extract presentation shared by marker and Label drawing.
pub fn extract_markers<'a>(
    map_rect: egui::Rect,
    map: &Map,
    extracts: &'a [Extract],
    zoom: f32,
    overlays: &OverlayVisibility,
) -> Vec<ExtractMarker<'a>> {
    extracts
        .iter()
        .enumerate()
        .filter_map(|(source_order, extract)| {
            let (fill_color, stroke_color) = extract_colors(extract, overlays)?;
            let position = extract.position?;
            let position = game_to_display(map, map_rect, [position[0], position[2]])?;
            let size = (12.0 * zoom).clamp(8.0, 32.0);

            Some(ExtractMarker {
                name: &extract.name,
                source_order,
                position,
                size,
                label_font: label_font(MARKER_LABEL_BASE, zoom, 11.0, 20.0),
                fill_color,
                stroke_color,
            })
        })
        .collect()
}

/// Draws extraction point markers on the map.
pub fn draw_extracts(ui: &mut egui::Ui, map_rect: egui::Rect, extracts: &[ExtractMarker<'_>]) {
    let painter = ui.painter();

    for extract in extracts {
        if !map_rect.expand(20.0).contains(extract.position) {
            continue;
        }

        let rect =
            egui::Rect::from_center_size(extract.position, egui::vec2(extract.size, extract.size));
        painter.rect_filled(rect, 2.0, extract.fill_color);
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(2.0_f32, extract.stroke_color),
            egui::StrokeKind::Outside,
        );
    }
}

/// Contributes visible Extract Label candidates to the shared placement pass.
pub fn contribute_extract_labels(
    painter: &egui::Painter,
    extracts: &[ExtractMarker<'_>],
    candidates: &mut Vec<LabelCandidate>,
) {
    for extract in extracts {
        let measured = painter
            .layout_no_wrap(
                extract.name.to_owned(),
                extract.label_font.clone(),
                egui::Color32::WHITE,
            )
            .size();

        candidates.push(LabelCandidate {
            kind: LabelKind::Extract,
            within_kind_priority: 0.0,
            source_order: extract.source_order,
            text: extract.name.to_owned(),
            font: extract.label_font.clone(),
            color: egui::Color32::WHITE,
            outline: colors::EXTRACT_TEXT_SHADOW,
            anchor: extract.position + egui::vec2(0.0, -extract.size / 2.0 - 4.0),
            align: egui::Align2::CENTER_BOTTOM,
            measured,
        });
    }
}

/// One shown Transit projected and styled for this frame.
pub struct TransitMarker {
    label: String,
    source_order: usize,
    position: egui::Pos2,
    size: f32,
    label_font: egui::FontId,
}

/// Builds the Transit presentation shared by marker and Label drawing.
pub fn transit_markers(
    map_rect: egui::Rect,
    map: &Map,
    maps: &[Map],
    zoom: f32,
) -> Vec<TransitMarker> {
    map.transits
        .iter()
        .enumerate()
        .filter_map(|(source_order, transit)| {
            let target = maps
                .iter()
                .find(|candidate| candidate.normalized_name == transit.target)?;
            let position = game_to_display(map, map_rect, transit.position)?;
            let size = (12.0 * zoom).clamp(8.0, 32.0);

            Some(TransitMarker {
                label: target.name.clone(),
                source_order,
                position,
                size,
                label_font: label_font(MARKER_LABEL_BASE, zoom, 11.0, 20.0),
            })
        })
        .collect()
}

/// Draws Transit markers as solid chevrons.
pub fn draw_transits(ui: &mut egui::Ui, map_rect: egui::Rect, transits: &[TransitMarker]) {
    let painter = ui.painter();

    for transit in transits {
        if !map_rect.expand(20.0).contains(transit.position) {
            continue;
        }

        markers::transit(painter, transit.position, transit.size);
    }
}

/// Contributes visible Transit Label candidates to the shared placement pass.
pub fn contribute_transit_labels(
    painter: &egui::Painter,
    transits: &[TransitMarker],
    candidates: &mut Vec<LabelCandidate>,
) {
    for transit in transits {
        let measured = painter
            .layout_no_wrap(
                transit.label.clone(),
                transit.label_font.clone(),
                colors::TRANSIT,
            )
            .size();

        candidates.push(LabelCandidate {
            kind: LabelKind::Transit,
            within_kind_priority: 0.0,
            source_order: transit.source_order,
            text: transit.label.clone(),
            font: transit.label_font.clone(),
            color: colors::TRANSIT,
            outline: colors::EXTRACT_TEXT_SHADOW,
            anchor: transit.position + egui::vec2(0.0, -transit.size / 2.0 - 4.0),
            align: egui::Align2::CENTER_BOTTOM,
            measured,
        });
    }
}

/// One shown BTR Stop projected and styled for this frame.
pub struct BtrStopMarker<'a> {
    name: &'a str,
    source_order: usize,
    position: egui::Pos2,
    size: f32,
    label_font: egui::FontId,
}

/// Builds the BTR Stop presentation shared by marker and Label drawing.
pub fn btr_stop_markers<'a>(
    map_rect: egui::Rect,
    map: &Map,
    stops: &'a [BtrStop],
    zoom: f32,
) -> Vec<BtrStopMarker<'a>> {
    stops
        .iter()
        .enumerate()
        .filter_map(|(source_order, stop)| {
            let position = game_to_display(map, map_rect, stop.position)?;
            let size = (12.0 * zoom).clamp(8.0, 32.0);

            Some(BtrStopMarker {
                name: &stop.name,
                source_order,
                position,
                size,
                label_font: label_font(MARKER_LABEL_BASE, zoom, 11.0, 20.0),
            })
        })
        .collect()
}

/// Draws BTR Stop markers as solid stop-sign octagons.
pub fn draw_btr_stops(ui: &mut egui::Ui, map_rect: egui::Rect, stops: &[BtrStopMarker<'_>]) {
    let painter = ui.painter();

    for stop in stops {
        if !map_rect.expand(20.0).contains(stop.position) {
            continue;
        }

        markers::btr_stop(painter, stop.position, stop.size);
    }
}

/// Contributes visible BTR Stop Label candidates to the shared placement pass.
pub fn contribute_btr_stop_labels(
    painter: &egui::Painter,
    stops: &[BtrStopMarker<'_>],
    candidates: &mut Vec<LabelCandidate>,
) {
    for stop in stops {
        let measured = painter
            .layout_no_wrap(
                stop.name.to_owned(),
                stop.label_font.clone(),
                colors::BTR_STOP,
            )
            .size();

        candidates.push(LabelCandidate {
            kind: LabelKind::BtrStop,
            within_kind_priority: 0.0,
            source_order: stop.source_order,
            text: stop.name.to_owned(),
            font: stop.label_font.clone(),
            color: colors::BTR_STOP,
            outline: colors::EXTRACT_TEXT_SHADOW,
            anchor: stop.position + egui::vec2(0.0, -stop.size / 2.0 - 4.0),
            align: egui::Align2::CENTER_BOTTOM,
            measured,
        });
    }
}

struct MobSpawnPoints {
    chance: f64,
    source_order: usize,
    points: Vec<Point<f64>>,
}

/// One inferred Boss Spawn Area projected and styled for this frame.
pub struct BossSpawnArea {
    polygon: Polygon<f64>,
    label: String,
    source_order: usize,
    label_position: egui::Pos2,
    label_font: egui::FontId,
    rank: f64,
}

/// Builds one conservative area per connected cluster of shown Mob spawn positions.
pub fn boss_spawn_areas(
    map_rect: egui::Rect,
    map: &Map,
    zoom: f32,
    shown_mobs: &BTreeSet<String>,
) -> Vec<BossSpawnArea> {
    let mut by_mob = BTreeMap::<String, MobSpawnPoints>::new();
    for (source_order, spawn) in map.boss_spawns.iter().enumerate() {
        for mob in &spawn.mobs {
            if !shown_mobs.contains(&mob.name) {
                continue;
            }

            let group = by_mob
                .entry(mob.name.clone())
                .or_insert_with(|| MobSpawnPoints {
                    chance: mob.chance,
                    source_order,
                    points: Vec::new(),
                });
            group
                .points
                .push(Point::new(spawn.position[0], spawn.position[1]));
        }
    }

    let mut areas: Vec<BossSpawnArea> = Vec::new();
    for (name, group) in by_mob {
        let label = boss_spawn_label(&name, group.chance);
        let geometry = MultiPoint::new(group.points).buffer(BOSS_SPAWN_AREA_RADIUS);

        for polygon in geometry.0 {
            let Some(anchor) = polygon.interior_point() else {
                continue;
            };
            let Some(label_position) = game_to_display(map, map_rect, [anchor.x(), anchor.y()])
            else {
                continue;
            };

            if let Some(existing) = areas.iter_mut().find(|area| area.polygon == polygon) {
                existing.label.push('\n');
                existing.label.push_str(&label);
                existing.rank = existing.rank.max(group.chance);
                existing.source_order = existing.source_order.min(group.source_order);
                continue;
            }

            areas.push(BossSpawnArea {
                polygon,
                label: label.clone(),
                source_order: group.source_order,
                label_position,
                label_font: label_font(MARKER_LABEL_BASE, zoom, 11.0, 20.0),
                rank: group.chance,
            });
        }
    }

    areas
}

/// Draws inferred Boss Spawn Areas as faint fills with dashed outlines.
pub fn draw_boss_spawn_areas(
    ui: &mut egui::Ui,
    map_rect: egui::Rect,
    map: &Map,
    areas: &[BossSpawnArea],
    zoom: f32,
) {
    let painter = ui.painter();
    let stroke_width = (1.0 + 0.2 * zoom).min(2.0) * 1.2;

    for area in areas {
        let Some((exterior, interiors, _)) = project_area(map, map_rect, &area.polygon) else {
            continue;
        };

        paint_area_fill(
            painter,
            map,
            map_rect,
            &area.polygon,
            colors::BOSS_SPAWN_AREA_FILL,
        );
        for ring in std::iter::once(exterior).chain(interiors) {
            painter.add(egui::Shape::dashed_line(
                &ring,
                egui::Stroke::new(stroke_width, colors::BOSS_SPAWN_AREA_STROKE),
                6.0,
                4.0,
            ));
        }
    }
}

/// Contributes one title Label for each inferred Boss Spawn Area.
pub fn contribute_boss_spawn_area_labels(
    painter: &egui::Painter,
    areas: &[BossSpawnArea],
    candidates: &mut Vec<LabelCandidate>,
) {
    for area in areas {
        let measured = painter
            .layout_no_wrap(
                area.label.clone(),
                area.label_font.clone(),
                egui::Color32::WHITE,
            )
            .size();

        candidates.push(LabelCandidate {
            kind: LabelKind::BossSpawn,
            within_kind_priority: area.rank,
            source_order: area.source_order,
            text: area.label.clone(),
            font: area.label_font.clone(),
            color: egui::Color32::WHITE,
            outline: colors::EXTRACT_TEXT_SHADOW,
            anchor: area.label_position,
            align: egui::Align2::CENTER_CENTER,
            measured,
        });
    }
}

/// One clustered Switch marker projected and styled for this frame.
pub struct SwitchMarker {
    label: String,
    source_order: usize,
    position: egui::Pos2,
    size: f32,
    label_font: egui::FontId,
    stack_size: usize,
}

/// Builds clustered Switch presentation shared by marker and Label drawing.
pub fn switch_markers(map_rect: egui::Rect, map: &Map, zoom: f32) -> Vec<SwitchMarker> {
    cluster_switches(&map.switches)
        .into_iter()
        .enumerate()
        .filter_map(|(source_order, cluster)| {
            let position = game_to_display(map, map_rect, cluster.position)?;
            let stack_size = cluster.names.len();
            let size = (12.0 * zoom).clamp(8.0, 32.0);

            Some(SwitchMarker {
                label: cluster.names.join("\n"),
                source_order,
                position,
                size,
                label_font: label_font(MARKER_LABEL_BASE, zoom, 11.0, 20.0),
                stack_size,
            })
        })
        .collect()
}

/// Draws Switch markers as solid lightning bolts.
pub fn draw_switches(ui: &mut egui::Ui, map_rect: egui::Rect, switches: &[SwitchMarker]) {
    let painter = ui.painter();

    for switch in switches {
        if !map_rect.expand(20.0).contains(switch.position) {
            continue;
        }

        markers::switch(painter, switch.position, switch.size);
    }
}

/// Contributes clustered Switch Labels to the shared placement pass.
pub fn contribute_switch_labels(
    painter: &egui::Painter,
    switches: &[SwitchMarker],
    candidates: &mut Vec<LabelCandidate>,
) {
    for switch in switches {
        let measured = painter
            .layout_no_wrap(
                switch.label.clone(),
                switch.label_font.clone(),
                colors::SWITCH,
            )
            .size();

        candidates.push(LabelCandidate {
            kind: LabelKind::Switch,
            within_kind_priority: switch.stack_size as f64,
            source_order: switch.source_order,
            text: switch.label.clone(),
            font: switch.label_font.clone(),
            color: colors::SWITCH,
            outline: colors::EXTRACT_TEXT_SHADOW,
            anchor: switch.position + egui::vec2(0.0, -switch.size / 2.0 - 4.0),
            align: egui::Align2::CENTER_BOTTOM,
            measured,
        });
    }
}

fn extract_colors(
    extract: &Extract,
    overlays: &OverlayVisibility,
) -> Option<(egui::Color32, egui::Color32)> {
    if extract.faction.eq_ignore_ascii_case("pmc") && overlays.pmc_extracts {
        Some((colors::PMC_EXTRACT_FILL, colors::PMC_EXTRACT_STROKE))
    } else if extract.faction.eq_ignore_ascii_case("scav") && overlays.scav_extracts {
        Some((colors::SCAV_EXTRACT_FILL, colors::SCAV_EXTRACT_STROKE))
    } else if extract.faction.eq_ignore_ascii_case("shared") && overlays.shared_extracts {
        Some((colors::SHARED_EXTRACT_FILL, colors::SHARED_EXTRACT_STROKE))
    } else {
        None
    }
}

/// Draws the player position marker as a circle with a directional triangle on the map.
pub fn draw_player_marker(
    ui: &mut egui::Ui,
    map_rect: egui::Rect,
    map: &Map,
    player: &PlayerPosition,
    zoom: f32,
) {
    // Use x, z for 2D position (y is height in Tarkov)
    let game_pos = [player.position[0], player.position[2]];
    let Some(pos) = game_to_display(map, map_rect, game_pos) else {
        return;
    };

    // Don't draw if outside the visible map area
    if !map_rect.expand(50.0).contains(pos) {
        return;
    }

    let painter = ui.painter();

    // Sizes scale with zoom
    let circle_radius = (8.0 * zoom).clamp(6.0, 16.0);
    let triangle_size = (8.0 * zoom).clamp(5.0, 14.0);
    let triangle_offset = circle_radius + triangle_size * 0.6; // Distance from center to triangle

    // The yaw from the screenshot represents the player's facing direction.
    // We need to adjust for the map's coordinate rotation to display correctly.
    let coord_rotation = map.coordinate_rotation.unwrap_or(0.0) as f32;
    let adjusted_yaw = player.yaw - coord_rotation.to_radians();

    // Draw the circle at player position
    painter.circle(
        pos,
        circle_radius,
        colors::PLAYER_MARKER_FILL,
        egui::Stroke::new(2.0_f32, colors::PLAYER_MARKER_STROKE),
    );

    // Calculate triangle center position (outside the circle, in direction of yaw)
    let triangle_center = pos
        + egui::vec2(
            adjusted_yaw.sin() * triangle_offset,
            -adjusted_yaw.cos() * triangle_offset,
        );

    // Create triangle points (pointing outward from circle)
    // The tip points away from the circle center
    let tip = egui::vec2(0.0, -triangle_size);
    let back_left = egui::vec2(-triangle_size * 0.6, triangle_size * 0.4);
    let back_right = egui::vec2(triangle_size * 0.6, triangle_size * 0.4);

    // Rotate each point by the adjusted yaw
    let rotate = |v: egui::Vec2| -> egui::Pos2 {
        let cos = adjusted_yaw.cos();
        let sin = adjusted_yaw.sin();
        triangle_center + egui::vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
    };

    let points = vec![rotate(tip), rotate(back_left), rotate(back_right)];

    // Draw filled triangle with stroke
    painter.add(egui::Shape::convex_polygon(
        points,
        colors::PLAYER_MARKER_FILL,
        egui::Stroke::new(1.5_f32, colors::PLAYER_MARKER_STROKE),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Area;
    use tarkov_map::{BossChance, BossSpawn, BtrStop, Minefield, SniperZone, Switch, Transit};

    const MAP_OVERLAYS: [OverlayKind; 2] = [OverlayKind::LABELS, OverlayKind::PLAYER_MARKER];
    const HAZARD_OVERLAYS: [OverlayKind; 2] = [OverlayKind::SNIPER_ZONES, OverlayKind::MINEFIELDS];
    const EXTRACT_OVERLAYS: [OverlayKind; 3] = [
        OverlayKind::PMC_EXTRACTS,
        OverlayKind::SCAV_EXTRACTS,
        OverlayKind::SHARED_EXTRACTS,
    ];
    const NAVIGATION_OVERLAYS: [OverlayKind; 3] = [
        OverlayKind::TRANSITS,
        OverlayKind::BTR_STOPS,
        OverlayKind::SWITCHES,
    ];

    fn empty_map() -> Map {
        ron::from_str(
            r#"Map(
                normalizedName: "test",
                name: "Test",
                imagePath: "maps/test.bc7z",
                imageSize: (256.0, 256.0),
                logicalSize: (200.0, 200.0),
            )"#,
        )
        .expect("test Map should parse")
    }

    fn map_with_current_overlays() -> Map {
        let mut map = empty_map();
        map.labels = Some(vec![Label {
            position: [0.0, 0.0],
            text: "Dorms".to_owned(),
            rotation: None,
            size: None,
            top: None,
            bottom: None,
        }]);
        map.spawns = Some(vec![Spawn {
            position: [0.0, 0.0, 0.0],
            sides: vec!["pmc".to_owned()],
            categories: vec!["player".to_owned()],
        }]);
        map.extracts = Some(vec![
            Extract {
                name: "PMC".to_owned(),
                faction: "pmc".to_owned(),
                position: Some([0.0, 0.0, 0.0]),
            },
            Extract {
                name: "Scav without a position".to_owned(),
                faction: "scav".to_owned(),
                position: None,
            },
            Extract {
                name: "Shared".to_owned(),
                faction: "shared".to_owned(),
                position: Some([0.0, 0.0, 0.0]),
            },
        ]);
        map
    }

    fn map_with_hazards() -> Map {
        let mut map = empty_map();
        map.sniper_zones = vec![SniperZone {
            outline: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
        }];
        map.minefields = vec![Minefield {
            outline: vec![[20.0, 20.0], [30.0, 20.0], [30.0, 30.0]],
        }];
        map
    }

    #[test]
    fn mob_rows_are_distinct_and_alphabetical() {
        let mut map = empty_map();
        map.boss_spawns = vec![
            BossSpawn {
                position: [10.0, 20.0],
                mobs: vec![
                    BossChance {
                        name: "Raider".to_owned(),
                        chance: 0.4,
                    },
                    BossChance {
                        name: "Glukhar".to_owned(),
                        chance: 0.3,
                    },
                ],
            },
            BossSpawn {
                position: [30.0, 40.0],
                mobs: vec![BossChance {
                    name: "Raider".to_owned(),
                    chance: 0.4,
                }],
            },
        ];

        assert_eq!(offered_mobs(&map), ["Glukhar", "Raider"]);
    }

    #[test]
    fn mob_visibility_round_trips_with_stale_names_and_old_settings_default_empty() {
        let visibility = OverlayVisibility {
            mobs: BTreeSet::from(["Cultist Priest".to_owned(), "Renamed Mob".to_owned()]),
            ..OverlayVisibility::default()
        };

        let saved = serde_json::to_string(&visibility).expect("visibility should serialize");
        let restored: OverlayVisibility =
            serde_json::from_str(&saved).expect("visibility should deserialize");
        let old_settings: OverlayVisibility =
            serde_json::from_str(r#"{"spawns":false}"#).expect("old settings should deserialize");

        assert_eq!(restored.mobs, visibility.mobs);
        assert!(old_settings.mobs.is_empty());
        assert!(!old_settings.spawns);
    }

    #[test]
    fn category_count_combines_pmc_spawns_with_offered_mobs() {
        let mut map = empty_map();
        map.spawns = Some(vec![Spawn {
            position: [0.0, 0.0, 0.0],
            sides: vec!["pmc".to_owned()],
            categories: vec!["player".to_owned()],
        }]);
        map.boss_spawns = vec![BossSpawn {
            position: [10.0, 20.0],
            mobs: vec![
                BossChance {
                    name: "Glukhar".to_owned(),
                    chance: 0.3,
                },
                BossChance {
                    name: "Raider".to_owned(),
                    chance: 0.4,
                },
            ],
        }];
        let mobs = offered_mobs(&map);

        let mut visibility = OverlayVisibility::default();
        assert_eq!(
            category_count([OverlayKind::PMC_SPAWNS], &mobs, &map, &visibility),
            (1, 3)
        );

        visibility.spawns = false;
        visibility.mobs.insert("Raider".to_owned());
        visibility.mobs.insert("Stale name".to_owned());
        assert_eq!(
            category_count([OverlayKind::PMC_SPAWNS], &mobs, &map, &visibility),
            (1, 3)
        );
    }

    #[test]
    fn boss_spawn_label_formats_chance() {
        assert_eq!(boss_spawn_label("AF", 1.0), "AF");
        assert_eq!(boss_spawn_label("Reshala", 0.452), "Reshala 45%");
        assert_eq!(
            boss_spawn_label("Cultist Priest", 0.025),
            "Cultist Priest 3%"
        );
    }

    #[test]
    fn nearby_boss_spawns_form_one_area_while_distant_spawns_stay_separate() {
        let mut map = empty_map();
        map.bounds = Some([[200.0, -100.0], [-100.0, 100.0]]);
        map.boss_spawns = [0.0, 35.0, 100.0]
            .into_iter()
            .map(|x| BossSpawn {
                position: [x, 0.0],
                mobs: vec![BossChance {
                    name: "Tagilla".to_owned(),
                    chance: 0.25,
                }],
            })
            .collect();
        let map_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 200.0));
        let shown = BTreeSet::from(["Tagilla".to_owned()]);

        let areas = boss_spawn_areas(map_rect, &map, 1.0, &shown);

        assert_eq!(areas.len(), 2);
        assert!(areas.iter().all(|area| area.label == "Tagilla 25%"));
    }

    #[test]
    fn mobs_with_the_same_area_share_one_stacked_title() {
        let mut map = empty_map();
        map.bounds = Some([[100.0, -100.0], [-100.0, 100.0]]);
        map.boss_spawns = vec![BossSpawn {
            position: [0.0, 0.0],
            mobs: vec![
                BossChance {
                    name: "Tagilla".to_owned(),
                    chance: 0.25,
                },
                BossChance {
                    name: "Killa".to_owned(),
                    chance: 0.45,
                },
            ],
        }];
        let map_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 200.0));
        let shown = BTreeSet::from(["Killa".to_owned(), "Tagilla".to_owned()]);

        let areas = boss_spawn_areas(map_rect, &map, 1.0, &shown);

        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0].label, "Killa 45%\nTagilla 25%");
    }

    #[test]
    fn interchange_tagilla_spawns_form_one_area() {
        let maps: Vec<Map> = ron::from_str(include_str!("../../../assets/maps.ron"))
            .expect("bundled Maps should parse");
        let interchange = maps
            .iter()
            .find(|map| map.normalized_name == "interchange")
            .expect("Interchange should be bundled");
        let map_rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(interchange.logical_size[0], interchange.logical_size[1]),
        );
        let shown = BTreeSet::from(["Tagilla".to_owned()]);

        let areas = boss_spawn_areas(map_rect, interchange, 1.0, &shown);

        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0].label, "Tagilla 25%");
    }

    #[test]
    fn customs_reshala_visibility_builds_fewer_areas_than_source_positions() {
        let maps: Vec<Map> = ron::from_str(include_str!("../../../assets/maps.ron"))
            .expect("bundled Maps should parse");
        let customs = maps
            .iter()
            .find(|map| map.normalized_name == "customs")
            .expect("Customs should be bundled");
        let map_rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(customs.logical_size[0], customs.logical_size[1]),
        );
        let shown = BTreeSet::from(["Reshala".to_owned()]);

        let areas = boss_spawn_areas(map_rect, customs, 1.0, &shown);

        assert!(!areas.is_empty());
        assert!(areas.len() < customs.boss_spawns.len());
        assert!(areas.iter().all(|area| area.label == "Reshala 45%"));
        assert!(boss_spawn_areas(map_rect, customs, 1.0, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn overlays_are_offered_only_when_the_map_has_something_to_draw() {
        let empty = empty_map();

        assert!(!overlay_offered(OverlayKind::LABELS, &empty));
        assert!(!overlay_offered(OverlayKind::PMC_SPAWNS, &empty));
        assert!(!overlay_offered(OverlayKind::PMC_EXTRACTS, &empty));
        assert!(!overlay_offered(OverlayKind::SCAV_EXTRACTS, &empty));
        assert!(!overlay_offered(OverlayKind::SHARED_EXTRACTS, &empty));
        assert!(overlay_offered(OverlayKind::PLAYER_MARKER, &empty));

        let offered = map_with_current_overlays();

        assert!(overlay_offered(OverlayKind::LABELS, &offered));
        assert!(overlay_offered(OverlayKind::PMC_SPAWNS, &offered));
        assert!(overlay_offered(OverlayKind::PMC_EXTRACTS, &offered));
        assert!(!overlay_offered(OverlayKind::SCAV_EXTRACTS, &offered));
        assert!(overlay_offered(OverlayKind::SHARED_EXTRACTS, &offered));
    }

    #[test]
    fn hazard_overlays_are_offered_only_for_non_empty_map_collections() {
        let empty = empty_map();

        assert!(!overlay_offered(OverlayKind::SNIPER_ZONES, &empty));
        assert!(!overlay_offered(OverlayKind::MINEFIELDS, &empty));

        let offered = map_with_hazards();
        assert!(overlay_offered(OverlayKind::SNIPER_ZONES, &offered));
        assert!(overlay_offered(OverlayKind::MINEFIELDS, &offered));
        assert_eq!(
            category_count(
                HAZARD_OVERLAYS,
                &[],
                &offered,
                &OverlayVisibility::default()
            ),
            (0, 2)
        );
    }

    #[test]
    fn transits_are_offered_only_for_non_empty_map_collections_and_default_off() {
        let empty = empty_map();
        let visibility = OverlayVisibility::default();

        assert!(!visibility.transits);
        assert!(!overlay_offered(OverlayKind::TRANSITS, &empty));
        assert_eq!(
            category_count(NAVIGATION_OVERLAYS, &[], &empty, &visibility),
            (0, 0)
        );

        let mut offered = empty_map();
        offered.transits = vec![Transit {
            position: [10.0, 20.0],
            target: "woods".to_owned(),
        }];

        assert!(overlay_offered(OverlayKind::TRANSITS, &offered));
        assert_eq!(
            category_count(NAVIGATION_OVERLAYS, &[], &offered, &visibility),
            (0, 1)
        );
    }

    #[test]
    fn switches_are_offered_only_for_non_empty_map_collections_and_default_off() {
        let empty = empty_map();
        let visibility = OverlayVisibility::default();

        assert!(!visibility.switches);
        assert!(!overlay_offered(OverlayKind::SWITCHES, &empty));
        assert_eq!(
            category_count(NAVIGATION_OVERLAYS, &[], &empty, &visibility),
            (0, 0)
        );

        let mut offered = empty_map();
        offered.switches = vec![Switch {
            position: [10.0, 20.0],
            name: "Power".to_owned(),
        }];

        assert!(overlay_offered(OverlayKind::SWITCHES, &offered));
        assert_eq!(
            category_count(NAVIGATION_OVERLAYS, &[], &offered, &visibility),
            (0, 1)
        );
    }

    #[test]
    fn bundled_switches_are_offered_on_the_six_expected_maps() {
        let maps: Vec<Map> = ron::from_str(include_str!("../../../assets/maps.ron"))
            .expect("bundled Maps should parse");
        let offered: Vec<_> = maps
            .iter()
            .filter(|map| overlay_offered(OverlayKind::SWITCHES, map))
            .map(|map| map.normalized_name.as_str())
            .collect();

        assert_eq!(
            offered,
            [
                "customs",
                "interchange",
                "the-lab",
                "the-labyrinth",
                "lighthouse",
                "reserve",
            ]
        );
    }

    #[test]
    fn switch_visibility_round_trips_and_old_settings_default_off() {
        let visibility = OverlayVisibility {
            switches: true,
            ..OverlayVisibility::default()
        };

        let saved = serde_json::to_string(&visibility).expect("visibility should serialize");
        let restored: OverlayVisibility =
            serde_json::from_str(&saved).expect("visibility should deserialize");
        let old_settings: OverlayVisibility =
            serde_json::from_str(r#"{"transits":true}"#).expect("old settings should deserialize");

        assert!(restored.switches);
        assert!(!old_settings.switches);
        assert!(old_settings.transits);
    }

    #[test]
    fn btr_stops_are_offered_only_for_non_empty_map_collections_and_default_off() {
        let empty = empty_map();
        let visibility = OverlayVisibility::default();

        assert!(!visibility.btr_stops);
        assert!(!overlay_offered(OverlayKind::BTR_STOPS, &empty));

        let mut offered = empty_map();
        offered.btr_stops = vec![BtrStop {
            position: [10.0, 20.0],
            name: "USEC Checkpoint".to_owned(),
        }];

        assert!(overlay_offered(OverlayKind::BTR_STOPS, &offered));
        assert_eq!(
            category_count(NAVIGATION_OVERLAYS, &[], &offered, &visibility),
            (0, 1)
        );
    }

    #[test]
    fn bundled_btr_stops_are_offered_on_woods_and_streets_only() {
        let maps: Vec<Map> = ron::from_str(include_str!("../../../assets/maps.ron"))
            .expect("bundled Maps should parse");
        let offered: Vec<_> = maps
            .iter()
            .filter(|map| overlay_offered(OverlayKind::BTR_STOPS, map))
            .map(|map| map.normalized_name.as_str())
            .collect();

        assert_eq!(offered, ["streets-of-tarkov", "woods"]);
        let map = |normalized_name: &str| {
            maps.iter()
                .find(|map| map.normalized_name == normalized_name)
                .unwrap_or_else(|| panic!("missing bundled Map {normalized_name}"))
        };
        let visibility = OverlayVisibility::default();
        assert_eq!(
            category_count(NAVIGATION_OVERLAYS, &[], map("woods"), &visibility),
            (0, 2)
        );
    }

    #[test]
    fn btr_stop_visibility_round_trips_and_old_settings_default_off() {
        let visibility = OverlayVisibility {
            btr_stops: true,
            ..OverlayVisibility::default()
        };

        let saved = serde_json::to_string(&visibility).expect("visibility should serialize");
        let restored: OverlayVisibility =
            serde_json::from_str(&saved).expect("visibility should deserialize");
        let old_settings: OverlayVisibility =
            serde_json::from_str(r#"{"transits":true}"#).expect("old settings should deserialize");

        assert!(restored.btr_stops);
        assert!(!old_settings.btr_stops);
        assert!(old_settings.transits);
    }

    #[test]
    fn switch_clustering_uses_three_game_units_and_the_first_members_position() {
        let switches = vec![
            Switch {
                position: [10.0, 20.0],
                name: "First".to_owned(),
            },
            Switch {
                position: [13.0, 20.0],
                name: "At boundary".to_owned(),
            },
            Switch {
                position: [13.01, 20.0],
                name: "Outside boundary".to_owned(),
            },
        ];

        let clusters = cluster_switches(&switches);

        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].position, [10.0, 20.0]);
        assert_eq!(clusters[0].names, ["First", "At boundary"]);
        assert_eq!(clusters[1].position, [13.01, 20.0]);
        assert_eq!(clusters[1].names, ["Outside boundary"]);
    }

    #[test]
    fn switch_clustering_is_greedy_and_deduplicates_names_without_reordering() {
        let switches = vec![
            Switch {
                position: [0.0, 0.0],
                name: "First".to_owned(),
            },
            Switch {
                position: [5.0, 0.0],
                name: "Second cluster".to_owned(),
            },
            Switch {
                position: [3.0, 0.0],
                name: "Joins first cluster".to_owned(),
            },
            Switch {
                position: [0.5, 0.0],
                name: "First".to_owned(),
            },
            Switch {
                position: [1.0, 0.0],
                name: "Last".to_owned(),
            },
        ];

        let clusters = cluster_switches(&switches);

        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].names, ["First", "Joins first cluster", "Last"]);
        assert_eq!(clusters[1].names, ["Second cluster"]);
    }

    #[test]
    fn bundled_switches_cluster_as_expected() {
        let maps: Vec<Map> = ron::from_str(include_str!("../../../assets/maps.ron"))
            .expect("bundled Maps should parse");
        let map = |normalized_name: &str| {
            maps.iter()
                .find(|map| map.normalized_name == normalized_name)
                .unwrap_or_else(|| panic!("missing bundled Map {normalized_name}"))
        };

        let lab = map("the-lab");
        assert_eq!(lab.switches.len(), 15);
        assert!(cluster_switches(&lab.switches).len() <= 12);

        let lighthouse = cluster_switches(&map("lighthouse").switches);
        assert_eq!(lighthouse.len(), 1);
        assert_eq!(lighthouse[0].names, ["Lightkeeper Switch"]);

        let reserve = cluster_switches(&map("reserve").switches);
        let d2_power = reserve
            .iter()
            .position(|cluster| cluster.names.iter().any(|name| name == "D-2 Power Switch"))
            .expect("Reserve should include the D-2 Power Switch");
        let d2_door = reserve
            .iter()
            .position(|cluster| cluster.names.iter().any(|name| name == "D-2 Door Switch"))
            .expect("Reserve should include the D-2 Door Switch");
        assert_ne!(d2_power, d2_door);
    }

    #[test]
    fn minefield_fallback_is_used_only_below_the_seven_pixel_threshold() {
        let below_threshold = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(6.99, 2.0));
        let at_threshold = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(7.0, 2.0));

        assert!(minefield_uses_fallback(below_threshold));
        assert!(!minefield_uses_fallback(at_threshold));
    }

    #[test]
    fn overlapping_overlay_areas_are_combined_before_painting() {
        let left = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let right = vec![[5.0, 0.0], [15.0, 0.0], [15.0, 10.0], [5.0, 10.0]];

        let combined = union_overlay_areas([left.as_slice(), right.as_slice()]);

        assert_eq!(combined.0.len(), 1);
        assert!((combined.unsigned_area() - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn bundled_woods_hazard_geometry_merges_same_type_overlaps() {
        let maps: Vec<Map> = ron::from_str(include_str!("../../../assets/maps.ron"))
            .expect("bundled Maps should parse");
        let woods = maps
            .iter()
            .find(|map| map.normalized_name == "woods")
            .expect("Woods should be bundled");

        let merged = HazardGeometry::for_map(woods);

        assert!(merged.sniper_zones.0.len() < woods.sniper_zones.len());
        assert!(merged.minefields.0.len() < woods.minefields.len());
    }

    #[test]
    fn category_count_includes_only_offered_overlays() {
        let map = map_with_current_overlays();
        let visibility = OverlayVisibility {
            labels: false,
            scav_extracts: false,
            ..OverlayVisibility::default()
        };

        assert_eq!(category_count(MAP_OVERLAYS, &[], &map, &visibility), (1, 2));
        assert_eq!(
            category_count(EXTRACT_OVERLAYS, &[], &map, &visibility),
            (2, 2)
        );
    }

    #[test]
    fn bundled_maps_offer_the_expected_extract_overlays() {
        let maps: Vec<Map> = ron::from_str(include_str!("../../../assets/maps.ron"))
            .expect("bundled Maps should parse");
        let map = |normalized_name: &str| {
            maps.iter()
                .find(|map| map.normalized_name == normalized_name)
                .unwrap_or_else(|| panic!("missing bundled Map {normalized_name}"))
        };
        let visibility = OverlayVisibility::default();

        assert_eq!(
            category_count(EXTRACT_OVERLAYS, &[], map("terminal"), &visibility),
            (0, 0)
        );
        assert_eq!(
            category_count(EXTRACT_OVERLAYS, &[], map("the-lab"), &visibility),
            (2, 2)
        );
        assert_eq!(
            category_count(EXTRACT_OVERLAYS, &[], map("customs"), &visibility),
            (3, 3)
        );
    }
}
