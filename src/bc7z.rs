//! The `.bc7z` map image container: zstd-compressed BC7 blocks with a small
//! header.
//!
//! Map images ship pre-encoded as BC7 (premultiplied alpha, sRGB) so the app
//! can upload them straight to the GPU without decoding pixels. BC7 requires
//! dimensions that are multiples of 4, so images are padded with transparent
//! pixels; the header keeps both the real pixel size (for UV cropping) and
//! the padded size (the texture dimensions).
//!
//! Layout: `b"BC7Z"` magic, `u32` version, `u32` pixel width/height, `u32`
//! padded width/height (all little-endian), then a zstd stream of the BC7
//! blocks (`padded_w/4 * padded_h/4 * 16` bytes).

use thiserror::Error;

const MAGIC: &[u8; 4] = b"BC7Z";
const VERSION: u32 = 1;
const HEADER_LEN: usize = 4 + 4 + 4 * 4;

/// Bytes per 4×4 BC7 block.
const BLOCK_BYTES: usize = 16;

#[derive(Error, Debug)]
pub enum Bc7zError {
    #[error("not a bc7z container (bad magic)")]
    BadMagic,
    #[error("unsupported bc7z version {0}")]
    UnsupportedVersion(u32),
    #[error("truncated bc7z header")]
    Truncated,
    #[error("invalid dimensions: pixel {pixel:?}, padded {padded:?}")]
    InvalidDimensions { pixel: [u32; 2], padded: [u32; 2] },
    #[error("payload is {actual} bytes, expected {expected} for the padded size")]
    PayloadSizeMismatch { actual: usize, expected: usize },
    #[error("zstd: {0}")]
    Zstd(#[from] std::io::Error),
}

/// A decoded `.bc7z` image: raw BC7 blocks ready for GPU upload.
pub struct Bc7Image {
    /// Real image size in pixels, before padding. UVs should crop to this.
    pub pixel_size: [u32; 2],
    /// Texture size: `pixel_size` rounded up to multiples of 4.
    pub padded_size: [u32; 2],
    /// BC7 blocks, row-major, `padded/4 × padded/4` blocks of 16 bytes.
    pub blocks: Vec<u8>,
}

impl Bc7Image {
    /// The byte length `blocks` must have for `padded_size`.
    pub fn expected_len(padded_size: [u32; 2]) -> usize {
        (padded_size[0] as usize / 4) * (padded_size[1] as usize / 4) * BLOCK_BYTES
    }
}

fn validate(pixel: [u32; 2], padded: [u32; 2]) -> Result<(), Bc7zError> {
    let ok = pixel[0] > 0
        && pixel[1] > 0
        && padded[0].is_multiple_of(4)
        && padded[1].is_multiple_of(4)
        && padded[0] >= pixel[0]
        && padded[1] >= pixel[1]
        && padded[0] - pixel[0] < 4
        && padded[1] - pixel[1] < 4;
    if ok {
        Ok(())
    } else {
        Err(Bc7zError::InvalidDimensions { pixel, padded })
    }
}

/// Packs BC7 blocks into a `.bc7z` container (zstd level 19; generation-time
/// cost, irrelevant at runtime).
pub fn pack(image: &Bc7Image) -> Result<Vec<u8>, Bc7zError> {
    validate(image.pixel_size, image.padded_size)?;
    let expected = Bc7Image::expected_len(image.padded_size);
    if image.blocks.len() != expected {
        return Err(Bc7zError::PayloadSizeMismatch {
            actual: image.blocks.len(),
            expected,
        });
    }

    let mut out = Vec::with_capacity(HEADER_LEN + image.blocks.len() / 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    for value in [
        image.pixel_size[0],
        image.pixel_size[1],
        image.padded_size[0],
        image.padded_size[1],
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    let compressed = zstd::encode_all(&image.blocks[..], 19)?;
    out.extend_from_slice(&compressed);
    Ok(out)
}

/// Unpacks a `.bc7z` container, validating the header and payload size.
pub fn unpack(bytes: &[u8]) -> Result<Bc7Image, Bc7zError> {
    if bytes.len() < HEADER_LEN {
        return Err(
            if bytes.get(..4) != Some(MAGIC.as_slice()) && bytes.len() >= 4 {
                Bc7zError::BadMagic
            } else {
                Bc7zError::Truncated
            },
        );
    }
    if &bytes[..4] != MAGIC {
        return Err(Bc7zError::BadMagic);
    }
    let word = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let version = word(4);
    if version != VERSION {
        return Err(Bc7zError::UnsupportedVersion(version));
    }
    let pixel_size = [word(8), word(12)];
    let padded_size = [word(16), word(20)];
    validate(pixel_size, padded_size)?;

    let blocks = zstd::decode_all(&bytes[HEADER_LEN..])?;
    let expected = Bc7Image::expected_len(padded_size);
    if blocks.len() != expected {
        return Err(Bc7zError::PayloadSizeMismatch {
            actual: blocks.len(),
            expected,
        });
    }

    Ok(Bc7Image {
        pixel_size,
        padded_size,
        blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(pixel: [u32; 2], padded: [u32; 2]) -> Bc7Image {
        Bc7Image {
            pixel_size: pixel,
            padded_size: padded,
            blocks: vec![0xAB; Bc7Image::expected_len(padded)],
        }
    }

    #[test]
    fn round_trips_padded_image() {
        let original = image([1654, 1522], [1656, 1524]);
        let packed = pack(&original).unwrap();
        let unpacked = unpack(&packed).unwrap();

        assert_eq!(unpacked.pixel_size, [1654, 1522]);
        assert_eq!(unpacked.padded_size, [1656, 1524]);
        assert_eq!(unpacked.blocks, original.blocks);
    }

    #[test]
    fn round_trips_exact_multiple_of_four() {
        let original = image([4096, 4096], [4096, 4096]);
        let packed = pack(&original).unwrap();
        let unpacked = unpack(&packed).unwrap();
        assert_eq!(unpacked.pixel_size, unpacked.padded_size);
        assert_eq!(unpacked.blocks.len(), Bc7Image::expected_len([4096, 4096]));
    }

    #[test]
    fn compresses_flat_content() {
        let packed = pack(&image([4096, 4096], [4096, 4096])).unwrap();
        assert!(
            packed.len() < Bc7Image::expected_len([4096, 4096]) / 10,
            "flat blocks should compress heavily, got {} bytes",
            packed.len()
        );
    }

    #[test]
    fn rejects_bad_magic() {
        let mut packed = pack(&image([4, 4], [4, 4])).unwrap();
        packed[0] = b'X';
        assert!(matches!(unpack(&packed), Err(Bc7zError::BadMagic)));
    }

    #[test]
    fn rejects_unsupported_version() {
        let mut packed = pack(&image([4, 4], [4, 4])).unwrap();
        packed[4] = 99;
        assert!(matches!(
            unpack(&packed),
            Err(Bc7zError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn rejects_truncated_input() {
        assert!(matches!(unpack(b"BC7"), Err(Bc7zError::Truncated)));
        assert!(matches!(unpack(b"BC7Z\x01\x00"), Err(Bc7zError::Truncated)));
    }

    #[test]
    fn rejects_invalid_dimensions() {
        // padding not a multiple of 4
        assert!(pack(&image([5, 5], [6, 6])).is_err());
        // padded smaller than pixel
        assert!(pack(&image([8, 8], [4, 4])).is_err());
        // zero-sized
        assert!(pack(&image([0, 4], [0, 4])).is_err());
        // padding further than the next multiple of 4
        assert!(pack(&image([4, 4], [12, 4])).is_err());
    }

    #[test]
    fn rejects_payload_size_mismatch() {
        let mut original = image([8, 8], [8, 8]);
        original.blocks.truncate(16);
        assert!(matches!(
            pack(&original),
            Err(Bc7zError::PayloadSizeMismatch { .. })
        ));
    }
}
