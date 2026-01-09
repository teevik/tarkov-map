#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use rust_embed::RustEmbed;
use std::collections::HashMap;
use std::sync::mpsc;
use tarkov_map::{Extract, Label, Map, Spawn, TarkovMaps};

/// Embeds all assets from the assets/ directory into the binary.
/// In debug mode, assets are loaded from the filesystem for faster iteration.
/// In release mode, assets are compressed and embedded in the binary.
#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;
const SIDEBAR_WIDTH: f32 = 200.0;
const ZOOM_MIN: f32 = 1.0;
const ZOOM_MAX: f32 = 10.0;
const ZOOM_SPEED: f32 = 1.2;

mod colors {
    use eframe::egui::Color32;

    pub const SPAWN_FILL: Color32 = Color32::from_rgb(50, 205, 50);
    pub const SPAWN_STROKE: Color32 = Color32::from_rgb(0, 100, 0);

    pub const PMC_EXTRACT_FILL: Color32 = Color32::from_rgb(65, 105, 225);
    pub const PMC_EXTRACT_STROKE: Color32 = Color32::from_rgb(25, 25, 112);

    pub const SCAV_EXTRACT_FILL: Color32 = Color32::from_rgb(255, 165, 0);
    pub const SCAV_EXTRACT_STROKE: Color32 = Color32::from_rgb(139, 69, 19);

    pub const SHARED_EXTRACT_FILL: Color32 = Color32::from_rgb(186, 85, 211);
    pub const SHARED_EXTRACT_STROKE: Color32 = Color32::from_rgb(75, 0, 130);

    pub const LABEL_TEXT: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 220);
    pub const LABEL_SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 180);
    pub const EXTRACT_TEXT_SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 200);
}

struct DecodedImage {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

enum AssetLoadState {
    Loading(mpsc::Receiver<Result<DecodedImage, String>>),
    Ready(DecodedImage),
    Error(String),
}

#[derive(Clone, Copy)]
struct OverlayVisibility {
    labels: bool,
    spawns: bool,
    pmc_extracts: bool,
    scav_extracts: bool,
    shared_extracts: bool,
}

impl Default for OverlayVisibility {
    fn default() -> Self {
        Self {
            labels: false,
            spawns: true,
            pmc_extracts: true,
            scav_extracts: true,
            shared_extracts: true,
        }
    }
}

struct TarkovMapApp {
    maps: TarkovMaps,
    load_error: Option<String>,
    selected_map: usize,
    zoom: f32,
    prev_zoom: f32,
    pan_offset: egui::Vec2,
    sidebar_open: bool,
    overlays: OverlayVisibility,
    asset_cache: HashMap<String, AssetLoadState>,
    texture_cache: HashMap<String, TextureHandle>,
    #[allow(dead_code)]
    runtime: tokio::runtime::Runtime,
}

impl TarkovMapApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (maps, load_error) = match load_maps() {
            Ok(maps) => (maps, None),
            Err(err) => (Vec::new(), Some(err)),
        };

        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        let mut asset_cache = HashMap::new();

        // Preload all map images in background using blocking tasks for image decoding
        for map in &maps {
            let (tx, rx) = mpsc::channel();
            let ctx = cc.egui_ctx.clone();
            let asset_path = map.image_path.clone();

            runtime.spawn(async move {
                let result =
                    tokio::task::spawn_blocking(move || load_and_decode_image(&asset_path))
                        .await
                        .map_err(|e| format!("Task join error: {e}"))
                        .and_then(|r| r);
                let _ = tx.send(result);
                ctx.request_repaint();
            });

            asset_cache.insert(map.image_path.clone(), AssetLoadState::Loading(rx));
        }

