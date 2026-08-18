//! PROTOTYPE — throwaway. Wayfinder ticket #28.
//!
//! Three variants of the categorised Overlays sidebar, mounted inside the real
//! sidebar in place of the flat toggle list. Switch with the floating bar at the
//! bottom of the map (or ← / → keys). `G` toggles the marker glyphs. Nothing
//! here persists; the five new toggles are in-memory only.
//!
//! Auto-screenshot: `TARKOV_MAP_PROTO_SHOTS=<dir>` captures every variant with
//! and without glyphs to `<dir>/variant-<X>-glyphs-<on|off>.png`, then quits.

use crate::TarkovMapApp;
use crate::colors;
use eframe::egui::{self, Color32};

pub const VARIANTS: [&str; 3] = ["Eyebrow headers", "Chips", "Collapsing headers"];

/// Placeholder colours for the not-yet-designed overlays. NOT the visual-language
/// decision — that is fog on the map. Only here so the toggles have a glyph.
const BOSS_FILL: Color32 = Color32::from_rgb(220, 60, 90);
const TRANSIT_FILL: Color32 = Color32::from_rgb(0, 190, 190);
const BTR_FILL: Color32 = Color32::from_rgb(218, 165, 32);
const SWITCH_FILL: Color32 = Color32::from_rgb(240, 220, 60);
const HAZARD_FILL: Color32 = Color32::from_rgb(230, 90, 30);

#[derive(Default)]
pub struct ProtoToggles {
    pub bosses: bool,
    pub transits: bool,
    pub btr_stops: bool,
    pub switches: bool,
    pub hazards: bool,
}

pub struct ProtoState {
    pub variant: usize,
    pub glyphs: bool,
    pub extra: ProtoToggles,
    shots: Option<Shots>,
}

struct Shots {
    dir: std::path::PathBuf,
    /// Index into the (variant, glyphs) grid still to capture.
    next: usize,
    /// Frames rendered since the last change, so the layout has settled.
    settle: u32,
    pending: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Circle,
    Rect,
    Triangle,
}

struct Toggle {
    label: &'static str,
    glyph: Glyph,
    color: Color32,
    is_new: bool,
}

const fn t(label: &'static str, glyph: Glyph, color: Color32, is_new: bool) -> Toggle {
    Toggle { label, glyph, color, is_new }
}

