#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod colors;
mod constants;
mod coordinates;
mod demo;
mod overlays;
mod screenshot_watcher;
mod ui;
mod updater;

use assets::{AssetCache, Bc7Image, load_and_decode_image, load_maps};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
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
    overlays: OverlayVisibility,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 2,
            selected_map_normalized_name: None,
            overlays: OverlayVisibility::default(),
        }
    }
}

/// A map image's BC7 texture, uploaded through wgpu and compressed in VRAM,
/// plus the UV rect cropping off the transparent padding BC7 block alignment
/// adds.
struct MapTexture {
    id: egui::TextureId,
    texture: wgpu::Texture,
    uv: egui::Rect,
}

/// Main application state for the Tarkov Map viewer.
pub struct TarkovMapApp {
    maps: TarkovMaps,
    selected_map: usize,
    /// One-shot request to center the view on the player marker this frame.
    center_on_player: bool,
    zoom: f32,
    prev_zoom: f32,
    pan_offset: egui::Vec2,
    overlays: OverlayVisibility,
    asset_cache: AssetCache,
    texture_cache: HashMap<String, MapTexture>,
    toasts: Toasts,
    updater: updater::Updater,
    screenshot_watcher: Option<ScreenshotWatcher>,
    demo: Option<demo::DemoWalker>,
    player_position: Option<PlayerPosition>,
    /// Set once the missing-BC-support error has been surfaced, so the toast
    /// doesn't repeat on every switch.
    bc_unsupported_notified: bool,

    /// Flag to clear settings on app close (triggered by File -> Clear Settings).
    pub clear_settings_on_close: bool,
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
            center_on_player: false,
            zoom: 1.0,
            prev_zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            overlays: settings.overlays,
            asset_cache,
            texture_cache: HashMap::new(),
            toasts,
            updater,
            screenshot_watcher,
            demo: demo::DemoWalker::from_env(),
            player_position,
            bc_unsupported_notified: false,
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

    /// The image path the current map selection resolves to (its Main Floor).
    fn active_image_path_now(&self) -> Option<String> {
        self.maps
            .get(self.selected_map)
            .map(|map| map.image_path.clone())
    }

    /// Active-image-only retention: frees every texture other than the one
    /// the current selection displays, releasing both the egui registration
    /// and the underlying VRAM, and forgets the path in the asset cache so a
    /// later visit reloads through the normal loading path.
    fn free_inactive_textures(&mut self, frame: &mut eframe::Frame, active: Option<&str>) {
        let stale: Vec<String> = self
            .texture_cache
            .keys()
            .filter(|path| Some(path.as_str()) != active)
            .cloned()
            .collect();

        for path in stale {
            if let Some(map_texture) = self.texture_cache.remove(&path) {
                if let Some(render_state) = frame.wgpu_render_state() {
                    render_state.renderer.write().free_texture(&map_texture.id);
                }
                map_texture.texture.destroy();
            }
            self.asset_cache.evict(&path);
            log::debug!("freed inactive map texture: {path}");
        }
    }

    /// Polls loading assets and uploads the active image's BC7 blocks once
    /// decompressed. Non-active results are dropped without upload (the user
    /// switched away mid-load); the block buffer is released either way.
    fn poll_all_assets(&mut self, frame: &eframe::Frame, active: Option<&str>) {
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

        for path in self.asset_cache.pending_uploads() {
            let Some(decoded) = self.asset_cache.take_decoded(&path) else {
                continue;
            };

            // Only the active image becomes a texture; anything else finished
            // loading after the user moved on. Forget it so a revisit reloads.
            if Some(path.as_str()) != active || self.texture_cache.contains_key(&path) {
                self.asset_cache.evict(&path);
                continue;
            }

            if let Some(map_texture) = self.upload_texture(&path, decoded, frame) {
                self.texture_cache.insert(path, map_texture);
            }
        }
    }

