#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod colors;
mod constants;
mod coordinates;
mod overlays;
mod screenshot_watcher;
mod ui;
mod updater;

use assets::{AssetCache, load_and_decode_image, load_maps};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts};
use overlays::OverlayVisibility;
use screenshot_watcher::{PlayerPosition, ScreenshotWatcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, mpsc};
use std::thread;
use tarkov_map::{Map, TarkovMaps};

const APP_ID: &str = "tarkov-map";
const APP_TITLE: &str = "Tarkov Map";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const SETTINGS_STORAGE_KEY: &str = "app_settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct AppSettings {
    schema_version: u32,
    selected_map_normalized_name: Option<String>,
    selected_layers: HashMap<String, usize>,
    auto_layer: bool,
    overlays: OverlayVisibility,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 2,
            selected_map_normalized_name: None,
            selected_layers: HashMap::new(),
            auto_layer: false,
            overlays: OverlayVisibility::default(),
        }
    }
}

/// Main application state for the Tarkov Map viewer.
pub struct TarkovMapApp {
    maps: TarkovMaps,
    selected_map: usize,
    selected_layers: HashMap<String, usize>,
    auto_layer: bool,
    zoom: f32,
    prev_zoom: f32,
    pan_offset: egui::Vec2,
    overlays: OverlayVisibility,
    asset_cache: AssetCache,
    texture_cache: HashMap<String, TextureHandle>,
    toasts: Toasts,
    updater: updater::Updater,
    screenshot_watcher: Option<ScreenshotWatcher>,
    player_position: Option<PlayerPosition>,

    /// Flag to clear settings on app close (triggered by File -> Clear Settings).
    pub clear_settings_on_close: bool,
}

impl TarkovMapApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut settings: AppSettings = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, SETTINGS_STORAGE_KEY))
            .unwrap_or_default();

        // Version 1 enabled automatic floor tracking and populated
        // manual selections merely by rendering the sidebar. Start them on the
        // main floor instead; tracking can still be enabled explicitly.
        if settings.schema_version < 2 {
            settings.selected_layers.clear();
            settings.auto_layer = false;
            settings.schema_version = 2;
        }

        let updater = updater::Updater::new(cc.egui_ctx.clone());

        let mut toasts = updater.configure_toasts(
            Toasts::new()
                .anchor(egui::Align2::RIGHT_TOP, (-10.0, 10.0))
                .direction(egui::Direction::TopDown),
        );

        let maps = match load_maps() {
            Ok(maps) => maps,
            Err(err) => {
                toasts.add(Toast {
                    kind: ToastKind::Error,
                    text: err.to_string().into(),
                    options: ToastOptions::default()
                        .duration_in_seconds(10.0)
                        .show_icon(true),
                    ..Default::default()
                });
                Vec::new()
            }
        };

        let selected_map = settings
            .selected_map_normalized_name
            .as_deref()
            .and_then(|saved_name| {
                maps.iter()
                    .position(|map| map.normalized_name == saved_name)
            })
            .unwrap_or(0);

        // Images load on demand: only the active map or floor is decoded, and
        // only once it is actually about to be rendered (see `request_image`).
        let asset_cache = AssetCache::new();

        // Initialize screenshot watcher for player position tracking
        let mut screenshot_watcher = ScreenshotWatcher::new(cc.egui_ctx.clone());
        // Get initial position from the newest screenshot
        let player_position = screenshot_watcher.as_mut().and_then(|w| w.poll());

        if screenshot_watcher.is_none() {
            log::info!("Screenshot watcher not available - player position tracking disabled");
        }

        Self {
            maps,
            selected_map,
            selected_layers: settings.selected_layers,
            auto_layer: settings.auto_layer,
            zoom: 1.0,
            prev_zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            overlays: settings.overlays,
            asset_cache,
            texture_cache: HashMap::new(),
            toasts,
            updater,
            screenshot_watcher,
            player_position,
            clear_settings_on_close: false,
        }
    }

    fn selected_map(&self) -> Option<&Map> {
        self.maps.get(self.selected_map)
    }

    /// Requests a demand-driven load for `path` if it has not been requested
    /// yet. Decoding happens on a background thread that repaints when done.
    fn request_image(&mut self, path: &str, ctx: &egui::Context) {
        let ctx = ctx.clone();
        let path_to_load = path.to_string();
        self.asset_cache.request(path, move || {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let result = load_and_decode_image(&path_to_load);
                let _ = tx.send(result);
                ctx.request_repaint();
            });
            rx
        });
    }

    /// Polls loading assets and creates textures for freshly decoded ones,
    /// releasing the decoded pixel buffers once the texture is on the GPU.
    fn poll_all_assets(&mut self, ctx: &egui::Context) {
        // Show toasts for any decode failures that surfaced this frame.
        for err in self.asset_cache.poll() {
            self.toasts.add(Toast {
                kind: ToastKind::Error,
                text: err.into(),
                options: ToastOptions::default()
                    .duration_in_seconds(8.0)
                    .show_icon(true),
                ..Default::default()
            });
        }

        // Upload decoded images to textures. `take_decoded` moves the pixel
        // buffer out of the cache, and it is dropped at the end of each
        // iteration once the texture has been created, so no decoded copy is
        // retained after upload.
        for path in self.asset_cache.pending_uploads() {
            let Some(decoded) = self.asset_cache.take_decoded(&path) else {
                continue;
            };

            // A texture may already exist (e.g. re-decoded after an eviction);
            // the decoded buffer is still dropped here, releasing it.
            if self.texture_cache.contains_key(&path) {
                continue;
            }

            let image = ColorImage::from_rgba_unmultiplied(
                [decoded.width as usize, decoded.height as usize],
                &decoded.pixels,
            );
            let texture = ctx.load_texture(&path, image, TextureOptions::LINEAR);
            self.texture_cache.insert(path, texture);
        }
    }

    fn get_texture(&self, path: &str) -> Option<&TextureHandle> {
        self.texture_cache.get(path)
    }

    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan_offset = egui::Vec2::ZERO;
    }

    /// Polls the screenshot watcher for player position updates.
    fn poll_player_position(&mut self) {
        if let Some(watcher) = &mut self.screenshot_watcher
            && let Some(position) = watcher.poll()
        {
            self.player_position = Some(position);
        }
    }
}

