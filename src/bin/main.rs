#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use log::error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tarkov_map::{Label, Map, TarkovMaps};

const MAPS_RON_PATH: &str = "assets/maps.ron";

/// Min/max zoom levels (1.0 = fit to screen)
const ZOOM_MIN: f32 = 1.0;
const ZOOM_MAX: f32 = 10.0;

/// Zoom speed for mouse wheel (multiplier per scroll unit)
const ZOOM_SPEED: f32 = 1.1;

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

    /// Whether to show map labels
    show_labels: bool,

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
            show_labels: true,
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

    fn show_map(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, map: &Map) {
        let image_path = &map.image_path;
        let logical_size = egui::vec2(map.logical_size[0], map.logical_size[1]);

        // Request and poll the image
        self.request_asset(ctx, image_path);
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
        let Some(texture) = self.get_texture(ctx, image_path) else {
            ui.label("Failed to create texture");
            return;
        };
        let texture_id = texture.id();

        let scroll_id = egui::Id::new("map_scroll");
        let available_rect = ui.available_rect_before_wrap();
        let viewport_size = available_rect.size();

        // Calculate base scale to fit map in viewport at zoom 1.0
        let scale_x = viewport_size.x / logical_size.x;
        let scale_y = viewport_size.y / logical_size.y;
        let fit_scale = scale_x.min(scale_y);

        // Display size: fit to viewport, then apply zoom
        let display_size = logical_size * fit_scale * self.zoom;

        // Handle mouse wheel zoom (before scroll area consumes the events)
        let hover_pos = ui.input(|i| i.pointer.hover_pos());
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);

        if scroll_delta != 0.0 && hover_pos.map_or(false, |p| available_rect.contains(p)) {
            let zoom_factor = if scroll_delta > 0.0 {
                ZOOM_SPEED
            } else {
                1.0 / ZOOM_SPEED
            };
            let new_zoom = (self.zoom * zoom_factor).clamp(ZOOM_MIN, ZOOM_MAX);

            // Zoom towards mouse position
            if let (Some(hover), Some(scroll_state)) =
                (hover_pos, egui::scroll_area::State::load(ctx, scroll_id))
            {
                let old_offset = scroll_state.offset;

                // Mouse position relative to viewport
                let mouse_in_viewport = hover - available_rect.min;
                // Mouse position in map coordinates (old zoom)
                let mouse_in_map = old_offset + mouse_in_viewport;

                // Scale to new zoom
                let zoom_ratio = new_zoom / self.zoom;
                let new_mouse_in_map = mouse_in_map * zoom_ratio;

                // New offset to keep mouse at same position
                let new_offset = new_mouse_in_map - mouse_in_viewport;
                let new_display_size = logical_size * fit_scale * new_zoom;
                let max_offset = (new_display_size - viewport_size).max(egui::Vec2::ZERO);
                let new_offset = egui::vec2(
                    new_offset.x.clamp(0.0, max_offset.x),
                    new_offset.y.clamp(0.0, max_offset.y),
                );

                let mut new_state = scroll_state;
                new_state.offset = new_offset;
                new_state.store(ctx, scroll_id);
            }

            self.zoom = new_zoom;
        }

        // Center-based zoom adjustment (for slider zoom)
        let zoom_ratio = self.zoom / self.prev_zoom;
        let needs_adjustment = (zoom_ratio - 1.0).abs() > 0.001 && scroll_delta == 0.0;

        if needs_adjustment {
            if let Some(scroll_state) = egui::scroll_area::State::load(ctx, scroll_id) {
                let old_offset = scroll_state.offset;
                let old_center = old_offset + viewport_size * 0.5;
                let new_center = old_center * zoom_ratio;
                let new_offset = new_center - viewport_size * 0.5;
                let max_offset = (display_size - viewport_size).max(egui::Vec2::ZERO);
                let new_offset = egui::vec2(
                    new_offset.x.clamp(0.0, max_offset.x),
                    new_offset.y.clamp(0.0, max_offset.y),
                );

                let mut new_state = scroll_state;
                new_state.offset = new_offset;
                new_state.store(ctx, scroll_id);
            }
        }

        egui::ScrollArea::both()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                let (rect, response) = ui.allocate_exact_size(display_size, egui::Sense::drag());

                // Handle drag panning
                if response.dragged() {
                    if let Some(mut scroll_state) = egui::scroll_area::State::load(ctx, scroll_id) {
                        let delta = response.drag_delta();
                        scroll_state.offset -= delta;

                        // Clamp to valid range
                        let viewport_size = available_rect.size();
                        let max_offset = (display_size - viewport_size).max(egui::Vec2::ZERO);
                        scroll_state.offset = egui::vec2(
                            scroll_state.offset.x.clamp(0.0, max_offset.x),
                            scroll_state.offset.y.clamp(0.0, max_offset.y),
                        );

                        scroll_state.store(ctx, scroll_id);
                    }
                }

                // Draw the map image
                ui.painter().image(
                    texture_id,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                // Draw labels
                if self.show_labels {
                    if let Some(labels) = &map.labels {
                        draw_labels(ui, rect, map, labels, self.zoom);
                    }
                }
            });
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
            // 0 to reset zoom
            if i.key_pressed(egui::Key::Num0) {
                self.zoom = 1.0;
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

                egui::ComboBox::from_id_salt("map")
                    .selected_text(selected_map_name)
                    .show_ui(ui, |ui| {
                        for (idx, map) in self.maps.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_map, idx, &map.name);
                        }
                    });

                ui.separator();

                ui.add(
                    egui::Slider::new(&mut self.zoom, ZOOM_MIN..=ZOOM_MAX)
                        .logarithmic(true)
                        .text("Zoom"),
                );

                if ui.button("Fit").clicked() {
                    self.zoom = 1.0;
                }

                ui.label(format!("{:.0}x", self.zoom));

                ui.separator();

                ui.checkbox(&mut self.show_labels, "Labels");
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

