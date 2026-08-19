//! Hand-rolled fakes (no mockall). Scripted, deterministic, `Notify`-blockable.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::Stream;
use tokio::sync::Semaphore;

use super::{ImageDecodeError, ImageDecoder, MapImageData, Ports, PositionEvent, PositionSource};
use crate::domain::MapImageKey;

/// Replays a scripted list of events once; records every `observe` call.
#[derive(Default)]
pub struct FakePositionSource {
    script: Mutex<Vec<PositionEvent>>,
    pub observed: Mutex<Vec<PathBuf>>,
}

impl FakePositionSource {
    pub fn scripted(events: Vec<PositionEvent>) -> Self {
        Self {
            script: Mutex::new(events),
            observed: Mutex::default(),
        }
    }
}

impl PositionSource for FakePositionSource {
    fn observe(&self, dir: PathBuf) -> impl Stream<Item = PositionEvent> + Send + 'static {
        self.observed.lock().unwrap().push(dir);
        let events = std::mem::take(&mut *self.script.lock().unwrap());
        futures::stream::iter(events)
    }
}

/// Returns a fixed image for every key; optionally blocks until the test adds a permit to `gate`
/// (for cancellation / stale-result tests).
pub struct FakeDecoder {
    pub image: MapImageData,
    pub fail_keys: Vec<MapImageKey>,
    pub gate: Option<Arc<Semaphore>>,
}

impl FakeDecoder {
    pub fn instant() -> Self {
        Self {
            image: MapImageData {
                pixel_size: euclid::size2(4, 4),
                padded_size: euclid::size2(4, 4),
                bc7_blocks: vec![0; 16],
            },
            fail_keys: vec![],
            gate: None,
        }
    }
}

impl ImageDecoder for FakeDecoder {
    fn decode(&self, key: &MapImageKey) -> Result<MapImageData, ImageDecodeError> {
        if let Some(gate) = &self.gate {
            // sync port: block the (spawn_blocking) thread until released
            futures::executor::block_on(gate.acquire())
                .expect("gate never closed")
                .forget();
        }
        if self.fail_keys.contains(key) {
            return Err(ImageDecodeError {
                key: key.clone(),
                source: "scripted failure".into(),
            });
        }
        Ok(self.image.clone())
    }
}

pub struct FakePorts {
    pub positions: FakePositionSource,
    pub decoder: FakeDecoder,
}

impl Ports for FakePorts {
    type Positions = FakePositionSource;
    type Decoder = FakeDecoder;
    fn positions(&self) -> &FakePositionSource {
        &self.positions
    }
    fn decoder(&self) -> &FakeDecoder {
        &self.decoder
    }
}

/// Adapter-style boundary demo (ADR-0001): eros free *inside* the adapter, mapped into the
/// port's hand-rolled error exactly once at the `impl Port` method. Lives here only so the
/// skeleton exercises eros on one path; the real thing is `EmbeddedImageDecoder` in adapters.
#[cfg(test)]
mod eros_boundary {
    use super::*;
    use eros::{IntoDynUnion, bail};

    fn unpack_bc7z(bytes: &[u8]) -> eros::Result<MapImageData> {
        if bytes.len() < 4 {
            bail!("bc7z header truncated: {} bytes", bytes.len());
        }
        let magic = std::str::from_utf8(&bytes[..4]).into_dyn_union()?; // std error → untyped union
        // NB: eros `ensure!/bail!(cond, "literal {x}")` does NOT format a bare literal — pass args explicitly.
        eros::ensure!(magic == "BC7Z", "bad magic {:?}", magic);
        Ok(MapImageData {
            pixel_size: euclid::size2(1, 1),
            padded_size: euclid::size2(4, 4),
            bc7_blocks: vec![],
        })
    }

    struct BytesDecoder(Vec<u8>);

    impl ImageDecoder for BytesDecoder {
        fn decode(&self, key: &MapImageKey) -> Result<MapImageData, ImageDecodeError> {
            // the one mapping point: opaque eros error becomes the port's `source`
            unpack_bc7z(&self.0).map_err(|e| ImageDecodeError {
                key: key.clone(),
                source: e.into_inner(),
            })
        }
    }

    #[test]
    fn opaque_adapter_error_surfaces_as_the_port_error_with_its_chain() {
        let err = BytesDecoder(b"NOPE".to_vec())
            .decode(&MapImageKey("maps/x.bc7z".into()))
            .unwrap_err();
        assert_eq!(err.to_string(), "could not decode map image maps/x.bc7z");
        assert_eq!(
            std::error::Error::source(&err).unwrap().to_string(),
            "bad magic \"NOPE\""
        );
        assert!(
            BytesDecoder(b"BC7Z....".to_vec())
                .decode(&MapImageKey("k".into()))
                .is_ok()
        );
    }
}
