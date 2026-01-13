//! UI rendering methods for the Tarkov Map application.

use crate::TarkovMapApp;
use crate::colors;
use crate::constants::{SIDEBAR_WIDTH, TITLE_BAR_HEIGHT, ZOOM_MAX, ZOOM_MIN, ZOOM_SPEED};
use crate::overlays::{draw_extracts, draw_labels, draw_player_marker, draw_spawns};
use crate::{APP_TITLE, APP_VERSION};
use eframe::egui::{self, ViewportCommand};
use tarkov_map::Map;

impl TarkovMapApp {
    /// Handles keyboard shortcuts for zoom and overlay toggles.
    pub fn handle_keyboard_input(&mut self, ctx: &egui::Context) {
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
        });
    }

    /// Renders the bottom status bar with controls hint and map author info.
    pub fn show_status_bar(&self, ctx: &egui::Context, selected_map: &Option<Map>) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Scroll: Zoom | Drag: Pan | +/-: Zoom | 0: Fit | L: Labels");

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

    /// Renders the left sidebar panel.
    pub fn show_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(SIDEBAR_WIDTH)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_sidebar_content(ui);
                });
            });
    }

    /// Renders the sidebar content: map selector and overlay toggles.
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
        Self::overlay_toggle_triangle(
            ui,
            &mut self.overlays.player_marker,
            "Player Position",
            colors::PLAYER_MARKER_FILL,
        );
    }

    /// Renders a triangle-style overlay toggle (for player marker).
    fn overlay_toggle_triangle(
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
            // Draw a small triangle pointing up
            let size = 5.0;
            let points = vec![
                center + egui::vec2(0.0, -size),
                center + egui::vec2(-size * 0.7, size * 0.5),
                center + egui::vec2(size * 0.7, size * 0.5),
            ];
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                color,
                egui::Stroke::new(1.0, color.gamma_multiply(0.5)),
            ));
            let label_response = ui
                .label(label)
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if icon_response.clicked() || label_response.clicked() {
                *value = !*value;
            }
        });
    }

    /// Renders a circle-style overlay toggle (for spawns, labels).
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

    /// Renders a rectangle-style overlay toggle (for extracts).
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

    /// Renders the central panel containing the map view.
    pub fn show_central_panel(&mut self, ctx: &egui::Context, selected_map: Option<Map>) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(map) = selected_map else {
                ui.centered_and_justified(|ui| {
                    ui.label("No map data.\nRun `cargo run --bin fetch_maps` to generate assets.");
                });
                return;
            };

            let panel_rect = ui.max_rect();
            self.show_map(ui, ctx, &map);
            self.show_zoom_controls(ctx, panel_rect);
        });
    }

    /// Renders the floating zoom controls panel.
    fn show_zoom_controls(&mut self, ctx: &egui::Context, panel_rect: egui::Rect) {
        let margin = 12.0;
        let panel_width = 160.0;
        let panel_height = 36.0;

        let anchor_pos = egui::pos2(
            panel_rect.right() - panel_width - margin,
            panel_rect.bottom() - panel_height - margin,
        );

        egui::Area::new(egui::Id::new("zoom_controls"))
            .fixed_pos(anchor_pos)
            .interactable(true)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(ui.style().visuals.window_fill.gamma_multiply(0.95))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Slider::new(&mut self.zoom, ZOOM_MIN..=ZOOM_MAX)
                                    .logarithmic(true)
                                    .show_value(false),
                            );
                            if ui.button("Fit").on_hover_text("Reset view (0)").clicked() {
                                self.reset_view();
                            }
                        });
                    });
            });
    }

    /// Renders the map image and overlays.
    fn show_map(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, map: &Map) {
        use crate::assets::AssetLoadState;

        let image_path = &map.image_path;
        let logical_size = egui::vec2(map.logical_size[0], map.logical_size[1]);

        // Check loading state - errors are shown via toasts
        match self.asset_cache.get(image_path) {
            Some(AssetLoadState::Loading(_)) | None => {
                ui.centered_and_justified(|ui| ui.spinner());
                return;
            }
            Some(AssetLoadState::Error(msg)) => {
                ui.centered_and_justified(|ui| {
                    ui.label(format!("Failed to load map: {msg}"));
                });
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

        // Draw player position marker
        if overlays.player_marker
            && let Some(player_pos) = &self.player_position
        {
            draw_player_marker(ui, map_rect, map, player_pos, self.zoom);
        }
    }

    /// Handles scroll wheel zoom, zooming towards the mouse position.
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

    /// Handles zoom changes from the slider, adjusting pan to zoom from center.
    fn handle_slider_zoom(&mut self) {
        let zoom_ratio = self.zoom / self.prev_zoom;
        if (zoom_ratio - 1.0).abs() > 0.001 {
            self.pan_offset *= zoom_ratio;
        }
    }

    /// Renders the complete custom window frame with title bar and content.
    pub fn show_custom_frame(&mut self, ctx: &egui::Context) {
        let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

        // When maximized, no border radius or stroke (like native Windows)
        let corner_radius = if is_maximized { 0.0 } else { 10.0 };
        let panel_frame = egui::Frame::new()
            .fill(ctx.style().visuals.window_fill())
            .corner_radius(corner_radius)
            .stroke(if is_maximized {
                egui::Stroke::NONE
            } else {
                ctx.style().visuals.widgets.noninteractive.fg_stroke
            })
            .outer_margin(if is_maximized { 0.0 } else { 1.0 });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                panel_frame.show(ui, |ui| {
                    let app_rect = ui.max_rect();
                    ui.expand_to_include_rect(app_rect);

                    // Title bar area
                    let title_bar_rect = {
                        let mut rect = app_rect;
                        rect.max.y = rect.min.y + TITLE_BAR_HEIGHT;
                        rect
                    };

                    // Content area (below title bar)
                    let content_rect = {
                        let mut rect = app_rect;
                        rect.min.y = title_bar_rect.max.y;
                        rect
                    };

                    // Render title bar
                    self.show_title_bar(ui, title_bar_rect, is_maximized, corner_radius);

                    // Render content in the remaining area
                    let mut content_ui =
                        ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
                    self.show_frame_content(&mut content_ui, is_maximized);
                });
            });
    }

    /// Renders the content inside the custom frame (sidebar, central panel, status bar).
    fn show_frame_content(&mut self, ui: &mut egui::Ui, is_maximized: bool) {
        let ctx = ui.ctx().clone();
        let selected_map = self.selected_map().cloned();

        // Status bar at bottom (no corner radius when maximized)
        let status_corner_radius = if is_maximized { 0 } else { 10 };
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::side_top_panel(ui.style()).corner_radius(egui::CornerRadius {
                    sw: status_corner_radius,
                    se: status_corner_radius,
                    ..Default::default()
                }),
            )
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Scroll: Zoom | Drag: Pan | +/-: Zoom | 0: Fit | L: Labels");

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(map) = &selected_map {
                            if let Some(link) = &map.author_link {
                                ui.hyperlink_to(
                                    map.author.as_deref().unwrap_or("Map author"),
                                    link,
                                );
                                ui.label("Map by:");
                            } else if let Some(author) = &map.author {
                                ui.label(format!("Map by: {author}"));
                            }
                        }
                    });
                });
            });

        // Sidebar on left
        egui::SidePanel::left("sidebar")
            .exact_width(SIDEBAR_WIDTH)
            .resizable(false)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_sidebar_content(ui);
                });
            });

        // Central panel with map
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let Some(map) = selected_map else {
                ui.centered_and_justified(|ui| {
                    ui.label("No map data.\nRun `cargo run --bin fetch_maps` to generate assets.");
                });
                return;
            };

            let panel_rect = ui.max_rect();
            self.show_map(ui, &ctx, &map);
            self.show_zoom_controls(&ctx, panel_rect);
        });
    }

    /// Renders the custom title bar with file menu, title, and window controls.
    fn show_title_bar(
        &mut self,
        ui: &mut egui::Ui,
        title_bar_rect: egui::Rect,
        is_maximized: bool,
        corner_radius: f32,
    ) {
        let painter = ui.painter();

        // Make the title bar draggable
        let title_bar_response = ui.interact(
            title_bar_rect,
            egui::Id::new("title_bar"),
            egui::Sense::click_and_drag(),
        );

        // Paint the title in the center
        let title = format!("{} v{}", APP_TITLE, APP_VERSION);
        painter.text(
            title_bar_rect.center(),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(16.0),
            ui.style().visuals.text_color(),
        );

        // Paint line under title bar
        painter.line_segment(
            [
                title_bar_rect.left_bottom() + egui::vec2(1.0, 0.0),
                title_bar_rect.right_bottom() + egui::vec2(-1.0, 0.0),
            ],
            ui.visuals().widgets.noninteractive.bg_stroke,
        );

        // Double-click to maximize/restore
        if title_bar_response.double_clicked() {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
        }

        // Drag to move window
        if title_bar_response.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }

        // File menu on the left
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(title_bar_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add_space(8.0);
                self.show_menu_bar(ui);
            },
        );

        // Window controls on the right
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(title_bar_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                Self::window_controls(ui, is_maximized, corner_radius);
            },
        );
    }

    /// Renders the menu bar (File, Help).
    fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            // File menu
            ui.menu_button("File", |ui| {
                if ui.button("Clear Settings").clicked() {
                    // Clear settings by resetting to defaults and restarting app
                    self.clear_settings_on_close = true;

                    // Spawn a new instance of the app before closing
                    if let Ok(exe_path) = std::env::current_exe() {
                        let _ = std::process::Command::new(exe_path).spawn();
                    }

                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                    ui.close();
                }

                ui.separator();

                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                    ui.close();
                }
            });

            // Help menu
            ui.menu_button("Help", |ui| {
                if ui.button("GitHub").clicked() {
                    let _ = open::that("https://github.com/teevik/tarkov-map");
                    ui.close();
                }
            });
        });
    }

    /// Renders Windows-style window control buttons (minimize, maximize/restore, close).
    fn window_controls(ui: &mut egui::Ui, is_maximized: bool, corner_radius: f32) {
        let button_width = 46.0;
        let button_height = TITLE_BAR_HEIGHT;
        let icon_color = ui.style().visuals.text_color();

        // Close button (red on hover, with corner radius to match window frame)
        let (close_rect, close_response) = ui.allocate_exact_size(
            egui::vec2(button_width, button_height),
            egui::Sense::click(),
        );
        if close_response.hovered() {
            // Only round the top-right corner to match the window frame
            let close_corner_radius = egui::CornerRadius {
                ne: corner_radius as u8,
                ..Default::default()
            };
            ui.painter().rect_filled(
                close_rect,
                close_corner_radius,
                egui::Color32::from_rgb(196, 43, 28),
            );
        }
        // Draw X icon
        let close_icon_color = if close_response.hovered() {
            egui::Color32::WHITE
        } else {
            icon_color
        };
        Self::draw_close_icon(ui.painter(), close_rect.center(), close_icon_color);
        if close_response.clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }

        // Maximize/Restore button
        let (max_rect, max_response) = ui.allocate_exact_size(
            egui::vec2(button_width, button_height),
            egui::Sense::click(),
        );
        if max_response.hovered() {
            ui.painter()
                .rect_filled(max_rect, 0.0, ui.style().visuals.widgets.hovered.bg_fill);
        }
        if is_maximized {
            Self::draw_restore_icon(ui.painter(), max_rect.center(), icon_color);
        } else {
            Self::draw_maximize_icon(ui.painter(), max_rect.center(), icon_color);
        }
        if max_response.clicked() {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
        }

        // Minimize button
        let (min_rect, min_response) = ui.allocate_exact_size(
            egui::vec2(button_width, button_height),
            egui::Sense::click(),
        );
        if min_response.hovered() {
            ui.painter()
                .rect_filled(min_rect, 0.0, ui.style().visuals.widgets.hovered.bg_fill);
        }
        Self::draw_minimize_icon(ui.painter(), min_rect.center(), icon_color);
        if min_response.clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
        }
    }

    /// Draws a close (X) icon.
    fn draw_close_icon(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
        let size = 4.5;
        let stroke = egui::Stroke::new(1.0, color);
        painter.line_segment(
            [
                center + egui::vec2(-size, -size),
                center + egui::vec2(size, size),
            ],
            stroke,
        );
        painter.line_segment(
            [
                center + egui::vec2(size, -size),
                center + egui::vec2(-size, size),
            ],
            stroke,
        );
    }

    /// Draws a maximize (square) icon.
    fn draw_maximize_icon(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
        let size = 4.5;
        let stroke = egui::Stroke::new(1.0, color);
        let rect = egui::Rect::from_center_size(center, egui::vec2(size * 2.0, size * 2.0));
        painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
    }

    /// Draws a restore (overlapping squares) icon.
    fn draw_restore_icon(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
        let size = 4.0;
        let stroke = egui::Stroke::new(1.0, color);
        // Back square (offset up-right)
        let back_rect = egui::Rect::from_min_size(
            center + egui::vec2(-size + 2.0, -size - 2.0),
            egui::vec2(size * 2.0 - 2.0, size * 2.0 - 2.0),
        );
        painter.line_segment([back_rect.left_top(), back_rect.right_top()], stroke);
        painter.line_segment([back_rect.right_top(), back_rect.right_bottom()], stroke);
        // Front square
        let front_rect = egui::Rect::from_min_size(
            center + egui::vec2(-size, -size + 2.0),
            egui::vec2(size * 2.0 - 2.0, size * 2.0 - 2.0),
        );
        painter.rect_stroke(front_rect, 0.0, stroke, egui::StrokeKind::Inside);
    }

    /// Draws a minimize (horizontal line) icon.
    fn draw_minimize_icon(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
        let size = 5.0;
        let stroke = egui::Stroke::new(1.0, color);
        painter.line_segment(
            [
                center + egui::vec2(-size, 0.0),
                center + egui::vec2(size, 0.0),
            ],
            stroke,
        );
    }
}
