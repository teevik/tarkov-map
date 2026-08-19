//! Driven ports. Only two of the four in this skeleton: the one async-stream
//! port (`PositionSource`) and one sync port (`ImageDecoder`), enough to feel
//! the `Ports` bundle with both shapes.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use euclid::Size2D;
use futures::Stream;

use crate::domain::{Image, MapImageKey, PlayerPosition};

#[cfg(any(test, feature = "fakes"))]
pub mod fakes;

// ---------------------------------------------------------------- PositionSource

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Screenshots,
    Demo,
}

#[derive(Debug)]
pub enum WatchError {
    NoDocumentsFolder,
    FolderMissing(PathBuf),
    /// Opaque adapter cause (eros error, notify error…) kept in the chain.
    WatchFailed(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for WatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDocumentsFolder => {
                f.write_str("no Documents folder to look for screenshots in")
            }
            Self::FolderMissing(p) => {
                write!(f, "screenshots folder {} does not exist", p.display())
            }
            Self::WatchFailed(_) => f.write_str("could not watch the screenshots folder"),
        }
    }
}

impl Error for WatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WatchFailed(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum PositionEvent {
    Started(SourceKind),
    Position(PlayerPosition),
    Failed(WatchError),
}

/// Where Player Positions come from. Push, not poll: the runner forwards every item as a Msg.
pub trait PositionSource: Send + Sync + 'static {
    fn observe(&self, dir: PathBuf) -> impl Stream<Item = PositionEvent> + Send + 'static;
}

// ---------------------------------------------------------------- ImageDecoder

/// Decoded Main Floor image as plain data; rides opaquely Msg::ImageDecoded → Effect::PresentImage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapImageData {
    pub pixel_size: Size2D<u32, Image>,
    pub padded_size: Size2D<u32, Image>,
    pub bc7_blocks: Vec<u8>,
}

#[derive(Debug)]
pub struct ImageDecodeError {
    pub key: MapImageKey,
    pub source: Box<dyn Error + Send + Sync>,
}

impl fmt::Display for ImageDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "could not decode map image {}", self.key.0)
    }
}

impl Error for ImageDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

pub trait ImageDecoder: Send + Sync + 'static {
    fn decode(&self, key: &MapImageKey) -> Result<MapImageData, ImageDecodeError>;
}

// ---------------------------------------------------------------- bundle

/// The bundle the Effect Runner is generic over. Real impl in the app crate, `FakePorts` here.
pub trait Ports: Send + Sync + 'static {
    type Positions: PositionSource;
    type Decoder: ImageDecoder;
    fn positions(&self) -> &Self::Positions;
    fn decoder(&self) -> &Self::Decoder;
}