        Self {
            maps,
            load_error,
            selected_map: 0,
            zoom: 1.0,
            prev_zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            sidebar_open: true,
            overlays: OverlayVisibility::default(),
            asset_cache,
            texture_cache: HashMap::new(),
            runtime,
        }
    }

    fn selected_map(&self) -> Option<&Map> {
        self.maps.get(self.selected_map)
    }

    fn poll_all_assets(&mut self, ctx: &egui::Context) {
        let mut updates: Vec<(String, AssetLoadState)> = Vec::new();

        for (path, state) in &mut self.asset_cache {
            if let AssetLoadState::Loading(rx) = state {
                match rx.try_recv() {
                    Ok(Ok(decoded)) => {
                        updates.push((path.clone(), AssetLoadState::Ready(decoded)));
                    }
                    Ok(Err(err)) => {
                        updates.push((path.clone(), AssetLoadState::Error(err)));
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        updates.push((
                            path.clone(),
                            AssetLoadState::Error("Channel disconnected".into()),
                        ));
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        }

        for (path, new_state) in updates {
            self.asset_cache.insert(path, new_state);
        }

        // Create textures for ready assets
        let ready_paths: Vec<_> = self
            .asset_cache
            .iter()
            .filter_map(|(path, state)| {
                matches!(state, AssetLoadState::Ready(_))
                    .then(|| !self.texture_cache.contains_key(path))
                    .and_then(|not_cached| not_cached.then(|| path.clone()))
            })
            .collect();

        for path in ready_paths {
            if let Some(AssetLoadState::Ready(decoded)) = self.asset_cache.get(&path) {
                let image = ColorImage::from_rgba_unmultiplied(
                    [decoded.width as usize, decoded.height as usize],
                    &decoded.pixels,
                );
                let texture = ctx.load_texture(&path, image, TextureOptions::LINEAR);
                self.texture_cache.insert(path, texture);
            }
        }
    }

    fn get_texture(&self, path: &str) -> Option<&TextureHandle> {
        self.texture_cache.get(path)
    }

    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan_offset = egui::Vec2::ZERO;
    }
}

impl eframe::App for TarkovMapApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_all_assets(ctx);
        self.handle_keyboard_input(ctx);

        let selected_map = self.selected_map().cloned();

        self.show_top_panel(ctx, &selected_map);
        self.show_status_bar(ctx, &selected_map);

        if self.sidebar_open {
            self.show_sidebar(ctx);
        }

        self.show_central_panel(ctx, selected_map);
        self.prev_zoom = self.zoom;
    }
}

