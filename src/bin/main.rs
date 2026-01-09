#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use log::error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tarkov_map::{Extract, Label, Map, Spawn, TarkovMaps};

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

/// Sidebar width when open
const SIDEBAR_WIDTH: f32 = 200.0;

struct TarkovMapApp {
    maps: TarkovMaps,
    load_error: Option<String>,

    selected_map: usize,
    zoom: f32,
    prev_zoom: f32,

    /// Pan offset in display coordinates
    pan_offset: egui::Vec2,

    /// Whether the sidebar is open
    sidebar_open: bool,

    /// Whether to show map labels
    show_labels: bool,

    /// Whether to show PMC spawns
    show_spawns: bool,

    /// Whether to show PMC extracts
    show_pmc_extracts: bool,

    /// Whether to show Scav extracts
    show_scav_extracts: bool,

    /// Whether to show Shared extracts
    show_shared_extracts: bool,

    /// Raw asset bytes cache
    asset_cache: HashMap<String, AssetLoadState>,
    /// PNG texture cache
    texture_cache: HashMap<String, TextureHandle>,

    runtime: tokio::runtime::Runtime,
}

impl TarkovMapApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let maps_path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), MAPS_RON_PATH);

        let (maps, load_error) = match load_maps(&maps_path) {
            Ok(maps) => (maps, None),
            Err(err) => (Vec::new(), Some(err)),
        };

        let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");

        // Preload all map images in the background
        let mut asset_cache = HashMap::new();
        for map in &maps {
            let (tx, rx) = mpsc::channel();
            let ctx = cc.egui_ctx.clone();
            let asset_path = map.image_path.clone();

            runtime.spawn(async move {
                let result = load_asset_bytes(&asset_path).await;
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
            show_labels: false,
            show_spawns: true,
            show_pmc_extracts: true,
            show_scav_extracts: true,
            show_shared_extracts: true,
            asset_cache,
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

        // Draw extracts
        if let Some(extracts) = &map.extracts {
            draw_extracts(
                ui,
                map_rect,
                map,
                extracts,
                self.zoom,
                self.show_pmc_extracts,
                self.show_scav_extracts,
                self.show_shared_extracts,
            );
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
            // Tab to toggle sidebar
            if i.key_pressed(egui::Key::Tab) {
                self.sidebar_open = !self.sidebar_open;
            }
        });

        let selected_map = self.selected_map().cloned();

        // Top bar - minimal with burger menu and zoom
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Burger menu button
                let burger_text = if self.sidebar_open { "<<" } else { ">>" };
                if ui
                    .button(burger_text)
                    .on_hover_text("Toggle sidebar (Tab)")
                    .clicked()
                {
                    self.sidebar_open = !self.sidebar_open;
                }

                ui.separator();

                // Map name display
                if let Some(map) = &selected_map {
                    ui.strong(&map.name);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Fit").on_hover_text("Reset zoom (0)").clicked() {
                        self.zoom = 1.0;
                        self.pan_offset = egui::Vec2::ZERO;
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

        // Bottom status bar
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Hints on the left
                ui.label(
                    "Scroll: Zoom | Drag: Pan | +/-: Zoom | 0: Fit | L: Labels | Tab: Sidebar",
                );

                // Credits on the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(map) = &selected_map {
                        if let Some(link) = &map.author_link {
                            ui.hyperlink_to(map.author.as_deref().unwrap_or("Map author"), link);
                            ui.label("Map by:");
                        } else if let Some(author) = &map.author {
                            ui.label(format!("Map by: {}", author));
                        }
                    }
                });
            });
        });

        // Sidebar
        if self.sidebar_open {
            egui::SidePanel::left("sidebar")
                .exact_width(SIDEBAR_WIDTH)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.show_sidebar(ui);
                    });
                });
        }

        // Main map area
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.load_error {
                ui.colored_label(egui::Color32::RED, err);
                ui.separator();
            }

            let Some(map) = selected_map else {
                ui.centered_and_justified(|ui| {
                    ui.label(format!(
                        "No map data.\nRun `cargo run --bin fetch_maps` to generate {}.",
                        MAPS_RON_PATH
                    ));
                });
                return;
            };

            self.show_map(ui, ctx, &map);
        });

        self.prev_zoom = self.zoom;
    }
}

