//! Shared vector shapes for marker Overlays and their sidebar glyphs.
//!
//! Every point marker is one solid shape in its Overlay colour with a darker
//! rim of the same hue — the same language as the Spawn disc and Extract
//! square. Shape carries meaning (circle = spawn, square = extract,
//! octagon = stop, chevron = go, bolt = power); colour carries category.
//! The sidebar glyph is the exact same shape painted into a 14-point box.

use crate::colors;
use eframe::egui;

/// Side length of the square sidebar glyph.
pub const GLYPH_SIZE: f32 = 14.0;

fn rim(size: f32, color: egui::Color32) -> egui::Stroke {
    egui::Stroke::new((size * 0.09).clamp(1.0, 2.0), color)
}

fn regular_polygon(pos: egui::Pos2, radius: f32, sides: usize, rotation: f32) -> Vec<egui::Pos2> {
    (0..sides)
        .map(|index| {
            let angle = rotation + index as f32 * std::f32::consts::TAU / sides as f32;
            pos + egui::vec2(angle.cos() * radius, angle.sin() * radius)
        })
        .collect()
}

/// Paints a flat-topped regular octagon — the stop-sign silhouette.
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

/// Paints a concave outline made of convex pieces: each piece is filled
/// without a stroke, then the whole silhouette is stroked once.
fn concave_shape(
    painter: &egui::Painter,
    pos: egui::Pos2,
    half: f32,
    pieces: &[&[[f32; 2]]],
    outline: &[[f32; 2]],
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    let project = |[x, y]: &[f32; 2]| pos + egui::vec2(x * half, y * half);
    for piece in pieces {
        painter.add(egui::Shape::convex_polygon(
            piece.iter().map(project).collect(),
            fill,
            egui::Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::closed_line(
        outline.iter().map(project).collect(),
        stroke,
    ));
}

/// Paints a solid right-pointing chevron, `size` tall.
pub fn chevron(
    painter: &egui::Painter,
    pos: egui::Pos2,
    size: f32,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    const UPPER: [[f32; 2]; 4] = [[-0.125, -1.0], [0.675, 0.0], [0.125, 0.0], [-0.675, -1.0]];
    const LOWER: [[f32; 2]; 4] = [[-0.125, 1.0], [0.675, 0.0], [0.125, 0.0], [-0.675, 1.0]];
    const OUTLINE: [[f32; 2]; 6] = [
        [-0.125, -1.0],
        [0.675, 0.0],
        [-0.125, 1.0],
        [-0.675, 1.0],
        [0.125, 0.0],
        [-0.675, -1.0],
    ];
    concave_shape(
        painter,
        pos,
        size * 0.5,
        &[&UPPER, &LOWER],
        &OUTLINE,
        fill,
        stroke,
    );
}

/// Paints a solid lightning bolt, `size` tall.
pub fn bolt(
    painter: &egui::Painter,
    pos: egui::Pos2,
    size: f32,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    const UPPER: [[f32; 2]; 4] = [[0.3, -1.0], [-0.5, 0.12], [-0.1, 0.12], [0.1, -0.12]];
    const LOWER: [[f32; 2]; 4] = [[-0.3, 1.0], [0.5, -0.12], [0.1, -0.12], [-0.1, 0.12]];
    const OUTLINE: [[f32; 2]; 6] = [
        [0.3, -1.0],
        [-0.5, 0.12],
        [-0.1, 0.12],
        [-0.3, 1.0],
        [0.5, -0.12],
        [0.1, -0.12],
    ];
    concave_shape(
        painter,
        pos,
        size * 0.5,
        &[&UPPER, &LOWER],
        &OUTLINE,
        fill,
        stroke,
    );
}

/// Paints the Transit marker: a cyan chevron.
pub fn transit(painter: &egui::Painter, pos: egui::Pos2, size: f32) {
    chevron(
        painter,
        pos,
        size,
        colors::TRANSIT,
        rim(size, colors::TRANSIT_STROKE),
    );
}

/// Paints the BTR Stop marker: an amber stop-sign octagon.
pub fn btr_stop(painter: &egui::Painter, pos: egui::Pos2, size: f32) {
    octagon(
        painter,
        pos,
        size * 0.5,
        colors::BTR_STOP,
        rim(size, colors::BTR_STOP_STROKE),
    );
}

/// Paints the Switch marker: a yellow lightning bolt.
pub fn switch(painter: &egui::Painter, pos: egui::Pos2, size: f32) {
    bolt(
        painter,
        pos,
        size,
        colors::SWITCH,
        rim(size, colors::SWITCH_STROKE),
    );
}

/// Allocates a sidebar glyph box and paints a marker into it at glyph scale.
pub fn glyph(ui: &mut egui::Ui, paint: fn(&egui::Painter, egui::Pos2, f32)) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(GLYPH_SIZE), egui::Sense::click());
    paint(ui.painter(), rect.center(), GLYPH_SIZE * 0.8);
    response
}
