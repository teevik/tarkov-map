//! Asset embedding and loading utilities.

use rust_embed::RustEmbed;
use std::sync::mpsc;
use tarkov_map::TarkovMaps;
use thiserror::Error;

/// Embeds all assets from the assets/ directory into the binary.
/// In debug mode, assets are loaded from the filesystem for faster iteration.
/// In release mode, assets are compressed and embedded in the binary.
#[derive(RustEmbed)]
#[folder = "assets/"]
pub struct Assets;

/// Errors that can occur when loading map data.
#[derive(Error, Debug)]
pub enum MapLoadError {
    #[error("maps.ron not found in embedded assets")]
    MapsNotFound,
    #[error("invalid UTF-8 in maps.ron: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("failed to parse maps.ron: {0}")]
    ParseError(#[from] ron::de::SpannedError),
}

/// Errors that can occur when loading and decoding images.
#[derive(Error, Debug)]
pub enum ImageLoadError {
    #[error("asset not found: {0}")]
    AssetNotFound(String),
    #[error("failed to decode image '{path}': {source}")]
    DecodeError {
        path: String,
        source: image::ImageError,
    },
}

/// Decoded image data ready for texture creation.
pub struct DecodedImage {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// State of an asset being loaded asynchronously.
pub enum AssetLoadState {
    /// Asset is being loaded in a background thread.
    Loading(mpsc::Receiver<Result<DecodedImage, ImageLoadError>>),
    /// Asset has been decoded and is ready for texture creation.
    Ready(DecodedImage),
    /// Loading failed; stores the error message (already displayed via toast).
    Error(String),
}

/// Loads and decodes an image from embedded assets.
pub fn load_and_decode_image(path: &str) -> Result<DecodedImage, ImageLoadError> {
    let file = Assets::get(path).ok_or_else(|| ImageLoadError::AssetNotFound(path.to_string()))?;

    let img =
        image::load_from_memory(&file.data).map_err(|source| ImageLoadError::DecodeError {
            path: path.to_string(),
            source,
        })?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    Ok(DecodedImage {
        pixels: rgba.into_raw(),
        width,
        height,
    })
}

/// Loads the map data from embedded assets.
pub fn load_maps() -> Result<TarkovMaps, MapLoadError> {
    let file = Assets::get("maps.ron").ok_or(MapLoadError::MapsNotFound)?;
    let ron_string = std::str::from_utf8(&file.data)?;
    Ok(ron::from_str(ron_string)?)
}
