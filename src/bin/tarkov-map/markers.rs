//! Shared vector primitives for marker Overlays and their sidebar glyphs.

use crate::colors;
use eframe::egui;

/// Paints the shared near-black marker disc with a coloured ring.
pub fn disc(
    painter: &egui::Painter,
    pos: egui::Pos2,
    size: f32,
    fill: egui::Color32,
    ring: egui::Color32,
) {
    painter.circle(pos, size * 0.5, fill, egui::Stroke::new(1.5, ring));
}

fn regular_polygon(pos: egui::Pos2, radius: f32, sides: usize, rotation: f32) -> Vec<egui::Pos2> {
    (0..sides)
        .map(|index| {
            let angle = rotation + index as f32 * std::f32::consts::TAU / sides as f32;
            pos + egui::vec2(angle.cos() * radius, angle.sin() * radius)
        })
        .collect()
}

/// Paints a regular octagon used by the BTR Stop icon.
#[allow(dead_code)] // Reserved for the BTR Stop Overlay ticket.
pub fn octagon(
    painter: &egui::Painter,
    pos: egui::Pos2,
    radius: f32,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    let points = regular_polygon(pos, radius, 8, std::f32::consts::PI / 8.0);
    painter.add(egui::Shape::convex_polygon(points, fill, stroke));
}

/// Paints the shared skull icon used by Mob Overlays.
#[allow(dead_code)] // Reserved for the Boss Spawn Overlay ticket.
pub fn icon_skull(
    painter: &egui::Painter,
    pos: egui::Pos2,
    size: f32,
    bone: egui::Color32,
    dark: egui::Color32,
) {
    let radius = size * 0.24;
    let head = pos + egui::vec2(0.0, -radius * 0.25);
    painter.circle_filled(head, radius, bone);
    let jaw = egui::Rect::from_center_size(
        head + egui::vec2(0.0, radius * 0.95),
        egui::vec2(radius * 1.1, radius * 0.7),
    );
    painter.rect_filled(jaw, 1.0, bone);
    let eye_offset = radius * 0.42;
    painter.circle_filled(
        head + egui::vec2(-eye_offset, -radius * 0.05),
        eye_offset * 0.6,
        dark,
    );
    painter.circle_filled(
        head + egui::vec2(eye_offset, -radius * 0.05),
        eye_offset * 0.6,
        dark,
    );
}

/// Paints the shared lightning-bolt icon used by the Switches Overlay.
#[allow(dead_code)] // Reserved for the Switches Overlay ticket.
pub fn icon_bolt(painter: &egui::Painter, pos: egui::Pos2, size: f32, color: egui::Color32) {
    let scale = size * 0.28;
    let points = vec![
        pos + egui::vec2(scale * 0.35, -scale),
        pos + egui::vec2(-scale * 0.35, scale * 0.1),
        pos + egui::vec2(scale * 0.15, scale * 0.1),
        pos + egui::vec2(-scale * 0.35, scale),
    ];
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(size * 0.09, color),
    ));
}

/// Paints the double-chevron icon used by the Transits Overlay.
pub fn icon_chevrons(painter: &egui::Painter, pos: egui::Pos2, size: f32, color: egui::Color32) {
    let scale = size * 0.22;
    let width = size * 0.09;
    for offset_x in [-scale * 0.55, scale * 0.35] {
        let points = vec![
            pos + egui::vec2(offset_x - scale * 0.4, -scale),
            pos + egui::vec2(offset_x + scale * 0.4, 0.0),
            pos + egui::vec2(offset_x - scale * 0.4, scale),
        ];
        painter.add(egui::Shape::line(points, egui::Stroke::new(width, color)));
    }
}

/// Allocates and paints the 14-point sidebar glyph for a disc marker Overlay.
pub fn glyph_disc(
    ui: &mut egui::Ui,
    icon: fn(&egui::Painter, egui::Pos2, f32, egui::Color32),
    color: egui::Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::click());
    let size = 14.0;
    disc(
        ui.painter(),
        rect.center(),
        size,
        colors::MARKER_DISC,
        color,
    );
    icon(ui.painter(), rect.center(), size, color);
    response
}
