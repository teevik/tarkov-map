//! PROTOTYPE — throwaway. Answers, with real repo assets:
//!   1. How big are BC7+zstd files vs the current PNGs?
//!   2. How fast is zstd-decompress (the runtime cost) vs PNG decode?
//!   3. How much quality does BC7 lose on line-art maps?
//!   4. What does padding non-div-4 base maps cost?
//!
//! Run: cargo run --release  (from prototype-bc7/)
//! Writes side-by-side quality crops to ./out/ for eyeballing.

use image::GenericImageView;
use intel_tex_2::{bc7, RgbaSurface};
use std::time::Instant;

const IMAGES: &[&str] = &[
    "../assets/maps/factory-layer-2.png", // densest content, 6 MB PNG
    "../assets/maps/reserve-layer-4.png", // typical line art, 3 MB PNG
    "../assets/maps/reserve.png",         // non-div-4 base map (1654x1522)
];

fn main() {
    std::fs::create_dir_all("out").unwrap();
    println!(
        "{:<28} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>8}",
        "image", "png MB", "pngDec ms", "bc7enc s", "bc7z MB", "zstdDec ms", "PSNR dB", "pad px"
    );

    for path in IMAGES {
        let name = path.rsplit('/').next().unwrap();
        let png_bytes = std::fs::read(path).unwrap();
        let png_mb = png_bytes.len() as f64 / 1048576.0;

        // 1. Current path: PNG decode (what the app does today)
        let t = Instant::now();
        let img = image::load_from_memory(&png_bytes).unwrap();
        let png_decode_ms = t.elapsed().as_secs_f64() * 1000.0;
        let (w, h) = img.dimensions();
        let mut rgba = img.to_rgba8();

        // Premultiply alpha: egui textures are premultiplied (the app pays
        // this per-pixel pass on every load today; BC7 assets bake it in).
        // Also zeroes RGB of fully-transparent pixels -> flatter BC7 blocks.
        let t = Instant::now();
        for px in rgba.chunks_exact_mut(4) {
            let a = px[3] as u32;
            px[0] = (px[0] as u32 * a / 255) as u8;
            px[1] = (px[1] as u32 * a / 255) as u8;
            px[2] = (px[2] as u32 * a / 255) as u8;
        }
        let premul_ms = t.elapsed().as_secs_f64() * 1000.0;

        // 4. Pad to multiple of 4 if needed (transparent border)
        let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
        let padded_px = (pw - w) + (ph - h);
        if (pw, ph) != (w, h) {
            let mut padded = image::RgbaImage::new(pw, ph);
            image::imageops::overlay(&mut padded, &rgba, 0, 0);
            rgba = padded;
        }

        // 2. Offline: BC7 encode (basic quality profile, has alpha)
        let surface = RgbaSurface {
            width: pw,
            height: ph,
            stride: pw * 4,
            data: &rgba,
        };
        let t = Instant::now();
        let bc7 = bc7::compress_blocks(&bc7::alpha_basic_settings(), &surface);
        let bc7_encode_s = t.elapsed().as_secs_f64();

        // Offline: zstd compress (level 19 — release-time cost, irrelevant at runtime)
        let bc7z = zstd::encode_all(&bc7[..], 19).unwrap();
        let bc7z_mb = bc7z.len() as f64 / 1048576.0;

        // 3. Runtime path: zstd decompress (this replaces PNG decode)
        let t = Instant::now();
        let decompressed = zstd::decode_all(&bc7z[..]).unwrap();
        let zstd_decode_ms = t.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(decompressed.len(), bc7.len());

        // Quality: decode BC7 on CPU, PSNR vs original + crop for eyeballing
        let mut decoded_u32 = vec![0u32; (pw * ph) as usize];
        texture2ddecoder::decode_bc7(&bc7, pw as usize, ph as usize, &mut decoded_u32).unwrap();
        // texture2ddecoder outputs 0xAARRGGBB u32s
        let decoded_rgba: Vec<u8> = decoded_u32
            .iter()
            .flat_map(|px| {
                let [b, g, r, a] = px.to_le_bytes();
                [r, g, b, a]
            })
            .collect();
        let mse: f64 = rgba
            .iter()
            .zip(&decoded_rgba)
            .map(|(&a, &b)| {
                let d = a as f64 - b as f64;
                d * d
            })
            .sum::<f64>()
            / rgba.len() as f64;
        let psnr = 10.0 * (255.0f64 * 255.0 / mse).log10();

        // Side-by-side 512x512 crop from the image center
        let (cx, cy) = (pw / 2 - 256, ph / 2 - 256);
        let mut side = image::RgbaImage::new(1024, 512);
        let orig_view = image::imageops::crop_imm(&rgba, cx, cy, 512, 512).to_image();
        let dec_img = image::RgbaImage::from_raw(pw, ph, decoded_rgba).unwrap();
        let dec_view = image::imageops::crop_imm(&dec_img, cx, cy, 512, 512).to_image();
        image::imageops::overlay(&mut side, &orig_view, 0, 0);
        image::imageops::overlay(&mut side, &dec_view, 512, 0);
        side.save(format!("out/{name}.compare.png")).unwrap();

        // Persist the bc7z payload for the GPU prototype
        std::fs::write(format!("out/{name}.bc7z"), &bc7z).unwrap();
        std::fs::write(
            format!("out/{name}.dims"),
            format!("{pw} {ph}"),
        )
        .unwrap();

        println!(
            "{:<28} {:>9.1} {:>9.0} {:>9.1} {:>9.1} {:>10.0} {:>9.1} {:>8} (+premul {:.0} ms in today's path)",
            name, png_mb, png_decode_ms, bc7_encode_s, bc7z_mb, zstd_decode_ms, psnr, padded_px, premul_ms
        );
    }
    println!("\nQuality crops written to out/*.compare.png (left: original, right: BC7)");
}