    /// Creates a GPU texture from BC7 blocks, uploaded compressed. BC texture
    /// compression is mandatory on the DX11-class hardware Tarkov itself
    /// requires; without it (software rasterizers, broken drivers) the map
    /// cannot be shown and an error is surfaced once.
    fn upload_texture(
        &mut self,
        path: &str,
        image: Bc7Image,
        frame: &eframe::Frame,
    ) -> Option<MapTexture> {
        let render_state = frame.wgpu_render_state().filter(|render_state| {
            render_state
                .device
                .features()
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
        });
        let Some(render_state) = render_state else {
            if !self.bc_unsupported_notified {
                self.bc_unsupported_notified = true;
                log::error!("GPU lacks BC texture compression; cannot display maps");
                self.toasts.add(Toast {
                    kind: ToastKind::Error,
                    text: "This GPU lacks BC texture compression; maps cannot be displayed."
                        .into(),
                    options: ToastOptions::default()
                        .duration_in_seconds(10.0)
                        .show_icon(true),
                    ..Default::default()
                });
            }
            return None;
        };

        let [width, height] = image.padded_size;
        let uv = egui::Rect::from_min_max(
            egui::pos2(0.0, 0.0),
            egui::pos2(
                image.pixel_size[0] as f32 / width as f32,
                image.pixel_size[1] as f32 / height as f32,
            ),
        );
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = render_state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(path),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Non-sRGB on purpose: egui renders in gamma space and
            // expects raw (gamma-encoded) samples — an Srgb format
            // here double-converts and darkens the map.
            format: wgpu::TextureFormat::Bc7RgbaUnorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        render_state.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.blocks,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width / 4 * 16), // 16 bytes per 4x4 BC7 block
                rows_per_image: Some(height / 4),
            },
            size,
        );
        let view = texture.create_view(&Default::default());
        let id = render_state.renderer.write().register_native_texture(
            &render_state.device,
            &view,
            wgpu::FilterMode::Linear,
        );

        Some(MapTexture { id, texture, uv })
    }

    fn get_texture(&self, path: &str) -> Option<(egui::TextureId, egui::Rect)> {
        self.texture_cache
            .get(path)
            .map(|texture| (texture.id, texture.uv))
    }

    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan_offset = egui::Vec2::ZERO;
    }

    /// Polls the screenshot watcher for player position updates.
    fn poll_player_position(&mut self) {
        let demo_fix = match (&mut self.demo, self.maps.get(self.selected_map)) {
            (Some(demo), Some(map)) => demo.poll(map),
            _ => None,
        };
        let watcher_fix = self.screenshot_watcher.as_mut().and_then(|w| w.poll());

        if let Some(position) = demo_fix.or(watcher_fix) {
            self.player_position = Some(position);
        }
    }
}

impl eframe::App for TarkovMapApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array() // Don't paint behind rounded corners
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Position first so a fresh fix is drawn this frame.
        self.poll_player_position();

        let active = self.active_image_path_now();
        self.free_inactive_textures(frame, active.as_deref());
        self.poll_all_assets(frame, active.as_deref());

        self.handle_keyboard_input(ctx);
        self.updater.poll(ctx, &mut self.toasts);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Render custom window frame with title bar
        self.show_custom_frame(ui);

        self.prev_zoom = self.zoom;

        // Show toasts
        self.toasts.show(ui);
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

/// The default wgpu configuration, with the device descriptor overridden to
/// request BC texture compression when the adapter has it, so map images can
/// be uploaded (and stay) BC7-compressed. Mandatory on DX11-class hardware;
/// adapters without it fall back to CPU decoding (see `upload_texture`).
fn wgpu_configuration() -> egui_wgpu::WgpuConfiguration {
    let mut configuration = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut configuration.wgpu_setup {
        setup.device_descriptor = Arc::new(|adapter| {
            let mut features = wgpu::Features::default();
            if adapter
                .features()
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
            {
                features |= wgpu::Features::TEXTURE_COMPRESSION_BC;
            }
            wgpu::DeviceDescriptor {
                label: Some("tarkov-map device"),
                required_features: features,
                ..Default::default()
            }
        });
    }
    configuration
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
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_configuration(),
        ..Default::default()
    };

    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(TarkovMapApp::new(cc)))),
    )
}
