//! UI rendering methods for the Tarkov Map application.

use crate::{MapTransition, MapTransitionPhase, OutgoingMap, TarkovMapApp};
use crate::colors;
use crate::constants::{
    CENTER_ZOOM, FRESH_FIX_MAX_AGE, MAP_PLACEHOLDER_DELAY, MAP_REVEAL_DURATION,
    POINTS_PER_SCROLL_NOTCH, SIDEBAR_WIDTH, TITLE_BAR_HEIGHT, ZOOM_MAX, ZOOM_MIN, ZOOM_SPEED,
};
use crate::coordinates::game_to_display;
use crate::overlays::{draw_extracts, draw_labels, draw_player_marker, draw_spawns};
use crate::screenshot_watcher::ScreenshotWatcher;
use crate::{APP_TITLE, APP_VERSION};
use eframe::egui::{self, ViewportCommand};
use std::time::{Duration, Instant};
use tarkov_map::Map;

/// Formats a fix age for the position card: "just now", "12 s ago", "5 min ago", "2 h ago".
fn format_age(age: Duration) -> String {
    let secs = age.as_secs();
    match secs {
        0..=2 => "just now".to_string(),
        3..=59 => format!("{secs} s ago"),
        60..=3599 => format!("{} min ago", secs / 60),
        _ => format!("{} h ago", secs / 3600),
    }
}

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
            if i.key_pressed(egui::Key::C) {
                self.center_on_player = true;
            }
        });
    }

    /// Renders the sidebar content: position tracking, map selector, overlays.
    fn show_sidebar_content(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);

        self.show_position_card(ui);

        Self::section_header(ui, "Map");

        if self.maps.is_empty() {
            ui.weak("No maps loaded");
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

        Self::section_header(ui, "Overlays");

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
            "Player marker",
            colors::PLAYER_MARKER_FILL,
        );
    }

    /// A quiet uppercase eyebrow that separates sidebar sections.
    fn section_header(ui: &mut egui::Ui, title: &str) {
        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .size(11.0)
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(2.0);
    }

    /// The position card: is tracking working and how fresh is the fix.
    fn show_position_card(&mut self, ui: &mut egui::Ui) {
        let position = self.player_position;
        let demo = self.demo.is_some();
        let watching = self.screenshot_watcher.is_some() || demo;

        // Status line: coloured dot + short state, age on the right. The age
        // carries the status colour once the fix is stale, so "old" is visible
        // at a glance without reading.
        let (dot_color, state, age_text): (egui::Color32, &str, Option<String>) = match position {
            Some(pos) => {
                let age = pos.age();
                let fresh = age < FRESH_FIX_MAX_AGE;
                (
                    if fresh {
                        colors::TRACKING_LIVE
                    } else {
                        colors::TRACKING_STALE
                    },
                    match (fresh, demo) {
                        (_, true) => "Demo",
                        (true, false) => "Live",
                        (false, false) => "Stale",
                    },
                    Some(format_age(age)),
                )
            }
            None if watching => (colors::TRACKING_STALE, "Waiting", None),
            None => (colors::TRACKING_OFF, "Not tracking", None),
        };
        let stale = position.is_some_and(|pos| pos.age() >= FRESH_FIX_MAX_AGE);

        let card = egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::symmetric(10, 8))
            .corner_radius(6);
        card.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 5.0, dot_color);
                ui.strong(state);
                if let Some(age_text) = age_text {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut text = egui::RichText::new(age_text);
                        text = if stale {
                            text.color(colors::TRACKING_STALE)
                        } else {
                            text.weak()
                        };
                        ui.label(text);
                    });
                }
            });

            match position {
                Some(pos) => {
                    ui.add_space(2.0);
                    Self::coordinate_row(ui, pos.position);
                }
                None if watching => {
                    ui.weak("Take a screenshot in raid to place your marker.");
                }
                None => {
                    ui.weak("Screenshots folder not found:");
                    let path = ScreenshotWatcher::screenshots_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "Documents folder unavailable".to_string());
                    ui.add(
                        egui::Label::new(egui::RichText::new(path).monospace().size(10.5).weak())
                            .wrap(),
                    );
                }
            }
        });

        // The age readout ticks; keep it honest without spinning the CPU.
        if position.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    /// Labelled X / Y / Z readout in even columns.
    fn coordinate_row(ui: &mut egui::Ui, [x, y, z]: [f64; 3]) {
        let weak = ui.visuals().weak_text_color();
        let axes = [("X", x), ("Y", y), ("Z", z)];
        ui.columns(3, |columns| {
            for (column, (axis, value)) in columns.iter_mut().zip(axes) {
                column
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(egui::RichText::new(axis).size(10.5).color(weak));
                        ui.label(
                            egui::RichText::new(format!("{value:.1}"))
                                .monospace()
                                .size(12.0),
                        );
                    })
                    .response
                    .on_hover_text("Game coordinates; Y is height.");
            }
        });
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
                egui::Stroke::new(1.0_f32, color.gamma_multiply(0.5)),
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
            ui.painter().circle_stroke(
                center,
                5.0,
                egui::Stroke::new(1.0_f32, egui::Color32::GRAY),
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
                egui::Stroke::new(1.0_f32, color.gamma_multiply(0.5)),
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

    /// Renders the floating zoom controls panel.
    fn show_zoom_controls(&mut self, ctx: &egui::Context, panel_rect: egui::Rect) {
        let margin = 12.0;
        let panel_width = 220.0;
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
                            if ui
                                .add_enabled(
                                    self.player_position.is_some(),
                                    egui::Button::new("Center"),
                                )
                                .on_hover_text("Center on player (C)")
                                .clicked()
                            {
                                self.center_on_player = true;
                            }
                        });
                    });
            });
    }

    /// Starts (or continues) the transition into `image_path`, capturing the
    /// last fully drawn map as the outgoing side of the crossfade when the
    /// image actually changes.
    fn begin_transition(&mut self, image_path: &str, phase: MapTransitionPhase) -> Instant {
        let now = Instant::now();
        match &mut self.map_transition {
            Some(t) if t.path == image_path && t.phase == phase => t.since,
            Some(t) if t.path == image_path => {
                // Loading → Reveal for the same image: keep the outgoing map,
                // restart the clock for the fade.
                t.phase = phase;
                t.since = now;
                now
            }
            _ => {
                let outgoing = self
                    .last_drawn_map
                    .clone()
                    .filter(|o| o.path != image_path && self.texture_cache.contains_key(&o.path));
                self.map_transition = Some(MapTransition {
                    path: image_path.to_owned(),
                    since: now,
                    phase,
                    outgoing,
                });
                now
            }
        }
    }

    /// Paints the outgoing map fitted to `viewport_rect` at `opacity`.
    fn paint_outgoing(&self, ui: &egui::Ui, viewport_rect: egui::Rect, opacity: f32) {
        let Some(outgoing) = self.map_transition.as_ref().and_then(|t| t.outgoing.as_ref()) else {
            return;
        };
        let Some((texture_id, uv)) = self.get_texture(&outgoing.path) else {
            return;
        };
        let fit = (viewport_rect.width() / outgoing.logical_size.x)
            .min(viewport_rect.height() / outgoing.logical_size.y);
        let rect = egui::Rect::from_center_size(viewport_rect.center(), outgoing.logical_size * fit);
        ui.painter().with_clip_rect(viewport_rect).image(
            texture_id,
            rect,
            uv,
            egui::Color32::WHITE.gamma_multiply(opacity),
        );
    }

    /// Shown while the selected map's texture is not ready. The outgoing map
    /// stays on screen and dims a little so the click registers; only a load
    /// that drags on gets a quiet "Loading …" placeholder on top, so the
    /// common fast path never flashes.
    fn show_map_loading(&mut self, ui: &mut egui::Ui, map: &Map, image_path: &str) {
        let started = self.begin_transition(image_path, MapTransitionPhase::Loading);
        let elapsed = Instant::now().duration_since(started);

        let (viewport_rect, _) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
        let dim_t = (elapsed.as_secs_f32() / MAP_PLACEHOLDER_DELAY.as_secs_f32()).min(1.0);
        self.paint_outgoing(ui, viewport_rect, 1.0 - 0.6 * dim_t);

        if elapsed >= MAP_PLACEHOLDER_DELAY {
            ui.painter().text(
                viewport_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Loading {}…", map.name),
                egui::FontId::proportional(16.0),
                ui.visuals().weak_text_color(),
            );
        } else {
            // Keep dimming, then wake once the delay elapses.
            ui.ctx().request_repaint();
        }
    }

    /// Crossfade progress for the map this frame: ramps 0 → 1 over
    /// [`MAP_REVEAL_DURATION`] after `image_path` becomes ready, painting the
    /// outgoing map underneath at the complementary opacity. Returns the
    /// incoming map's opacity; the outgoing map is released once it hits 1.
    fn map_reveal_opacity(
        &mut self,
        ui: &egui::Ui,
        viewport_rect: egui::Rect,
        image_path: &str,
    ) -> f32 {
        let since = self.begin_transition(image_path, MapTransitionPhase::Reveal);
        let t = Instant::now().duration_since(since).as_secs_f32()
            / MAP_REVEAL_DURATION.as_secs_f32();

        if t < 1.0 {
            // Ease-out: fast start, gentle landing.
            let opacity = 1.0 - (1.0 - t).powi(2);
            self.paint_outgoing(ui, viewport_rect, 1.0 - opacity);
            ui.ctx().request_repaint();
            opacity
        } else {
            if let Some(t) = &mut self.map_transition {
                t.outgoing = None;
            }
            1.0
        }
    }

    /// Renders the map image and overlays.
    fn show_map(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, map: &Map) {
        use crate::assets::AssetLoadState;

        let image_path = map.image_path.clone();
        let logical_size = egui::vec2(map.logical_size[0], map.logical_size[1]);

        // Demand-driven: request the active image the first time it is needed.
        // Retention is active-image-only; anything else was freed in `logic`.
        self.request_image(&image_path, ctx);

        // Check loading state - errors are shown via toasts
        match self.asset_cache.state(&image_path) {
            Some(AssetLoadState::Loading(_)) | Some(AssetLoadState::Decoded(_)) | None => {
                self.show_map_loading(ui, map, &image_path);
                return;
            }
            Some(AssetLoadState::Error(msg)) => {
                ui.centered_and_justified(|ui| {
                    ui.label(format!("Failed to load map: {msg}"));
                });
                return;
            }
            Some(AssetLoadState::Uploaded) => {}
        }

        let Some((texture_id, texture_uv)) = self.get_texture(&image_path) else {
            ui.label("Failed to create texture");
            return;
        };

        let (viewport_rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let viewport_size = viewport_rect.size();

        // Crossfade: the outgoing map is painted underneath, fading out, while
        // everything painted below (map and overlays) fades in.
        let opacity = self.map_reveal_opacity(ui, viewport_rect, &image_path);
        ui.multiply_opacity(opacity);
        self.last_drawn_map = Some(OutgoingMap {
            path: image_path.clone(),
            logical_size,
        });

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

        // Center on the player when asked (Center button or C). From the fit
        // view, zoom in so centering means something; otherwise keep whatever
        // zoom the user chose.
        if self.center_on_player {
            self.center_on_player = false;
            if let Some(player) = self.player_position {
                if self.zoom <= ZOOM_MIN {
                    self.zoom = CENTER_ZOOM;
                }
                let display_size = logical_size * fit_scale * self.zoom;
                let map_rect = egui::Rect::from_center_size(
                    viewport_rect.center() + self.pan_offset,
                    display_size,
                );
                let game_pos = [player.position[0], player.position[2]];
                if let Some(player_pos) = game_to_display(map, map_rect, game_pos) {
                    self.pan_offset += viewport_rect.center() - player_pos;
                }
            }
        }

        let display_size = logical_size * fit_scale * self.zoom;
        let map_center = viewport_rect.center() + self.pan_offset;
        let map_rect = egui::Rect::from_center_size(map_center, display_size);

        ui.set_clip_rect(viewport_rect);

        // Draw map image
        ui.painter()
            .image(texture_id, map_rect, texture_uv, egui::Color32::WHITE);

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
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);

        if scroll_delta == 0.0 || !hover_pos.is_some_and(|p| viewport_rect.contains(p)) {
            return false;
        }

        // The smoothed delta spreads one wheel notch over a few frames;
        // scaling the zoom factor by magnitude makes the per-frame factors
        // multiply back up to one ZOOM_SPEED step per notch.
        let zoom_factor = ZOOM_SPEED.powf(scroll_delta / POINTS_PER_SCROLL_NOTCH);
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
    /// Expects the root viewport `Ui` (no margin or background), as handed to
    /// [`eframe::App::ui`].
    pub fn show_custom_frame(&mut self, ui: &mut egui::Ui) {
        let is_maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));

        // When maximized, no border radius or stroke (like native Windows)
        let corner_radius = if is_maximized { 0.0 } else { 10.0 };
        let panel_frame = egui::Frame::new()
            .fill(ui.global_style().visuals.window_fill())
            .corner_radius(corner_radius)
            .stroke(if is_maximized {
                egui::Stroke::NONE
            } else {
                ui.global_style().visuals.widgets.noninteractive.fg_stroke
            })
            .outer_margin(if is_maximized { 0.0 } else { 1.0 });

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
            let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
            self.show_frame_content(&mut content_ui, is_maximized);
        });
    }

    /// Renders the content inside the custom frame (sidebar, central panel, status bar).
    fn show_frame_content(&mut self, ui: &mut egui::Ui, is_maximized: bool) {
        let ctx = ui.ctx().clone();
        let selected_map = self.selected_map().cloned();

        // Status bar at bottom (no corner radius when maximized)
        let status_corner_radius = if is_maximized { 0 } else { 10 };
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::side_top_panel(ui.style()).corner_radius(egui::CornerRadius {
                    sw: status_corner_radius,
                    se: status_corner_radius,
                    ..Default::default()
                }),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.weak(
                        "Scroll: Zoom · Drag: Pan · +/−: Zoom · 0: Fit · C: Center · L: Labels",
                    );

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
        egui::Panel::left("sidebar")
            .exact_size(SIDEBAR_WIDTH)
            .resizable(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_sidebar_content(ui);
                });
            });

        // Central panel with map
        egui::CentralPanel::default().show(ui, |ui| {
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
        let stroke = egui::Stroke::new(1.0_f32, color);
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
        let stroke = egui::Stroke::new(1.0_f32, color);
        let rect = egui::Rect::from_center_size(center, egui::vec2(size * 2.0, size * 2.0));
        painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
    }

    /// Draws a restore (overlapping squares) icon.
    fn draw_restore_icon(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
        let size = 4.0;
        let stroke = egui::Stroke::new(1.0_f32, color);
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
        let stroke = egui::Stroke::new(1.0_f32, color);
        painter.line_segment(
            [
                center + egui::vec2(-size, 0.0),
                center + egui::vec2(size, 0.0),
            ],
            stroke,
        );
    }
}
