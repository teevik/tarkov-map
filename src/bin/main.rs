#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use log::error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tarkov_map::{Label, Map, Spawn, TarkovMaps};

const MAPS_RON_PATH: &str = "assets/maps.ron";

/// Min/max zoom levels (1.0 = fit to screen)
const ZOOM_MIN: f32 = 1.0;
const ZOOM_MAX: f32 = 10.0;

/// Zoom speed for mouse wheel (multiplier per scroll unit)
const ZOOM_SPEED: f32 = 1.2;

// ============================================================================
// Asset loading
// ============================================================================

enum AssetLoadState {
    Loading(mpsc::Receiver<Result<Vec<u8>, String>>),
    Ready(Vec<u8>),
    Error(String),
}

// ============================================================================
// Application
// ============================================================================

struct TarkovMapApp {
    maps: TarkovMaps,
    load_error: Option<String>,

    selected_map: usize,
    zoom: f32,
    prev_zoom: f32,

    /// Pan offset in display coordinates
    pan_offset: egui::Vec2,

    /// Whether to show map labels
    show_labels: bool,

    /// Whether to show PMC spawns
    show_spawns: bool,

    /// Raw asset bytes cache
    asset_cache: HashMap<String, AssetLoadState>,
    /// PNG texture cache
    texture_cache: HashMap<String, TextureHandle>,

    runtime: tokio::runtime::Runtime,
}

impl TarkovMapApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let maps_path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), MAPS_RON_PATH);

        let (maps, load_error) = match load_maps(&maps_path) {
            Ok(maps) => (maps, None),
            Err(err) => (Vec::new(), Some(err)),
        };

        let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");

        Self {
            maps,
            load_error,
            selected_map: 0,
            zoom: 1.0,
            prev_zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            show_labels: true,
            show_spawns: true,
            asset_cache: HashMap::new(),
            texture_cache: HashMap::new(),
            runtime,
        }
    }

    fn selected_map(&self) -> Option<&Map> {
        self.maps.get(self.selected_map)
    }

    /// Request async loading of an asset
    fn request_asset(&mut self, ctx: &egui::Context, path: &str) {
        if self.asset_cache.contains_key(path) {
            return;
        }

        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let asset_path = path.to_owned();

        self.runtime.spawn(async move {
            let result = load_asset_bytes(&asset_path).await;
            let _ = tx.send(result);
            ctx.request_repaint();
        });

        self.asset_cache
            .insert(path.to_owned(), AssetLoadState::Loading(rx));
    }

    /// Poll for completed asset loads
    fn poll_asset(&mut self, path: &str) {
        let mut done: Option<AssetLoadState> = None;

        if let Some(AssetLoadState::Loading(rx)) = self.asset_cache.get_mut(path) {
            match rx.try_recv() {
                Ok(Ok(bytes)) => done = Some(AssetLoadState::Ready(bytes)),
                Ok(Err(err)) => done = Some(AssetLoadState::Error(err)),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(AssetLoadState::Error("channel disconnected".to_owned()));
                }
            }
        }

        if let Some(new_state) = done {
            self.asset_cache.insert(path.to_owned(), new_state);
        }
    }

    /// Get or create texture from loaded bytes
    fn get_texture(&mut self, ctx: &egui::Context, path: &str) -> Option<&TextureHandle> {
        if let Some(AssetLoadState::Ready(bytes)) = self.asset_cache.get(path) {
            if !self.texture_cache.contains_key(path) {
                match image::load_from_memory(bytes) {
                    Ok(img) => {
                        let rgba = img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        let image = ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            rgba.as_raw(),
                        );
                        let texture = ctx.load_texture(path, image, TextureOptions::LINEAR);
                        self.texture_cache.insert(path.to_owned(), texture);
                    }
                    Err(err) => {
                        error!("Failed to decode image {}: {}", path, err);
                        return None;
                    }
                }
            }
            self.texture_cache.get(path)
        } else {
            None
        }
    }

    fn show_map(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, map: &Map) {
        let image_path = &map.image_path;
        let logical_size = egui::vec2(map.logical_size[0], map.logical_size[1]);

        // Request and poll the image
        self.request_asset(_ctx, image_path);
        self.poll_asset(image_path);

        // Check loading state
        match self.asset_cache.get(image_path) {
            Some(AssetLoadState::Loading(_)) | None => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Loading map...");
                });
                return;
            }
            Some(AssetLoadState::Error(err)) => {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
                return;
            }
            Some(AssetLoadState::Ready(_)) => {}
        }

        // Get texture
        let Some(texture) = self.get_texture(_ctx, image_path) else {
            ui.label("Failed to create texture");
            return;
        };
        let texture_id = texture.id();

        // Allocate the full available area for interaction
        let (viewport_rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let viewport_size = viewport_rect.size();

        // Calculate base scale to fit map in viewport at zoom 1.0
        let scale_x = viewport_size.x / logical_size.x;
        let scale_y = viewport_size.y / logical_size.y;
        let fit_scale = scale_x.min(scale_y);

        // Handle mouse wheel zoom
        let hover_pos = ui.input(|i| i.pointer.hover_pos());
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
        let mut zoomed_this_frame = false;

        if scroll_delta != 0.0 && hover_pos.map_or(false, |p| viewport_rect.contains(p)) {
            let zoom_factor = if scroll_delta > 0.0 {
                ZOOM_SPEED
            } else {
                1.0 / ZOOM_SPEED
            };
            let new_zoom = (self.zoom * zoom_factor).clamp(ZOOM_MIN, ZOOM_MAX);

            // Zoom towards mouse position
            if let Some(hover) = hover_pos {
                // Mouse position relative to viewport center
                let viewport_center = viewport_rect.center();
                let mouse_from_center = hover - viewport_center;

                // Current point on map under mouse (in display coords from center)
                let map_point = mouse_from_center - self.pan_offset;

                // Scale to new zoom
                let zoom_ratio = new_zoom / self.zoom;
                let new_map_point = map_point * zoom_ratio;

                // Adjust pan to keep mouse over same map point
                self.pan_offset = mouse_from_center - new_map_point;
            }

            self.zoom = new_zoom;
            zoomed_this_frame = true;
        }

        // Handle slider zoom (center-based) - only if not already zoomed by scroll
        if !zoomed_this_frame {
            let zoom_ratio = self.zoom / self.prev_zoom;
            if (zoom_ratio - 1.0).abs() > 0.001 {
                self.pan_offset = self.pan_offset * zoom_ratio;
            }
        }

        // Handle drag panning
        if response.dragged() {
            self.pan_offset += response.drag_delta();
        }

        // Calculate display size with current zoom (after all zoom updates)
        let display_size = logical_size * fit_scale * self.zoom;

        // Calculate map rect (centered in viewport, offset by pan)
        let map_center = viewport_rect.center() + self.pan_offset;
        let map_rect = egui::Rect::from_center_size(map_center, display_size);

        // Clip to viewport
        ui.set_clip_rect(viewport_rect);

        // Draw the map image
        ui.painter().image(
            texture_id,
            map_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        // Draw labels
        if self.show_labels {
            if let Some(labels) = &map.labels {
                draw_labels(ui, map_rect, map, labels, self.zoom);
            }
        }

        // Draw spawns
        if self.show_spawns {
            if let Some(spawns) = &map.spawns {
                draw_spawns(ui, map_rect, map, spawns, self.zoom);
            }
        }
    }
}