/// Category titles and their toggles, in the order decided by #27.
fn categories() -> [(&'static str, Vec<Toggle>); 5] {
    [
        (
            "Map",
            vec![
                t("Labels", Glyph::Circle, Color32::WHITE, false),
                t("Player marker", Glyph::Triangle, colors::PLAYER_MARKER_FILL, false),
            ],
        ),
        (
            "Spawns",
            vec![
                t("PMC Spawns", Glyph::Circle, colors::SPAWN_FILL, false),
                t("Bosses", Glyph::Circle, BOSS_FILL, true),
            ],
        ),
        (
            "Extracts",
            vec![
                t("PMC Extracts", Glyph::Rect, colors::PMC_EXTRACT_FILL, false),
                t("Scav Extracts", Glyph::Rect, colors::SCAV_EXTRACT_FILL, false),
                t("Shared Extracts", Glyph::Rect, colors::SHARED_EXTRACT_FILL, false),
            ],
        ),
        (
            "Navigation",
            vec![
                t("Transits", Glyph::Rect, TRANSIT_FILL, true),
                t("BTR stops", Glyph::Circle, BTR_FILL, true),
                t("Switches", Glyph::Rect, SWITCH_FILL, true),
            ],
        ),
        ("Hazards", vec![t("Hazards", Glyph::Rect, HAZARD_FILL, true)]),
    ]
}

impl ProtoState {
    pub fn from_env() -> Self {
        let shots = std::env::var_os("TARKOV_MAP_PROTO_SHOTS").map(|dir| Shots {
            dir: dir.into(),
            next: 0,
            settle: 0,
            pending: false,
        });
        Self {
            variant: 0,
            glyphs: true,
            extra: ProtoToggles::default(),
            shots,
        }
    }
}

impl TarkovMapApp {
    fn proto_toggle_ref(&mut self, label: &str) -> &mut bool {
        let o = &mut self.overlays;
        let x = &mut self.prototype.extra;
        match label {
            "Labels" => &mut o.labels,
            "Player marker" => &mut o.player_marker,
            "PMC Spawns" => &mut o.spawns,
            "Bosses" => &mut x.bosses,
            "PMC Extracts" => &mut o.pmc_extracts,
            "Scav Extracts" => &mut o.scav_extracts,
            "Shared Extracts" => &mut o.shared_extracts,
            "Transits" => &mut x.transits,
            "BTR stops" => &mut x.btr_stops,
            "Switches" => &mut x.switches,
            "Hazards" => &mut x.hazards,
            _ => unreachable!(),
        }
    }

    /// Replaces the flat Overlays list in the sidebar.
    pub fn show_overlays_prototype(&mut self, ui: &mut egui::Ui) {
        Self::section_header(ui, "Overlays");
        if self.prototype.shots.is_some() {
            ui.scroll_to_cursor(Some(egui::Align::TOP));
        }
        match self.prototype.variant {
            0 => self.proto_variant_eyebrow(ui),
            1 => self.proto_variant_chips(ui),
            _ => self.proto_variant_collapsing(ui),
        }
    }

    // ---- Variant A: eyebrow category titles, indented toggles (as #27 decided) ----
    fn proto_variant_eyebrow(&mut self, ui: &mut egui::Ui) {
        let glyphs = self.prototype.glyphs;
        for (title, toggles) in categories() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(title)
                    .size(11.5)
                    .strong()
                    .color(ui.visuals().text_color().gamma_multiply(0.75)),
            );
            ui.add_space(1.0);
            ui.indent(title, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for tg in &toggles {
                    let value = self.proto_toggle_ref(tg.label);
                    toggle_row(ui, value, tg, glyphs);
                }
            });
        }
    }

    // ---- Variant B: category title, toggles as wrapping pill chips ----
    fn proto_variant_chips(&mut self, ui: &mut egui::Ui) {
        let glyphs = self.prototype.glyphs;
        for (title, toggles) in categories() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(title.to_uppercase())
                    .size(9.5)
                    .color(ui.visuals().weak_text_color()),
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for tg in &toggles {
                    let value = self.proto_toggle_ref(tg.label);
                    chip(ui, value, tg, glyphs);
                }
            });
        }
    }

    // ---- Variant C: collapsing headers with an "n/m on" summary ----
    fn proto_variant_collapsing(&mut self, ui: &mut egui::Ui) {
        let glyphs = self.prototype.glyphs;
        for (title, toggles) in categories() {
            let on = toggles
                .iter()
                .filter(|tg| *self.proto_toggle_ref(tg.label))
                .count();
            let header = format!("{title}   {on}/{}", toggles.len());
            egui::CollapsingHeader::new(egui::RichText::new(header).size(12.5))
                .id_salt(title)
                .default_open(true)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for tg in &toggles {
                        let value = self.proto_toggle_ref(tg.label);
                        toggle_row(ui, value, tg, glyphs);
                    }
                });
        }
    }

    /// Floating switcher over the map, plus keyboard handling and the
    /// screenshot driver. Call once per frame after the panels.
    pub fn show_prototype_bar(&mut self, ctx: &egui::Context, panel_rect: egui::Rect) {
        let n = VARIANTS.len();
        if !ctx.egui_wants_keyboard_input() {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::ArrowRight) {
                    self.prototype.variant = (self.prototype.variant + 1) % n;
                }
                if i.key_pressed(egui::Key::ArrowLeft) {
                    self.prototype.variant = (self.prototype.variant + n - 1) % n;
                }
                if i.key_pressed(egui::Key::G) {
                    self.prototype.glyphs = !self.prototype.glyphs;
                }
            });
        }

        let bar_w = 320.0;
        let pos = egui::pos2(
            panel_rect.center().x - bar_w / 2.0,
            panel_rect.bottom() - 60.0,
        );
        egui::Area::new(egui::Id::new("prototype_bar"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(Color32::from_rgb(250, 230, 90))
                    .stroke(egui::Stroke::new(1.5, Color32::BLACK))
                    .corner_radius(18)
                    .show(ui, |ui| {
                        ui.set_width(bar_w);
                        ui.visuals_mut().override_text_color = Some(Color32::BLACK);
                        ui.horizontal(|ui| {
                            if ui.button("◀").clicked() {
                                self.prototype.variant = (self.prototype.variant + n - 1) % n;
                            }
                            let key = (b'A' + self.prototype.variant as u8) as char;
                            ui.label(
                                egui::RichText::new(format!(
                                    "PROTOTYPE  {key} — {}",
                                    VARIANTS[self.prototype.variant]
                                ))
                                .strong(),
                            );
                            if ui.button("▶").clicked() {
                                self.prototype.variant = (self.prototype.variant + 1) % n;
                            }
                            ui.checkbox(&mut self.prototype.glyphs, "glyphs");
                        });
                    });
            });

        self.drive_screenshots(ctx);
    }

    fn drive_screenshots(&mut self, ctx: &egui::Context) {
        let Some(shots) = self.prototype.shots.as_mut() else {
            return;
        };
        let grid: Vec<(usize, bool)> = (0..VARIANTS.len())
            .flat_map(|v| [(v, true), (v, false)])
            .collect();

        // Save any screenshot delivered this frame.
        let mut delivered = None;
        ctx.input(|i| {
            for ev in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = ev {
                    delivered = Some(image.clone());
                }
            }
        });
        if let Some(image) = delivered {
            let (v, g) = grid[shots.next];
            let key = (b'A' + v as u8) as char;
            let path = shots.dir.join(format!(
                "variant-{key}-glyphs-{}.png",
                if g { "on" } else { "off" }
            ));
            std::fs::create_dir_all(&shots.dir).ok();
            let [w, h] = image.size;
            let rgba: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(|c| c.to_srgba_unmultiplied())
                .collect();
            image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
                .expect("write screenshot");
            eprintln!("saved {}", path.display());
            shots.next += 1;
            shots.pending = false;
            shots.settle = 0;
            if shots.next >= grid.len() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }

        if shots.pending || shots.next >= grid.len() {
            ctx.request_repaint();
            return;
        }
        let (v, g) = grid[shots.next];
        self.prototype.variant = v;
        self.prototype.glyphs = g;
        shots.settle += 1;
        if shots.settle == 1 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1100.0, 900.0)));
        }
        // Give the map texture and layout a moment to settle before capturing.
        if shots.settle > 90 {
            shots.pending = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
    }
}