impl TarkovMapApp {
    fn show_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);

        // ===== Maps Section =====
        ui.strong("Maps");
        ui.separator();

        if self.maps.is_empty() {
            ui.label("No maps loaded");
        } else {
            let prev_selected = self.selected_map;
            for (idx, map) in self.maps.iter().enumerate() {
                let is_selected = self.selected_map == idx;
                if ui.selectable_label(is_selected, &map.name).clicked() {
                    self.selected_map = idx;
                }
            }

            // Reset view when map changes
            if self.selected_map != prev_selected {
                self.zoom = 1.0;
                self.pan_offset = egui::Vec2::ZERO;
            }
        }

        ui.add_space(12.0);

        // ===== OVERLAYS SECTION =====
        ui.strong("Overlays");
        ui.separator();

        // Labels toggle with white circle indicator
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_labels, "");
            let (rect, icon_response) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
            let center = rect.center();
            ui.painter()
                .circle_filled(center, 5.0, egui::Color32::WHITE);
            ui.painter()
                .circle_stroke(center, 5.0, egui::Stroke::new(1.0, egui::Color32::GRAY));
            let label_response = ui
                .label("Labels")
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if icon_response.clicked() || label_response.clicked() {
                self.show_labels = !self.show_labels;
            }
        });

        // PMC Spawns toggle with color indicator
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_spawns, "");
            let (rect, icon_response) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
            let center = rect.center();
            ui.painter()
                .circle_filled(center, 5.0, egui::Color32::from_rgb(50, 205, 50));
            ui.painter().circle_stroke(
                center,
                5.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 100, 0)),
            );
            let label_response = ui
                .label("PMC Spawns")
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if icon_response.clicked() || label_response.clicked() {
                self.show_spawns = !self.show_spawns;
            }
        });

        // PMC Extracts toggle with color indicator
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_pmc_extracts, "");
            let (rect, icon_response) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 2.0, egui::Color32::from_rgb(65, 105, 225));
            ui.painter().rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(25, 25, 112)),
                egui::StrokeKind::Inside,
            );
            let label_response = ui
                .label("PMC Extracts")
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if icon_response.clicked() || label_response.clicked() {
                self.show_pmc_extracts = !self.show_pmc_extracts;
            }
        });

        // Scav Extracts toggle with color indicator
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_scav_extracts, "");
            let (rect, icon_response) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 2.0, egui::Color32::from_rgb(255, 165, 0));
            ui.painter().rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(139, 69, 19)),
                egui::StrokeKind::Inside,
            );
            let label_response = ui
                .label("Scav Extracts")
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if icon_response.clicked() || label_response.clicked() {
                self.show_scav_extracts = !self.show_scav_extracts;
            }
        });

        // Shared Extracts toggle with color indicator
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_shared_extracts, "");
            let (rect, icon_response) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 2.0, egui::Color32::from_rgb(186, 85, 211));
            ui.painter().rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(75, 0, 130)),
                egui::StrokeKind::Inside,
            );
            let label_response = ui
                .label("Shared Extracts")
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if icon_response.clicked() || label_response.clicked() {
                self.show_shared_extracts = !self.show_shared_extracts;
            }
        });
    }
}

// ============================================================================
// Label rendering
// ============================================================================

/// Apply 2D rotation to a point.
fn rotate_point(x: f64, y: f64, angle_deg: f64) -> (f64, f64) {
    if angle_deg == 0.0 {
        return (x, y);
    }
    let angle_rad = angle_deg.to_radians();
    let cos_angle = angle_rad.cos();
    let sin_angle = angle_rad.sin();
    (x * cos_angle - y * sin_angle, x * sin_angle + y * cos_angle)
}