/// Convert game coordinates to pixel position on the map image.
///
/// The map has a transform [scaleX, translateX, scaleY, translateY] that converts
/// game coordinates to SVG/image coordinates:
///   pixel_x = game_x * scaleX + translateX
///   pixel_y = game_y * scaleY + translateY
fn game_to_pixel(map: &Map, game_pos: [f64; 2]) -> egui::Pos2 {
    let (scale_x, translate_x, scale_y, translate_y) = match map.transform {
        Some([sx, tx, sy, ty]) => (sx, tx, sy, ty),
        None => (1.0, 0.0, 1.0, 0.0),
    };

    let pixel_x = game_pos[0] * scale_x + translate_x;
    let pixel_y = game_pos[1] * scale_y + translate_y;

    egui::pos2(pixel_x as f32, pixel_y as f32)
}

fn draw_labels(ui: &mut egui::Ui, map_rect: egui::Rect, map: &Map, labels: &[Label], zoom: f32) {
    let painter = ui.painter();
    let image_size = egui::vec2(map.image_size[0], map.image_size[1]);

    for label in labels {
        // Convert game coordinates to pixel position
        let pixel_pos = game_to_pixel(map, label.position);

        // Scale to current display size
        let display_x = map_rect.min.x + (pixel_pos.x / image_size.x) * map_rect.width();
        let display_y = map_rect.min.y + (pixel_pos.y / image_size.y) * map_rect.height();
        let pos = egui::pos2(display_x, display_y);

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

        let galley = painter.layout_no_wrap(label.text.clone(), font_id.clone(), text_color);

        // Handle rotation if present
        let rotation = label.rotation.unwrap_or(0.0) as f32;

        if rotation.abs() > 0.1 {
            // For rotated text, we need to use a different approach
            // Draw at anchor point with rotation
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
        } else {
            // Non-rotated text - center it
            let text_offset = egui::vec2(galley.size().x / 2.0, galley.size().y / 2.0);
            let shadow_offset = egui::vec2(1.0, 1.0);

            // Shadow
            painter.galley(
                pos - text_offset + shadow_offset,
                painter.layout_no_wrap(label.text.clone(), font_id.clone(), shadow_color),
                shadow_color,
            );
            // Main text
            painter.galley(pos - text_offset, galley, text_color);
        }
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