impl eframe::App for TarkovMapApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array() // Don't paint behind rounded corners
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_all_assets(ctx);
        self.poll_player_position();
        self.handle_keyboard_input(ctx);
        self.updater.poll(ctx, &mut self.toasts);

        // Render custom window frame with title bar
        self.show_custom_frame(ctx);

        self.prev_zoom = self.zoom;

        // Show toasts
        self.toasts.show(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // If clear settings was requested, save default settings
        if self.clear_settings_on_close {
            eframe::set_value(storage, SETTINGS_STORAGE_KEY, &AppSettings::default());
            return;
        }

        let selected_map_normalized_name = self
            .maps
            .get(self.selected_map)
            .map(|map| map.normalized_name.clone());

        let settings = AppSettings {
            selected_map_normalized_name,
            selected_layers: self.selected_layers.clone(),
            auto_layer: self.auto_layer,
            overlays: self.overlays,
            ..Default::default()
        };

        eframe::set_value(storage, SETTINGS_STORAGE_KEY, &settings);
    }
}

fn load_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../../../assets/tarkov-map-icon.ico");
    let icon_dir =
        ico::IconDir::read(std::io::Cursor::new(icon_bytes)).expect("Failed to read icon");
    let entry = &icon_dir.entries()[2];
    let image = entry.decode().expect("Failed to decode icon");
    egui::IconData {
        rgba: image.rgba_data().to_vec(),
        width: image.width(),
        height: image.height(),
    }
}

fn main() -> eframe::Result {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_decorations(false) // Hide OS window decorations for custom title bar
            .with_transparent(true) // Enable transparency for rounded corners
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(Arc::new(load_icon())),
        ..Default::default()
    };

    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(TarkovMapApp::new(cc)))),
    )
}
