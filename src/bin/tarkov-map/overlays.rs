//! Overlay visibility settings and drawing functions for map markers.

use crate::colors;
use crate::coordinates::game_to_display;
use crate::screenshot_watcher::PlayerPosition;
use eframe::egui;
use serde::{Deserialize, Serialize};
use tarkov_map::{Extract, Label, Map, Spawn};

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

        // Shadow
        painter.text(
            pos + egui::vec2(1.0, 1.0),
            egui::Align2::CENTER_CENTER,
            &label.text,
            font_id.clone(),
            colors::LABEL_SHADOW,
        );

        // Main text
        painter.text(
            pos,
            egui::Align2::CENTER_CENTER,
            &label.text,
            font_id,
            colors::LABEL_TEXT,
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
            egui::Stroke::new(1.5, colors::SPAWN_STROKE),
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
            egui::Stroke::new(2.0, stroke_color),
            egui::StrokeKind::Outside,
        );

        // Extract name label
        let font_size = (6.0 * zoom).clamp(9.0, 18.0);
        let font_id = egui::FontId::proportional(font_size);
        let text_pos = pos + egui::vec2(0.0, -size / 2.0 - 4.0);

        painter.text(
            text_pos + egui::vec2(1.0, 1.0),
            egui::Align2::CENTER_BOTTOM,
            &extract.name,
            font_id.clone(),
            colors::EXTRACT_TEXT_SHADOW,
        );
        painter.text(
            text_pos,
            egui::Align2::CENTER_BOTTOM,
            &extract.name,
            font_id,
            egui::Color32::WHITE,
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
        egui::Stroke::new(2.0, colors::PLAYER_MARKER_STROKE),
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
        egui::Stroke::new(1.5, colors::PLAYER_MARKER_STROKE),
    ));
}
