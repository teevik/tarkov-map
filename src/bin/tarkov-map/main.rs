#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod colors;
mod constants;
mod coordinates;
mod overlays;
mod screenshot_watcher;
mod ui;
mod updater;

use assets::{AssetLoadState, load_and_decode_image, load_maps};
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
const SETTINGS_STORAGE_KEY: &str = "app_settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct AppSettings {
    schema_version: u32,
    selected_map_normalized_name: Option<String>,
    overlays: OverlayVisibility,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            selected_map_normalized_name: None,
            overlays: OverlayVisibility::default(),
        }
    }
}

/// Main application state for the Tarkov Map viewer.
pub struct TarkovMapApp {
    maps: TarkovMaps,
    selected_map: usize,
    zoom: f32,
    prev_zoom: f32,
    pan_offset: egui::Vec2,
    overlays: OverlayVisibility,
    asset_cache: HashMap<String, AssetLoadState>,
    texture_cache: HashMap<String, TextureHandle>,
    toasts: Toasts,
    updater: updater::Updater,
    screenshot_watcher: Option<ScreenshotWatcher>,

    player_position: Option<PlayerPosition>,
}

impl TarkovMapApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings: AppSettings = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, SETTINGS_STORAGE_KEY))
            .unwrap_or_default();

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

        let mut asset_cache = HashMap::new();

        // Preload all map images in background threads
        for map in &maps {
            let (tx, rx) = mpsc::channel();
            let ctx = cc.egui_ctx.clone();
            let asset_path = map.image_path.clone();

            thread::spawn(move || {
                let result = load_and_decode_image(&asset_path);
                let _ = tx.send(result);
                ctx.request_repaint();
            });

            asset_cache.insert(map.image_path.clone(), AssetLoadState::Loading(rx));
        }

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
        }
    }

    fn selected_map(&self) -> Option<&Map> {
        self.maps.get(self.selected_map)
    }

    /// Polls all loading assets and creates textures for ready ones.
    fn poll_all_assets(&mut self, ctx: &egui::Context) {
        let mut updates: Vec<(String, AssetLoadState)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for (path, state) in &mut self.asset_cache {
            if let AssetLoadState::Loading(rx) = state {
                match rx.try_recv() {
                    Ok(Ok(decoded)) => {
                        updates.push((path.clone(), AssetLoadState::Ready(decoded)));
                    }
                    Ok(Err(err)) => {
                        let msg = format!("{}: {}", path, err);
                        errors.push(msg.clone());
                        updates.push((path.clone(), AssetLoadState::Error(msg)));
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let msg = format!("{}: channel disconnected", path);
                        errors.push(msg.clone());
                        updates.push((path.clone(), AssetLoadState::Error(msg)));
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        }

        for (path, new_state) in updates {
            self.asset_cache.insert(path, new_state);
        }

        // Show toasts for any errors that occurred
        for err in errors {
            self.toasts.add(Toast {
                kind: ToastKind::Error,
                text: err.into(),
                options: ToastOptions::default()
                    .duration_in_seconds(8.0)
                    .show_icon(true),
                ..Default::default()
            });
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_all_assets(ctx);
        self.poll_player_position();
        self.handle_keyboard_input(ctx);
        self.updater.poll(ctx, &mut self.toasts);

        let selected_map = self.selected_map().cloned();

        self.show_status_bar(ctx, &selected_map);
        self.show_sidebar(ctx);
        self.show_central_panel(ctx, selected_map);

        self.prev_zoom = self.zoom;

        // Show toasts
        self.toasts.show(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let selected_map_normalized_name = self
            .maps
            .get(self.selected_map)
            .map(|map| map.normalized_name.clone());

        let settings = AppSettings {
            selected_map_normalized_name,
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
            .with_inner_size([1280.0, 720.0])
            .with_icon(Arc::new(load_icon())),
        ..Default::default()
    };

    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(TarkovMapApp::new(cc)))),
    )
}