impl eframe::App for TarkovMapApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts
        ctx.input(|i| {
            // + or = to zoom in
            if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                self.zoom = (self.zoom * ZOOM_SPEED).clamp(ZOOM_MIN, ZOOM_MAX);
            }
            // - to zoom out
            if i.key_pressed(egui::Key::Minus) {
                self.zoom = (self.zoom / ZOOM_SPEED).clamp(ZOOM_MIN, ZOOM_MAX);
            }
            // 0 to reset zoom and pan
            if i.key_pressed(egui::Key::Num0) {
                self.zoom = 1.0;
                self.pan_offset = egui::Vec2::ZERO;
            }
            // L to toggle labels
            if i.key_pressed(egui::Key::L) {
                self.show_labels = !self.show_labels;
            }
        });

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Map:");

                if self.maps.is_empty() {
                    ui.label("(no maps loaded)");
                    return;
                }

                let selected_map_name = self
                    .maps
                    .get(self.selected_map)
                    .map(|map| map.name.as_str())
                    .unwrap_or("(unknown)");

                let prev_selected = self.selected_map;
                egui::ComboBox::from_id_salt("map")
                    .selected_text(selected_map_name)
                    .show_ui(ui, |ui| {
                        for (idx, map) in self.maps.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_map, idx, &map.name);
                        }
                    });

                // Reset view when map changes
                if self.selected_map != prev_selected {
                    self.zoom = 1.0;
                    self.pan_offset = egui::Vec2::ZERO;
                }

                ui.separator();

                ui.add(
                    egui::Slider::new(&mut self.zoom, ZOOM_MIN..=ZOOM_MAX)
                        .logarithmic(true)
                        .text("Zoom"),
                );

                if ui.button("Fit").clicked() {
                    self.zoom = 1.0;
                    self.pan_offset = egui::Vec2::ZERO;
                }

                ui.separator();

                ui.checkbox(&mut self.show_labels, "Labels");
                ui.checkbox(&mut self.show_spawns, "Spawns");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.load_error {
                ui.colored_label(egui::Color32::RED, err);
                ui.separator();
            }

            let Some(map) = self.selected_map().cloned() else {
                ui.label(format!(
                    "No map data. Run `cargo run --bin fetch_maps` to generate {}.",
                    MAPS_RON_PATH
                ));
                return;
            };

            ui.heading(&map.name);

            ui.horizontal_wrapped(|ui| {
                ui.label(format!("ID: {}", map.normalized_name));
                if let Some(author) = &map.author {
                    ui.label(format!("Author: {}", author));
                }
                if let Some(link) = &map.author_link {
                    ui.hyperlink(link);
                }
            });

            ui.separator();

            self.show_map(ui, ctx, &map);
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Scroll: zoom in | Drag: pan | +/-: zoom | 0: fit to screen | L: labels");
            });
        });

        self.prev_zoom = self.zoom;
    }
}

