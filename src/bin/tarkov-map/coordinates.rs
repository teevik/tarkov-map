//! Coordinate transformation utilities for converting game coordinates to display positions.

use eframe::egui;
use tarkov_map::Map;

/// Rotates a 2D point by the given angle (in degrees).
pub fn rotate_point(x: f64, y: f64, angle_deg: f64) -> (f64, f64) {
    if angle_deg == 0.0 {
        return (x, y);
    }
    let angle_rad = angle_deg.to_radians();
    let (sin, cos) = angle_rad.sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

/// Converts game coordinates to display position.
///
/// The transformation follows the official tarkov-dev implementation:
/// 1. Apply coordinate rotation (rotate game coords by `coordinateRotation` degrees)
/// 2. Map the rotated coordinates to the image using the rotated bounds
pub fn game_to_display(map: &Map, map_rect: egui::Rect, game_pos: [f64; 2]) -> Option<egui::Pos2> {
    let bounds = map.bounds?;
    let rotation = map.coordinate_rotation.unwrap_or(0.0);

    let (rotated_x, rotated_y) = rotate_point(game_pos[0], game_pos[1], rotation);

    // For 270° rotation maps with transform, use transform-based approach
    // (handles SVG padding/margins in maps like Labs and Labyrinth)
    if rotation == 270.0
        && let Some(transform) = map.transform
    {
        let scale_x = transform[0];
        let margin_x = transform[1];
        let scale_y = -transform[2]; // Negated per tarkov-dev convention
        let margin_y = transform[3];

        let svg_x = scale_x * rotated_x + margin_x;
        let svg_y = scale_y * rotated_y + margin_y;

        let frac_x = svg_x / f64::from(map.image_size[0]);
        let frac_y = svg_y / f64::from(map.image_size[1]);

        let display_x = map_rect.min.x + (frac_x as f32) * map_rect.width();
        let display_y = map_rect.min.y + (frac_y as f32) * map_rect.height();

        return Some(egui::pos2(display_x, display_y));
    }

    // Rotate bounds corners to find rotated extent
    let corners = [
        (bounds[0][0], bounds[0][1]), // (maxX, minY)
        (bounds[0][0], bounds[1][1]), // (maxX, maxY)
        (bounds[1][0], bounds[0][1]), // (minX, minY)
        (bounds[1][0], bounds[1][1]), // (minX, maxY)
    ];

    let rotated_corners: Vec<_> = corners
        .iter()
        .map(|(x, y)| rotate_point(*x, *y, rotation))
        .collect();

    let (rotated_min_x, rotated_max_x) = rotated_corners
        .iter()
        .map(|(x, _)| *x)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), x| {
            (min.min(x), max.max(x))
        });

    let (rotated_min_y, rotated_max_y) = rotated_corners
        .iter()
        .map(|(_, y)| *y)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), y| {
            (min.min(y), max.max(y))
        });

    let bounds_width = rotated_max_x - rotated_min_x;
    let bounds_height = rotated_max_y - rotated_min_y;

    let frac_x = (rotated_x - rotated_min_x) / bounds_width;
    let frac_y = (rotated_max_y - rotated_y) / bounds_height; // Y inverted

    let display_x = map_rect.min.x + (frac_x as f32) * map_rect.width();
    let display_y = map_rect.min.y + (frac_y as f32) * map_rect.height();

    Some(egui::pos2(display_x, display_y))
}
