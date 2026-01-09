#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(rustdoc::missing_crate_level_docs)] // it's an example

use eframe::egui;
use egui_extras::install_image_loaders;
use std::collections::HashMap;
use std::sync::mpsc;
use tarkov_map::{Map, MapGroup, TarkovMaps};

const MAPS_RON_PATH: &str = "assets/maps.ron";
const USER_AGENT: &str = "tarkov-map";

enum SvgLoadState {
    Loading(mpsc::Receiver<Result<egui::load::Bytes, String>>),
    Ready(egui::load::Bytes),
    Error(String),
}

struct TarkovMapApp {
    maps: TarkovMaps,
    load_error: Option<String>,

    selected_group: usize,
    selected_map: usize,
    zoom: f32,

    svg_cache: HashMap<String, SvgLoadState>,
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

        let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");

        let mut app = Self {
            maps,
            load_error,
            selected_group: 0,
            selected_map: 0,
            zoom: 1.0,
            svg_cache: HashMap::new(),
            http: reqwest::Client::new(),
            runtime,
        };

        app.reset_selected_map();

        app
    }

    fn reset_selected_map(&mut self) {
        let Some(group) = self.maps.get(self.selected_group) else {
            self.selected_map = 0;
            return;
        };

        self.selected_map = default_map_index(group);
    }

    fn selected_group(&self) -> Option<&MapGroup> {
        self.maps.get(self.selected_group)
    }

    fn selected_map(&self) -> Option<&Map> {
        self.selected_group()
            .and_then(|group| group.maps.get(self.selected_map))
    }

    fn request_svg(&mut self, ctx: &egui::Context, url: &str) {
        if self.svg_cache.contains_key(url) {
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

        self.svg_cache.insert(url, SvgLoadState::Loading(rx));
    }

    fn poll_svg(&mut self, url: &str) {
        let mut done: Option<SvgLoadState> = None;

        if let Some(SvgLoadState::Loading(rx)) = self.svg_cache.get_mut(url) {
            match rx.try_recv() {
                Ok(Ok(bytes)) => done = Some(SvgLoadState::Ready(bytes)),
                Ok(Err(err)) => done = Some(SvgLoadState::Error(err)),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(SvgLoadState::Error(
                        "download channel disconnected unexpectedly".to_owned(),
                    ));
                }
            }
        }

        if let Some(new_state) = done {
            self.svg_cache.insert(url.to_owned(), new_state);
        }
    }

    fn show_svg(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, url: &str) {
        self.request_svg(ctx, url);
        self.poll_svg(url);

        match self.svg_cache.get(url) {
            Some(SvgLoadState::Ready(bytes)) => {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let uri = format!("bytes://{}", url);
                        let image = egui::Image::from_bytes(uri, bytes.clone())
                            .fit_to_original_size(self.zoom);
                        ui.add(image);
                    });
            }
            Some(SvgLoadState::Error(err)) => {
                ui.colored_label(egui::Color32::RED, err);
            }
            Some(SvgLoadState::Loading(_)) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Loading SVG…");
                });
            }
            None => {
                ui.label("Preparing download…");
            }
        }
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

                if self.selected_group != prev_group {
                    self.reset_selected_map();
                }

                ui.separator();

                if let Some(group) = self.selected_group() {
                    let variants: Vec<String> = group.maps.iter().map(variant_label).collect();

                    let selected_variant = variants
                        .get(self.selected_map)
                        .cloned()
                        .unwrap_or_else(|| "(unknown)".to_owned());

                    egui::ComboBox::from_id_salt("map_variant")
                        .selected_text(selected_variant)
                        .show_ui(ui, |ui| {
                            for (idx, label) in variants.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_map, idx, label.as_str());
                            }
                        });
                }

                ui.separator();
                ui.add(
                    egui::Slider::new(&mut self.zoom, 0.1..=3.0)
                        .logarithmic(true)
                        .text("Zoom"),
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

            let Some(map) = self.selected_map() else {
                ui.label("No map variant selected.");
                return;
            };

            let group_name = group.normalized_name.clone();
            let variant = variant_label(map);
            let author = map.author.clone();
            let author_link = map.author_link.clone();
            let svg_url = map.svg_path.clone();

            ui.heading(group_name);

            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Variant: {}", variant));

                if let Some(author) = &author {
                    ui.label(format!("Author: {}", author));
                }

                if let Some(author_link) = &author_link {
                    ui.hyperlink(author_link);
                }
            });

            ui.separator();

            if let Some(svg_url) = svg_url.as_deref() {
                self.show_svg(ui, ctx, svg_url);
            } else {
                ui.label("No SVG available for this map variant.");
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

fn load_maps(path: &str) -> Result<TarkovMaps, String> {
    let ron_string =
        std::fs::read_to_string(path).map_err(|err| format!("read maps file {path}: {err}"))?;

    ron::from_str::<TarkovMaps>(&ron_string).map_err(|err| format!("parse maps file {path}: {err}"))
}

fn default_map_index(group: &MapGroup) -> usize {
    group
        .maps
        .iter()
        .position(|map| map.svg_path.is_some())
        .unwrap_or(0)
}

fn variant_label(map: &Map) -> String {
    match &map.specific {
        Some(specific) if !specific.is_empty() => {
            format!("{} – {} ({})", map.key, specific, map.projection)
        }
        _ => format!("{} ({})", map.key, map.projection),
    }
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
