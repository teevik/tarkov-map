//! PROTOTYPE — throwaway. Verifies the GPU half of the BC7 pipeline:
//!   - Is GL_ARB_texture_compression_bptc available?
//!   - Can glow upload BC7 via compressed_tex_image_2d and register it
//!     with egui (`register_native_glow_texture`)?
//!   - How long does the upload take (the full "switch cost" = zstd + upload)?
//!   - Does sRGB/premultiplied BC7 render identically to the PNG path?
//!
//! Run `cargo run --release` first (writes out/*.bc7z), then
//! `cargo run --release --bin gpu`. Left: PNG path, right: BC7 path.

use eframe::egui;
use eframe::glow::{self, HasContext};
use std::time::Instant;

const IMAGE: &str = "../assets/maps/factory-layer-2.png";
const BC7Z: &str = "out/factory-layer-2.png.bc7z";
const DIMS: &str = "out/factory-layer-2.png.dims";

const COMPRESSED_SRGB_ALPHA_BPTC_UNORM: u32 = 0x8E8D;

struct GpuProto {
    png_texture: Option<egui::TextureHandle>,
    bc7_texture: Option<egui::TextureId>,
    report: Vec<String>,
    frames: u32,
}

impl GpuProto {
    fn load_png_path(&mut self, ctx: &egui::Context) {
        // Today's pipeline: PNG decode -> premultiply -> RGBA upload
        let t = Instant::now();
        let bytes = std::fs::read(IMAGE).unwrap();
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, &img);
        let decode_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let tex = ctx.load_texture("png_path", color, egui::TextureOptions::LINEAR);
        let upload_ms = t.elapsed().as_secs_f64() * 1000.0;
        self.report.push(format!(
            "PNG path: decode+premul {decode_ms:.0} ms, egui upload {upload_ms:.0} ms"
        ));
        self.png_texture = Some(tex);
    }

    fn load_bc7_path(&mut self, frame: &mut eframe::Frame) {
        let gl = frame.gl().expect("glow backend required").clone();

        let has_bptc = gl
            .supported_extensions()
            .iter()
            .any(|e| e.contains("texture_compression_bptc"));
        self.report
            .push(format!("BPTC extension present: {has_bptc}"));
        if !has_bptc {
            return;
        }

        let dims = std::fs::read_to_string(DIMS).unwrap();
        let (w, h) = dims.trim().split_once(' ').unwrap();
        let (w, h): (i32, i32) = (w.parse().unwrap(), h.parse().unwrap());
        let bc7z = std::fs::read(BC7Z).unwrap();

        // The proposed pipeline: zstd decompress -> compressed upload
        let t = Instant::now();
        let bc7 = zstd::decode_all(&bc7z[..]).unwrap();
        let zstd_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let texture = unsafe {
            let texture = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.compressed_tex_image_2d(
                glow::TEXTURE_2D,
                0,
                COMPRESSED_SRGB_ALPHA_BPTC_UNORM as i32,
                w,
                h,
                0,
                bc7.len() as i32,
                &bc7,
            );
            let err = gl.get_error();
            if err != glow::NO_ERROR {
                self.report.push(format!("GL ERROR after upload: {err:#x}"));
            }
            gl.finish(); // force completion so the timing is honest
            texture
        };
        let upload_ms = t.elapsed().as_secs_f64() * 1000.0;

        self.report.push(format!(
            "BC7 path: zstd {zstd_ms:.0} ms, GPU upload {upload_ms:.0} ms  (total switch cost {:.0} ms)",
            zstd_ms + upload_ms
        ));
        self.bc7_texture = Some(frame.register_native_glow_texture(texture));
    }
}

impl eframe::App for GpuProto {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Self-capture: screenshot at frame 5, save, then close.
        self.frames += 1;
        if self.frames == 5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.input(|i| {
            for event in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = event {
                    let pixels: Vec<u8> = image
                        .pixels
                        .iter()
                        .flat_map(|p| p.to_array())
                        .collect();
                    image::RgbaImage::from_raw(
                        image.width() as u32,
                        image.height() as u32,
                        pixels,
                    )
                    .unwrap()
                    .save("out/gpu-screenshot.png")
                    .unwrap();
                    println!("screenshot saved to out/gpu-screenshot.png");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        });
        ctx.request_repaint();

        if self.png_texture.is_none() {
            self.load_png_path(ctx);
            self.load_bc7_path(frame);
            for line in &self.report {
                println!("{line}");
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            for line in &self.report {
                ui.label(line);
            }
            ui.separator();
            ui.columns(2, |cols| {
                cols[0].label("PNG path (today)");
                if let Some(tex) = &self.png_texture {
                    cols[0].image((tex.id(), egui::vec2(480.0, 480.0)));
                }
                cols[1].label("BC7 path (proposed)");
                if let Some(id) = self.bc7_texture {
                    cols[1].image((id, egui::vec2(480.0, 480.0)));
                }
            });
        });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "PROTOTYPE bc7-gpu",
        eframe::NativeOptions::default(),
        Box::new(|_| {
            Ok(Box::new(GpuProto {
                png_texture: None,
                bc7_texture: None,
                report: Vec::new(),
                frames: 0,
            }))
        }),
    )
}
