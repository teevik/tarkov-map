//! Overlay visibility settings and drawing functions for map markers.

use crate::colors;
use crate::coordinates::game_to_display;
use crate::screenshot_watcher::PlayerPosition;
use eframe::egui;
use serde::{Deserialize, Serialize};
use tarkov_map::{Extract, Label, Map, Spawn};

/// Paints text with a dark outline so it stays legible over any map colour.
fn outlined_text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    anchor: egui::Align2,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
    outline: egui::Color32,
) {
    for offset in [
        egui::vec2(-1.0, 0.0),
        egui::vec2(1.0, 0.0),
        egui::vec2(0.0, -1.0),
        egui::vec2(0.0, 1.0),
    ] {
        painter.text(pos + offset, anchor, text, font_id.clone(), outline);
    }
    painter.text(pos, anchor, text, font_id, color);
}

/// Controls visibility of different overlay types on the map.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayVisibility {
    pub labels: bool,
    pub spawns: bool,
    pub pmc_extracts: bool,
    pub scav_extracts: bool,
    pub shared_extracts: bool,
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
    map: &Map,
    visibility: &OverlayVisibility,
) -> (usize, usize) {
    overlays
        .into_iter()
        .filter(|overlay| overlay_offered(*overlay, map))
        .fold((0, 0), |(on, total), overlay| {
            (on + usize::from((overlay.visible)(visibility)), total + 1)
        })
}

impl Default for OverlayVisibility {
    fn default() -> Self {
        Self {
            labels: false,
            spawns: true,
            pmc_extracts: true,
            scav_extracts: true,
            shared_extracts: true,
            player_marker: true,
        }
    }
}

/// Draws label overlays on the map.
pub fn draw_labels(
    ui: &mut egui::Ui,
    map_rect: egui::Rect,
    map: &Map,
    labels: &[Label],
    zoom: f32,
) {
    let painter = ui.painter();

    for label in labels {
        let Some(pos) = game_to_display(map, map_rect, label.position) else {
            continue;
        };

        if !map_rect.expand(50.0).contains(pos) {
            continue;
        }

        let base_size = label.size.unwrap_or(40) as f32 * 0.15;
        let font_size = (base_size * zoom).clamp(8.0, 48.0);
        let font_id = egui::FontId::proportional(font_size);

        outlined_text(
            painter,
            pos,
            egui::Align2::CENTER_CENTER,
            &label.text,
            font_id,
            colors::LABEL_TEXT,
            colors::LABEL_SHADOW,
        );
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

/// Draws extraction point markers on the map.
pub fn draw_extracts(
    ui: &mut egui::Ui,
    map_rect: egui::Rect,
    map: &Map,
    extracts: &[Extract],
    zoom: f32,
    overlays: &OverlayVisibility,
) {
    let painter = ui.painter();

    for extract in extracts {
        let faction = extract.faction.to_lowercase();
        let (fill_color, stroke_color) = match faction.as_str() {
            "pmc" if overlays.pmc_extracts => {
                (colors::PMC_EXTRACT_FILL, colors::PMC_EXTRACT_STROKE)
            }
            "scav" if overlays.scav_extracts => {
                (colors::SCAV_EXTRACT_FILL, colors::SCAV_EXTRACT_STROKE)
            }
            "shared" if overlays.shared_extracts => {
                (colors::SHARED_EXTRACT_FILL, colors::SHARED_EXTRACT_STROKE)
            }
            _ => continue,
        };

        let Some(position) = extract.position else {
            continue;
        };

        let game_pos = [position[0], position[2]];
        let Some(pos) = game_to_display(map, map_rect, game_pos) else {
            continue;
        };

        if !map_rect.expand(20.0).contains(pos) {
            continue;
        }

        let size = (12.0 * zoom).clamp(8.0, 32.0);
        let rect = egui::Rect::from_center_size(pos, egui::vec2(size, size));

        painter.rect_filled(rect, 2.0, fill_color);
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(2.0_f32, stroke_color),
            egui::StrokeKind::Outside,
        );

        // Extract name label
        let font_size = (6.0 * zoom).clamp(9.0, 18.0);
        let font_id = egui::FontId::proportional(font_size);
        let text_pos = pos + egui::vec2(0.0, -size / 2.0 - 4.0);

        outlined_text(
            painter,
            text_pos,
            egui::Align2::CENTER_BOTTOM,
            &extract.name,
            font_id,
            egui::Color32::WHITE,
            colors::EXTRACT_TEXT_SHADOW,
        );
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

    const MAP_OVERLAYS: [OverlayKind; 2] = [OverlayKind::LABELS, OverlayKind::PLAYER_MARKER];
    const EXTRACT_OVERLAYS: [OverlayKind; 3] = [
        OverlayKind::PMC_EXTRACTS,
        OverlayKind::SCAV_EXTRACTS,
        OverlayKind::SHARED_EXTRACTS,
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
    fn category_count_includes_only_offered_overlays() {
        let map = map_with_current_overlays();
        let visibility = OverlayVisibility {
            labels: false,
            scav_extracts: false,
            ..OverlayVisibility::default()
        };

        assert_eq!(category_count(MAP_OVERLAYS, &map, &visibility), (1, 2));
        assert_eq!(category_count(EXTRACT_OVERLAYS, &map, &visibility), (2, 2));
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
            category_count(EXTRACT_OVERLAYS, map("terminal"), &visibility),
            (0, 0)
        );
        assert_eq!(
            category_count(EXTRACT_OVERLAYS, map("the-lab"), &visibility),
            (2, 2)
        );
        assert_eq!(
            category_count(EXTRACT_OVERLAYS, map("customs"), &visibility),
            (3, 3)
        );
    }
}