impl TarkovMapApp {
    fn handle_keyboard_input(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                self.zoom = (self.zoom * ZOOM_SPEED).clamp(ZOOM_MIN, ZOOM_MAX);
            }
            if i.key_pressed(egui::Key::Minus) {
                self.zoom = (self.zoom / ZOOM_SPEED).clamp(ZOOM_MIN, ZOOM_MAX);
            }
            if i.key_pressed(egui::Key::Num0) {
                self.reset_view();
            }
            if i.key_pressed(egui::Key::L) {
                self.overlays.labels = !self.overlays.labels;
            }
            if i.key_pressed(egui::Key::Tab) {
                self.sidebar_open = !self.sidebar_open;
            }
        });
    }

    fn show_top_panel(&mut self, ctx: &egui::Context, selected_map: &Option<Map>) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let burger_text = if self.sidebar_open { "<<" } else { ">>" };
                if ui
                    .button(burger_text)
                    .on_hover_text("Toggle sidebar (Tab)")
                    .clicked()
                {
                    self.sidebar_open = !self.sidebar_open;
                }

                ui.separator();

                if let Some(map) = selected_map {
                    ui.strong(&map.name);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Fit").on_hover_text("Reset zoom (0)").clicked() {
                        self.reset_view();
                    }

                    ui.add(
                        egui::Slider::new(&mut self.zoom, ZOOM_MIN..=ZOOM_MAX)
                            .logarithmic(true)
                            .show_value(false)
                            .text("Zoom"),
                    );
                });
            });
        });
    }

    fn show_status_bar(&self, ctx: &egui::Context, selected_map: &Option<Map>) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    "Scroll: Zoom | Drag: Pan | +/-: Zoom | 0: Fit | L: Labels | Tab: Sidebar",
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(map) = selected_map {
                        if let Some(link) = &map.author_link {
                            ui.hyperlink_to(map.author.as_deref().unwrap_or("Map author"), link);
                            ui.label("Map by:");
                        } else if let Some(author) = &map.author {
                            ui.label(format!("Map by: {author}"));
                        }
                    }
                });
            });
        });
    }

    fn show_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(SIDEBAR_WIDTH)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_sidebar_content(ui);
                });
            });
    }

    fn show_sidebar_content(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);

        // Maps section
        ui.strong("Maps");
        ui.separator();

        if self.maps.is_empty() {
            ui.label("No maps loaded");
        } else {
            let prev_selected = self.selected_map;
            for (idx, map) in self.maps.iter().enumerate() {
                if ui
                    .selectable_label(self.selected_map == idx, &map.name)
                    .clicked()
                {
                    self.selected_map = idx;
                }
            }

            if self.selected_map != prev_selected {
                self.reset_view();
            }
        }

        ui.add_space(12.0);

        // Overlays section
        ui.strong("Overlays");
        ui.separator();

        Self::overlay_toggle_circle(
            ui,
            &mut self.overlays.labels,
            "Labels",
            egui::Color32::WHITE,
        );
        Self::overlay_toggle_circle(
            ui,
            &mut self.overlays.spawns,
            "PMC Spawns",
            colors::SPAWN_FILL,
        );
        Self::overlay_toggle_rect(
            ui,
            &mut self.overlays.pmc_extracts,
            "PMC Extracts",
            colors::PMC_EXTRACT_FILL,
        );
        Self::overlay_toggle_rect(
            ui,
            &mut self.overlays.scav_extracts,
            "Scav Extracts",
            colors::SCAV_EXTRACT_FILL,
        );
        Self::overlay_toggle_rect(
            ui,
            &mut self.overlays.shared_extracts,
            "Shared Extracts",
            colors::SHARED_EXTRACT_FILL,
        );
    }

    fn overlay_toggle_circle(
        ui: &mut egui::Ui,
        value: &mut bool,
        label: &str,
        color: egui::Color32,
    ) {
        ui.horizontal(|ui| {
            ui.checkbox(value, "");
            let (rect, icon_response) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
            let center = rect.center();
            ui.painter().circle_filled(center, 5.0, color);
            ui.painter()
                .circle_stroke(center, 5.0, egui::Stroke::new(1.0, egui::Color32::GRAY));
            let label_response = ui
                .label(label)
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if icon_response.clicked() || label_response.clicked() {
                *value = !*value;
            }
        });
    }

    fn overlay_toggle_rect(ui: &mut egui::Ui, value: &mut bool, label: &str, color: egui::Color32) {
        ui.horizontal(|ui| {
            ui.checkbox(value, "");
            let (rect, icon_response) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 2.0, color);
            ui.painter().rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, color.gamma_multiply(0.5)),
                egui::StrokeKind::Inside,
            );
            let label_response = ui
                .label(label)
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if icon_response.clicked() || label_response.clicked() {
                *value = !*value;
            }
        });
    }

    fn show_central_panel(&mut self, ctx: &egui::Context, selected_map: Option<Map>) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.load_error {
                ui.colored_label(egui::Color32::RED, err);
                ui.separator();
            }

            let Some(map) = selected_map else {
                ui.centered_and_justified(|ui| {
                    ui.label("No map data.\nRun `cargo run --bin fetch_maps` to generate assets.");
                });
                return;
            };

            self.show_map(ui, ctx, &map);
        });
    }

    fn show_map(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, map: &Map) {
        let image_path = &map.image_path;
        let logical_size = egui::vec2(map.logical_size[0], map.logical_size[1]);

        // Check loading state
        match self.asset_cache.get(image_path) {
            Some(AssetLoadState::Loading(_)) | None => {
                ui.centered_and_justified(|ui| ui.spinner());
                return;
            }
            Some(AssetLoadState::Error(err)) => {
                ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
                return;
            }
            Some(AssetLoadState::Ready(_)) => {}
        }

        let Some(texture) = self.get_texture(image_path) else {
            ui.label("Failed to create texture");
            return;
        };
        let texture_id = texture.id();

        let (viewport_rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let viewport_size = viewport_rect.size();

        // Calculate base scale to fit map in viewport at zoom 1.0
        let fit_scale = (viewport_size.x / logical_size.x).min(viewport_size.y / logical_size.y);

        // Handle zoom
        let zoomed_this_frame = self.handle_scroll_zoom(ui, viewport_rect);
        if !zoomed_this_frame {
            self.handle_slider_zoom();
        }

        // Handle drag panning
        if response.dragged() {
            self.pan_offset += response.drag_delta();
        }

        let display_size = logical_size * fit_scale * self.zoom;
        let map_center = viewport_rect.center() + self.pan_offset;
        let map_rect = egui::Rect::from_center_size(map_center, display_size);

        ui.set_clip_rect(viewport_rect);

        // Draw map image
        ui.painter().image(
            texture_id,
            map_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        // Draw overlays
        let overlays = self.overlays;
        if overlays.labels
            && let Some(labels) = &map.labels
        {
            draw_labels(ui, map_rect, map, labels, self.zoom);
        }

        if overlays.spawns
            && let Some(spawns) = &map.spawns
        {
            draw_spawns(ui, map_rect, map, spawns, self.zoom);
        }

        if let Some(extracts) = &map.extracts {
            draw_extracts(ui, map_rect, map, extracts, self.zoom, &overlays);
        }
    }

    fn handle_scroll_zoom(&mut self, ui: &mut egui::Ui, viewport_rect: egui::Rect) -> bool {
        let hover_pos = ui.input(|i| i.pointer.hover_pos());
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);

        if scroll_delta == 0.0 || !hover_pos.is_some_and(|p| viewport_rect.contains(p)) {
            return false;
        }

        let zoom_factor = if scroll_delta > 0.0 {
            ZOOM_SPEED
        } else {
            1.0 / ZOOM_SPEED
        };
        let new_zoom = (self.zoom * zoom_factor).clamp(ZOOM_MIN, ZOOM_MAX);

        // Zoom towards mouse position
        if let Some(hover) = hover_pos {
            let viewport_center = viewport_rect.center();
            let mouse_from_center = hover - viewport_center;
            let map_point = mouse_from_center - self.pan_offset;
            let zoom_ratio = new_zoom / self.zoom;
            let new_map_point = map_point * zoom_ratio;
            self.pan_offset = mouse_from_center - new_map_point;
        }

        self.zoom = new_zoom;
        true
    }

    fn handle_slider_zoom(&mut self) {
        let zoom_ratio = self.zoom / self.prev_zoom;
        if (zoom_ratio - 1.0).abs() > 0.001 {
            self.pan_offset *= zoom_ratio;
        }
    }
}