fn paint_glyph(ui: &mut egui::Ui, tg: &Toggle) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
    let p = ui.painter();
    match tg.glyph {
        Glyph::Circle => {
            p.circle_filled(rect.center(), 5.0, tg.color);
            p.circle_stroke(rect.center(), 5.0, egui::Stroke::new(1.0, Color32::GRAY));
        }
        Glyph::Rect => {
            p.rect_filled(rect, 2.0, tg.color);
            p.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, tg.color.gamma_multiply(0.5)),
                egui::StrokeKind::Inside,
            );
        }
        Glyph::Triangle => {
            let c = rect.center();
            let s = 5.0;
            p.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::vec2(0.0, -s),
                    c + egui::vec2(-s * 0.7, s * 0.5),
                    c + egui::vec2(s * 0.7, s * 0.5),
                ],
                tg.color,
                egui::Stroke::new(1.0, tg.color.gamma_multiply(0.5)),
            ));
        }
    }
    resp
}

/// The existing checkbox + glyph + label row.
fn toggle_row(ui: &mut egui::Ui, value: &mut bool, tg: &Toggle, glyphs: bool) {
    ui.horizontal(|ui| {
        ui.checkbox(value, "");
        let mut clicked = false;
        if glyphs {
            clicked |= paint_glyph(ui, tg).clicked();
        }
        let label = ui
            .label(tg.label)
            .interact(egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        clicked |= label.clicked();
        if tg.is_new {
            ui.label(egui::RichText::new("new").size(8.5).weak());
        }
        if clicked {
            *value = !*value;
        }
    });
}

/// A pill that is filled with the overlay colour when on, outlined when off.
fn chip(ui: &mut egui::Ui, value: &mut bool, tg: &Toggle, glyphs: bool) {
    let font = egui::FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(tg.label.to_string(), font, Color32::WHITE);
    let pad = egui::vec2(8.0, 3.0);
    let glyph_w = if glyphs { 12.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + pad.x * 2.0 + glyph_w, galley.size().y + pad.y * 2.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *value = !*value;
    }
    let on = *value;
    let p = ui.painter();
    let fill = if on { tg.color.gamma_multiply(0.35) } else { Color32::TRANSPARENT };
    let stroke = if on { tg.color } else { ui.visuals().weak_text_color() };
    p.rect(rect, 999.0, fill, egui::Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    let mut x = rect.left() + pad.x;
    if glyphs {
        let c = egui::pos2(x + 4.0, rect.center().y);
        let col = if on { tg.color } else { ui.visuals().weak_text_color() };
        match tg.glyph {
            Glyph::Circle => { p.circle_filled(c, 4.0, col); }
            Glyph::Rect => { p.rect_filled(egui::Rect::from_center_size(c, egui::vec2(8.0, 8.0)), 1.5, col); }
            Glyph::Triangle => {
                p.add(egui::Shape::convex_polygon(
                    vec![
                        c + egui::vec2(0.0, -4.0),
                        c + egui::vec2(-3.5, 2.5),
                        c + egui::vec2(3.5, 2.5),
                    ],
                    col,
                    egui::Stroke::NONE,
                ));
            }
        }
        x += glyph_w;
    }
    let text_col = if on { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
    p.galley(egui::pos2(x, rect.top() + pad.y), galley, text_col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
}
