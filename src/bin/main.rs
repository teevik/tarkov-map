#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(rustdoc::missing_crate_level_docs)] // it's an example

use eframe::egui;
use egui_extras::install_image_loaders;
use std::collections::HashMap;
use std::sync::mpsc;
use tarkov_map::{MapGroup, TarkovMaps};

const MAPS_RON_PATH: &str = "assets/maps.ron";
const USER_AGENT: &str = "tarkov-map";

enum AssetLoadState {
    Loading(mpsc::Receiver<Result<egui::load::Bytes, String>>),
    Ready(egui::load::Bytes),
    Error(String),
}

struct TarkovMapApp {
    maps: TarkovMaps,
    load_error: Option<String>,

    selected_group: usize,
    scale: f32,
    tile_zoom: i32,

    asset_cache: HashMap<String, AssetLoadState>,
    http: reqwest::Client,
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

        let tile_zoom = maps
            .first()
            .and_then(|group| group.map.min_zoom)
            .unwrap_or(0);

        let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");

        Self {
            maps,
            load_error,
            selected_group: 0,
            scale: 1.0,
            tile_zoom,
            asset_cache: HashMap::new(),
            http: reqwest::Client::new(),
            runtime,
        }
    }

    fn selected_group(&self) -> Option<&MapGroup> {
        self.maps.get(self.selected_group)
    }

    fn request_asset(&mut self, ctx: &egui::Context, url: &str) {
        if self.asset_cache.contains_key(url) {
            return;
        }

        let (tx, rx) = mpsc::channel();

        let ctx = ctx.clone();
        let client = self.http.clone();
        let url = url.to_owned();
        let url_for_task = url.clone();

        self.runtime.spawn(async move {
            let result = fetch_url_bytes(&client, &url_for_task)
                .await
                .map(egui::load::Bytes::from);
            let _ = tx.send(result);

            // Wake up egui so we render the newly loaded image.
            ctx.request_repaint();
        });

        self.asset_cache.insert(url, AssetLoadState::Loading(rx));
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

                let prev_group = self.selected_group;

                let selected_group_name = self
                    .maps
                    .get(self.selected_group)
                    .map(|group| group.normalized_name.as_str())
                    .unwrap_or("(unknown)");

                egui::ComboBox::from_id_salt("map_group")
                    .selected_text(selected_group_name)
                    .show_ui(ui, |ui| {
                        for (idx, group) in self.maps.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_group,
                                idx,
                                &group.normalized_name,
                            );
                        }
                    });

                let (use_tiles, min_zoom, max_zoom) = self
                    .selected_group()
                    .map(|group| {
                        let map = &group.map;
                        let min_zoom = map.min_zoom.unwrap_or(0);
                        let max_zoom = map.max_zoom.unwrap_or(min_zoom).max(min_zoom);
                        let use_tiles = map.svg_path.is_none() && map.tile_path.is_some();
                        (use_tiles, min_zoom, max_zoom)
                    })
                    .unwrap_or((false, 0, 0));

                if self.selected_group != prev_group {
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

            let Some(group) = self.selected_group() else {
                ui.label(format!(
                    "No map data loaded. Generate it with `cargo run --bin fetch_maps` (writes {}).",
                    MAPS_RON_PATH
                ));
                return;
            };

            let map = &group.map;

            let group_name = group.normalized_name.clone();
            let map_key = map.key.clone();
            let author = map.author.clone();
            let author_link = map.author_link.clone();
            let svg_url = map.svg_path.clone();
            let tile_path = map.tile_path.clone();
            let tile_size = map.tile_size.unwrap_or(256) as f32;

            ui.heading(group_name);

            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Key: {}", map_key));

                if let Some(author) = &author {
                    ui.label(format!("Author: {}", author));
                }

                if let Some(author_link) = &author_link {
                    ui.hyperlink(author_link);
                }
            });

            ui.separator();

            if let Some(svg_url) = svg_url.as_deref() {
                self.show_single_image(ui, ctx, svg_url);
            } else if let Some(tile_path) = tile_path.as_deref() {
                self.show_tile_map(ui, ctx, tile_path, self.tile_zoom, tile_size);
            } else {
                ui.label("No SVG or tile map available for this map.");
            }
        });
    }
}

async fn fetch_url_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|err| format!("fetch {url}: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("fetch {url}: HTTP {status}"));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("read {url}: {err}"))?;

    Ok(bytes.to_vec())
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
