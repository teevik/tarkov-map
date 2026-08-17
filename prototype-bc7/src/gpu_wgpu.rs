//! PROTOTYPE — throwaway. Verifies the wgpu-backend production path:
//!   - eframe with Renderer::Wgpu + TEXTURE_COMPRESSION_BC device feature
//!   - upload BC7 via queue.write_texture (Bc7RgbaUnormSrgb)
//!   - register_native_texture / free_texture lifecycle, cycled repeatedly
//!     (the exact pattern single-texture retention needs on every map switch)
//!
//! Run `cargo run --release` first (writes out/*.bc7z), then
//! `cargo run --release --bin gpu-wgpu`.

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use std::sync::Arc;
use std::time::Instant;

const BC7Z: &str = "out/factory-layer-2.png.bc7z";
const DIMS: &str = "out/factory-layer-2.png.dims";

struct WgpuProto {
    bc7: Vec<u8>,
    size: (u32, u32),
    current: Option<(egui::TextureId, wgpu::Texture)>,
    report: Vec<String>,
    frames: u32,
    cycles: u32,
}

impl WgpuProto {
    fn upload(&mut self, frame: &eframe::Frame) {
        let rs = frame.wgpu_render_state().expect("wgpu backend required");
        let (w, h) = self.size;

        let t = Instant::now();
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bc7 map prototype"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bc7RgbaUnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        rs.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.bc7,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w / 4 * 16), // 16 bytes per 4x4 BC7 block
                rows_per_image: Some(h / 4),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let id = rs.renderer.write().register_native_texture(
            &rs.device,
            &view,
            wgpu::FilterMode::Linear,
        );
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        self.report
            .push(format!("cycle {}: upload+register {ms:.1} ms", self.cycles));
        self.current = Some((id, texture));
    }

    fn free(&mut self, frame: &eframe::Frame) {
        if let Some((id, texture)) = self.current.take() {
            let rs = frame.wgpu_render_state().unwrap();
            rs.renderer.write().free_texture(&id);
            texture.destroy(); // immediate VRAM release
        }
    }
}

impl eframe::App for WgpuProto {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.frames += 1;

        if self.frames == 1 {
            let rs = frame.wgpu_render_state().unwrap();
            self.report.push(format!(
                "backend: {:?}, BC feature: {}",
                rs.adapter.get_info().backend,
                rs.device
                    .features()
                    .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
            ));
            self.upload(frame);
        }

        // Simulate map switching: free + re-upload every 20 frames, 3 times.
        if self.frames % 20 == 0 && self.cycles < 3 {
            self.cycles += 1;
            self.free(frame);
            self.upload(frame);
        }

        if self.frames == 70 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.input(|i| {
            for event in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = event {
                    let pixels: Vec<u8> =
                        image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, pixels)
                        .unwrap()
                        .save("out/gpu-wgpu-screenshot.png")
                        .unwrap();
                    println!("screenshot saved to out/gpu-wgpu-screenshot.png");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        });
        ctx.request_repaint();

        egui::CentralPanel::default().show(ctx, |ui| {
            for line in &self.report {
                ui.label(line);
            }
            ui.separator();
            if let Some((id, _)) = &self.current {
                ui.image((*id, egui::vec2(500.0, 500.0)));
            }
        });
    }
}

fn main() -> eframe::Result {
    let dims = std::fs::read_to_string(DIMS).expect("run the measure bin first");
    let (w, h) = dims.trim().split_once(' ').unwrap();
    let size = (w.parse().unwrap(), h.parse().unwrap());
    let bc7z = std::fs::read(BC7Z).unwrap();
    let t = Instant::now();
    let bc7 = zstd::decode_all(&bc7z[..]).unwrap();
    println!("zstd decompress: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
                device_descriptor: Arc::new(|adapter| {
                    let mut features = wgpu::Features::default();
                    if adapter
                        .features()
                        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
                    {
                        features |= wgpu::Features::TEXTURE_COMPRESSION_BC;
                    }
                    wgpu::DeviceDescriptor {
                        label: Some("bc7 prototype device"),
                        required_features: features,
                        ..Default::default()
                    }
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    eframe::run_native(
        "PROTOTYPE bc7-wgpu",
        options,
        Box::new(move |_| {
            Ok(Box::new(WgpuProto {
                bc7,
                size,
                current: None,
                report: Vec::new(),
                frames: 0,
                cycles: 0,
            }))
        }),
    )
}