/// Rotates a 2D point by the given angle (in degrees).
fn rotate_point(x: f64, y: f64, angle_deg: f64) -> (f64, f64) {
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
fn game_to_display(map: &Map, map_rect: egui::Rect, game_pos: [f64; 2]) -> Option<egui::Pos2> {
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

fn draw_labels(ui: &mut egui::Ui, map_rect: egui::Rect, map: &Map, labels: &[Label], zoom: f32) {
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

fn draw_spawns(ui: &mut egui::Ui, map_rect: egui::Rect, map: &Map, spawns: &[Spawn], zoom: f32) {
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

fn draw_extracts(
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

fn load_and_decode_image(path: &str) -> Result<DecodedImage, String> {
    let file = Assets::get(path).ok_or_else(|| format!("Asset not found: {path}"))?;

    let img = image::load_from_memory(&file.data)
        .map_err(|err| format!("Decode error for {path}: {err}"))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    Ok(DecodedImage {
        pixels: rgba.into_raw(),
        width,
        height,
    })
}

fn load_maps() -> Result<TarkovMaps, String> {
    let file = Assets::get("maps.ron").ok_or("maps.ron not found in embedded assets")?;
    let ron_string =
        std::str::from_utf8(&file.data).map_err(|e| format!("Invalid UTF-8 in maps.ron: {e}"))?;
    ron::from_str(ron_string).map_err(|err| format!("Failed to parse maps.ron: {err}"))
}

fn main() -> eframe::Result {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Tarkov Map",
        options,
        Box::new(|cc| Ok(Box::new(TarkovMapApp::new(cc)))),
    )
}
