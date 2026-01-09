#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(rustdoc::missing_crate_level_docs)] // it's an example

use eframe::egui;
use egui_extras::install_image_loaders;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tarkov_map::{Map, MapSource, TarkovMaps};

const MAPS_RON_PATH: &str = "assets/maps.ron";

enum AssetLoadState {
    Loading(mpsc::Receiver<Result<egui::load::Bytes, String>>),
    Ready(egui::load::Bytes),
    Error(String),
}

struct TarkovMapApp {
    maps: TarkovMaps,
    load_error: Option<String>,

    selected_map: usize,
    scale: f32,
    tile_zoom: i32,

    asset_cache: HashMap<String, AssetLoadState>,
    runtime: tokio::runtime::Runtime,
}

impl TarkovMapApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_image_loaders(&cc.egui_ctx);

        let maps_path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), MAPS_RON_PATH);

        let (maps, load_error) = match load_maps(&maps_path) {
            Ok(maps) => (maps, None),
            Err(err) => (Vec::new(), Some(err)),
        };

        let tile_zoom = 0;

        let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");

        Self {
            maps,
            load_error,
            selected_map: 0,
            scale: 1.0,
            tile_zoom,
            asset_cache: HashMap::new(),
            runtime,
        }
    }

    fn selected_map(&self) -> Option<&Map> {
        self.maps.get(self.selected_map)
    }

    fn request_asset(&mut self, ctx: &egui::Context, url: &str) {
        if self.asset_cache.contains_key(url) {
            return;
        }

        let (tx, rx) = mpsc::channel();

        let ctx = ctx.clone();
        let asset_id = url.to_owned();
        let asset_path = asset_id.clone();

        self.runtime.spawn(async move {
            let result = load_asset_bytes(&asset_path)
                .await
                .map(egui::load::Bytes::from);
            let _ = tx.send(result);

            // Wake up egui so we render the newly loaded image.
            ctx.request_repaint();
        });

        self.asset_cache
            .insert(asset_id, AssetLoadState::Loading(rx));
    }

    fn poll_asset(&mut self, url: &str) {
        let mut done: Option<AssetLoadState> = None;

        if let Some(AssetLoadState::Loading(rx)) = self.asset_cache.get_mut(url) {
            match rx.try_recv() {
                Ok(Ok(bytes)) => done = Some(AssetLoadState::Ready(bytes)),
                Ok(Err(err)) => done = Some(AssetLoadState::Error(err)),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(AssetLoadState::Error(
                        "download channel disconnected unexpectedly".to_owned(),
                    ));
                }
            }
        }

        if let Some(new_state) = done {
            self.asset_cache.insert(url.to_owned(), new_state);
        }
    }

    fn show_single_image(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, url: &str) {
        self.request_asset(ctx, url);
        self.poll_asset(url);

        match self.asset_cache.get(url) {
            Some(AssetLoadState::Ready(bytes)) => {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let uri = format!("bytes://{}", url);
                        let image = egui::Image::from_bytes(uri, bytes.clone())
                            .fit_to_original_size(self.scale);
                        ui.add(image);
                    });
            }
            Some(AssetLoadState::Error(err)) => {
                ui.colored_label(egui::Color32::RED, err);
            }
            Some(AssetLoadState::Loading(_)) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Loading image…");
                });
            }
            None => {
                ui.label("Preparing download…");
            }
        }
    }

    fn paint_asset_at(&mut self, ui: &egui::Ui, ctx: &egui::Context, url: &str, rect: egui::Rect) {
        self.request_asset(ctx, url);
        self.poll_asset(url);

        let Some(AssetLoadState::Ready(bytes)) = self.asset_cache.get(url) else {
            return;
        };

        let uri = format!("bytes://{}", url);
        egui::Image::from_bytes(uri, bytes.clone()).paint_at(ui, rect);
    }

    fn show_tile_map(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        tile_path: &str,
        tile_zoom: i32,
        tile_size: f32,
    ) {
        let tile_zoom = tile_zoom.max(0) as u32;
        let tiles_per_axis = 1usize << tile_zoom;

        let tile_size = (tile_size * self.scale).max(1.0);
        let content_size = egui::vec2(
            tile_size * tiles_per_axis as f32,
            tile_size * tiles_per_axis as f32,
        );

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                ui.set_min_size(content_size);

                let max_index = tiles_per_axis.saturating_sub(1) as i32;

                let min_x = ((viewport.min.x / tile_size).floor() as i32 - 1).clamp(0, max_index);
                let max_x = ((viewport.max.x / tile_size).ceil() as i32 + 1).clamp(0, max_index);
                let min_y = ((viewport.min.y / tile_size).floor() as i32 - 1).clamp(0, max_index);
                let max_y = ((viewport.max.y / tile_size).ceil() as i32 + 1).clamp(0, max_index);

                let origin = ui.max_rect().min;
                for y in min_y..=max_y {
                    for x in min_x..=max_x {
                        let url = tile_url(tile_path, tile_zoom as i32, x, y);
                        let rect = egui::Rect::from_min_size(
                            origin + egui::vec2(x as f32 * tile_size, y as f32 * tile_size),
                            egui::vec2(tile_size, tile_size),
                        );
                        self.paint_asset_at(ui, ctx, &url, rect);
                    }
                }
            });
    }
}