/// Convert game coordinates to display position.
///
/// The coordinate system transformation follows the official tarkov-dev implementation:
/// 1. Apply coordinate rotation (rotate game coords by coordinateRotation degrees)
/// 2. Map the rotated coordinates to the image using the rotated bounds
///
/// The bounds define the world coordinate extent that maps to the image.
/// After rotation, we find the new extent and normalize coordinates within it.
fn game_to_display(map: &Map, map_rect: egui::Rect, game_pos: [f64; 2]) -> Option<egui::Pos2> {
    let bounds = map.bounds?;
    let rotation = map.coordinate_rotation.unwrap_or(0.0);

    // Apply rotation to the game coordinates
    let (rotated_x, rotated_y) = rotate_point(game_pos[0], game_pos[1], rotation);

    // For 270° rotation maps with transform, use the transform-based approach
    // This handles maps like Labs and Labyrinth where the SVG has padding/margins
    // and the transform accounts for this offset
    if rotation == 270.0 {
        if let Some(transform) = map.transform {
            let scale_x = transform[0];
            let margin_x = transform[1];
            let scale_y = -transform[2]; // Negated as per tarkov-dev
            let margin_y = transform[3];

            // Apply Leaflet transformation: svg_coord = scale * rotated_coord + margin
            let svg_x = scale_x * rotated_x + margin_x;
            let svg_y = scale_y * rotated_y + margin_y;

            // Normalize to [0, 1] using image_size (the actual SVG dimensions)
            // The SVG has padding, and the transform maps game coords to SVG coords
            let frac_x = svg_x / map.image_size[0] as f64;
            let frac_y = svg_y / map.image_size[1] as f64;

            // Map to display coordinates
            let display_x = map_rect.min.x + (frac_x as f32) * map_rect.width();
            let display_y = map_rect.min.y + (frac_y as f32) * map_rect.height();

            return Some(egui::pos2(display_x, display_y));
        }
    }

    // Rotate all four corners of the bounds to find the rotated extent
    // bounds format: [[maxX, minY], [minX, maxY]]
    let corners = [
        (bounds[0][0], bounds[0][1]), // (maxX, minY)
        (bounds[0][0], bounds[1][1]), // (maxX, maxY)
        (bounds[1][0], bounds[0][1]), // (minX, minY)
        (bounds[1][0], bounds[1][1]), // (minX, maxY)
    ];

    let rotated_corners: Vec<(f64, f64)> = corners
        .iter()
        .map(|(x, y)| rotate_point(*x, *y, rotation))
        .collect();

    // Find the bounding box of rotated corners
    let rotated_min_x = rotated_corners
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::INFINITY, f64::min);
    let rotated_max_x = rotated_corners
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max);
    let rotated_min_y = rotated_corners
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min);
    let rotated_max_y = rotated_corners
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);

    // Calculate the bounds range
    let bounds_width = rotated_max_x - rotated_min_x;
    let bounds_height = rotated_max_y - rotated_min_y;

    // Normalize the rotated point within the rotated bounds to [0, 1]
    let frac_x = (rotated_x - rotated_min_x) / bounds_width;
    // Y axis is inverted (as per Leaflet's negated scaleY in the official implementation)
    let frac_y = (rotated_max_y - rotated_y) / bounds_height;

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

fn draw_extracts(
    ui: &mut egui::Ui,
    map_rect: egui::Rect,
    map: &Map,
    extracts: &[Extract],
    zoom: f32,
    show_pmc: bool,
    show_scav: bool,
    show_shared: bool,
) {
    let painter = ui.painter();

    for extract in extracts {
        // Determine if this extract should be shown based on faction
        let faction = extract.faction.to_lowercase();
        let is_pmc = faction == "pmc";
        let is_scav = faction == "scav";
        let is_shared = faction == "shared";

        // Skip if not matching current filter
        if is_pmc && !show_pmc {
            continue;
        }
        if is_scav && !show_scav {
            continue;
        }
        if is_shared && !show_shared {
            continue;
        }

        // Get position (skip if no position data)
        let Some(position) = extract.position else {
            continue;
        };

        // Convert game coordinates to display position (use x, z for 2D position, y is height)
        let game_pos = [position[0], position[2]];
        let Some(pos) = game_to_display(map, map_rect, game_pos) else {
            continue;
        };

        // Skip if outside visible area
        if !map_rect.expand(20.0).contains(pos) {
            continue;
        }

        // Choose colors based on faction
        let (fill_color, stroke_color) = if is_pmc {
            (
                egui::Color32::from_rgb(65, 105, 225), // Royal blue
                egui::Color32::from_rgb(25, 25, 112),  // Midnight blue
            )
        } else if is_scav {
            (
                egui::Color32::from_rgb(255, 165, 0), // Orange
                egui::Color32::from_rgb(139, 69, 19), // Saddle brown
            )
        } else {
            // Shared extracts
            (
                egui::Color32::from_rgb(186, 85, 211), // Medium orchid (purple)
                egui::Color32::from_rgb(75, 0, 130),   // Indigo
            )
        };

        // Draw extract marker (square shape, double size of spawns)
        let size = (12.0 * zoom).clamp(8.0, 32.0);
        let rect = egui::Rect::from_center_size(pos, egui::vec2(size, size));

        painter.rect_filled(rect, 2.0, fill_color);
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(2.0, stroke_color),
            egui::StrokeKind::Outside,
        );

        // Draw extract name label (always visible)
        let font_size = (6.0 * zoom).clamp(9.0, 18.0);
        let font_id = egui::FontId::proportional(font_size);
        let text_pos = pos + egui::vec2(0.0, -size / 2.0 - 4.0);

        // Shadow
        painter.text(
            text_pos + egui::vec2(1.0, 1.0),
            egui::Align2::CENTER_BOTTOM,
            &extract.name,
            font_id.clone(),
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
        );
        // Main text
        painter.text(
            text_pos,
            egui::Align2::CENTER_BOTTOM,
            &extract.name,
            font_id,
            egui::Color32::WHITE,
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