// ============================================================================
// Label rendering
// ============================================================================

/// Convert game coordinates to display position.
///
/// Maps game coordinates to the display rect using the map bounds.
/// bounds format: [[maxX, minY], [minX, maxY]]
fn game_to_display(map: &Map, map_rect: egui::Rect, game_pos: [f64; 2]) -> Option<egui::Pos2> {
    let bounds = map.bounds?;

    // Extract bounds: [[maxX, minY], [minX, maxY]]
    let min_x = bounds[1][0];
    let max_x = bounds[0][0];
    let min_y = bounds[0][1];
    let max_y = bounds[1][1];

    // Calculate position as fraction within bounds (0.0 to 1.0)
    // X axis is flipped (game coords increase right, but map image has left = max_x)
    let frac_x = (max_x - game_pos[0]) / (max_x - min_x);
    let frac_y = (game_pos[1] - min_y) / (max_y - min_y);

    // Map to display coordinates
    let display_x = map_rect.min.x + (frac_x as f32) * map_rect.width();
    let display_y = map_rect.min.y + (frac_y as f32) * map_rect.height();

    Some(egui::pos2(display_x, display_y))
}

fn draw_labels(ui: &mut egui::Ui, map_rect: egui::Rect, map: &Map, labels: &[Label], zoom: f32) {
    let painter = ui.painter();

    for label in labels {
        // Convert game coordinates to display position
        let Some(pos) = game_to_display(map, map_rect, label.position) else {
            continue;
        };

        // Skip if outside visible area (with margin)
        if !map_rect.expand(50.0).contains(pos) {
            continue;
        }

        // Calculate font size (scale with zoom, clamp to reasonable range)
        let base_size = label.size.unwrap_or(40) as f32 * 0.15;
        let font_size = (base_size * zoom).clamp(8.0, 48.0);

        // Draw text with shadow for visibility
        let font_id = egui::FontId::proportional(font_size);
        let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 220);
        let shadow_color = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180);

        let anchor = egui::Align2::CENTER_CENTER;
        let shadow_offset = egui::vec2(1.0, 1.0);

        // Shadow
        painter.text(
            pos + shadow_offset,
            anchor,
            &label.text,
            font_id.clone(),
            shadow_color,
        );
        // Main text
        painter.text(pos, anchor, &label.text, font_id, text_color);
    }
}

fn draw_spawns(ui: &mut egui::Ui, map_rect: egui::Rect, map: &Map, spawns: &[Spawn], zoom: f32) {
    let painter = ui.painter();

    for spawn in spawns {
        // Convert game coordinates to display position (use x, z for 2D position, y is height)
        let game_pos = [spawn.position[0], spawn.position[2]];
        let Some(pos) = game_to_display(map, map_rect, game_pos) else {
            continue;
        };

        // Skip if outside visible area
        if !map_rect.expand(20.0).contains(pos) {
            continue;
        }

        // Draw spawn marker (green circle for PMC spawns)
        let radius = (4.0 * zoom).clamp(3.0, 12.0);
        let fill_color = egui::Color32::from_rgb(50, 205, 50); // Lime green
        let stroke_color = egui::Color32::from_rgb(0, 100, 0); // Dark green

        painter.circle(
            pos,
            radius,
            fill_color,
            egui::Stroke::new(1.5, stroke_color),
        );
    }
}

// ============================================================================
// Utilities
// ============================================================================

fn resolve_asset_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}

async fn load_asset_bytes(path: &str) -> Result<Vec<u8>, String> {
    let resolved = resolve_asset_path(path);
    tokio::fs::read(&resolved)
        .await
        .map_err(|err| format!("read {}: {err}", resolved.display()))
}

fn load_maps(path: &str) -> Result<TarkovMaps, String> {
    let ron_string =
        std::fs::read_to_string(path).map_err(|err| format!("read maps file {path}: {err}"))?;
    ron::from_str::<TarkovMaps>(&ron_string).map_err(|err| format!("parse maps file {path}: {err}"))
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