impl eframe::App for TarkovMapApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Location:");

                if self.maps.is_empty() {
                    ui.label("(no maps loaded)");
                    return;
                }

                let prev_map = self.selected_map;

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

                let (use_tiles, min_zoom, max_zoom) = self
                    .selected_map()
                    .map(|map| match &map.source {
                        MapSource::Tiles {
                            min_zoom, max_zoom, ..
                        } => (true, *min_zoom, *max_zoom),
                        _ => (false, 0, 0),
                    })
                    .unwrap_or((false, 0, 0));

                if self.selected_map != prev_map {
                    self.tile_zoom = min_zoom;
                } else {
                    self.tile_zoom = self.tile_zoom.clamp(min_zoom, max_zoom);
                }

                if use_tiles {
                    ui.separator();
                    ui.add(
                        egui::Slider::new(&mut self.tile_zoom, min_zoom..=max_zoom)
                            .text("Tile zoom"),
                    );
                }

                ui.separator();
                ui.add(
                    egui::Slider::new(&mut self.scale, 0.1..=3.0)
                        .logarithmic(true)
                        .text("Scale"),
                );
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.load_error {
                ui.colored_label(egui::Color32::RED, err);
                ui.separator();
            }

            let Some(map) = self.selected_map() else {
                ui.label(format!(
                    "No map data loaded. Generate it with `cargo run --bin fetch_maps` (writes {}).",
                    MAPS_RON_PATH
                ));
                return;
            };

            let name = map.name.clone();
            let normalized_name = map.normalized_name.clone();
            let author = map.author.clone();
            let author_link = map.author_link.clone();
            let source = map.source.clone();

            ui.heading(name);

            ui.horizontal_wrapped(|ui| {
                ui.label(format!("ID: {}", normalized_name));

                if let Some(author) = &author {
                    ui.label(format!("Author: {}", author));
                }

                if let Some(author_link) = &author_link {
                    ui.hyperlink(author_link);
                }
            });

            ui.separator();

            match source {
                MapSource::Svg { path, .. } => {
                    self.show_single_image(ui, ctx, &path);
                }
                MapSource::Tiles {
                    template,
                    tile_size,
                    ..
                } => {
                    self.show_tile_map(ui, ctx, &template, self.tile_zoom, tile_size as f32);
                }
            }
        });
    }
}

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

fn tile_url(template: &str, z: i32, x: i32, y: i32) -> String {
    template
        .replace("{z}", &z.to_string())
        .replace("{x}", &x.to_string())
        .replace("{y}", &y.to_string())
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
