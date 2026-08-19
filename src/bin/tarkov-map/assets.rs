//! Asset embedding and loading utilities.

use rust_embed::RustEmbed;
use std::collections::HashMap;
use std::sync::mpsc;
use tarkov_map::TarkovMaps;
use thiserror::Error;

/// Embeds all assets from the assets/ directory into the binary.
/// In debug mode, assets are loaded from the filesystem for faster iteration.
/// In release mode, assets are compressed and embedded in the binary.
///
/// Map images ship as `.bc7z` containers; PNGs under `maps/` are fetch_maps
/// intermediates and are never embedded.
#[derive(RustEmbed)]
#[folder = "assets/"]
#[exclude = "maps/*.png"]
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
        source: tarkov_map::bc7z::Bc7zError,
    },
}

/// A loaded map image: BC7 blocks ready for direct GPU upload.
pub use tarkov_map::bc7z::Bc7Image;

/// State of an asset in the demand-driven loading pipeline.
pub enum AssetLoadState {
    /// Asset is being decompressed on a background thread.
    Loading(mpsc::Receiver<Result<Bc7Image, ImageLoadError>>),
    /// Asset has been decompressed; the block buffer is awaiting texture upload.
    Decoded(Bc7Image),
    /// A texture has been created and the block buffer released.
    Uploaded,
    /// Loading failed; stores the error message (already displayed via toast).
    Error(String),
}

/// Loads and decompresses a `.bc7z` map image from embedded assets.
pub fn load_and_decode_image(path: &str) -> Result<Bc7Image, ImageLoadError> {
    let file = Assets::get(path).ok_or_else(|| ImageLoadError::AssetNotFound(path.to_string()))?;

    tarkov_map::bc7z::unpack(&file.data).map_err(|source| ImageLoadError::DecodeError {
        path: path.to_string(),
        source,
    })
}

/// Loads the map data from embedded assets.
pub fn load_maps() -> Result<TarkovMaps, MapLoadError> {
    let file = Assets::get("maps.ron").ok_or(MapLoadError::MapsNotFound)?;
    let ron_string = std::str::from_utf8(&file.data)?;
    Ok(ron::from_str(ron_string)?)
}

/// Tracks the demand-driven load state of map images keyed by asset path.
///
/// The cache is the single source of truth for whether a given image has been
/// requested. Images move through `Loading -> Decoded -> Uploaded`, with any
/// failure landing in `Error`. Once a texture has been created the decoded
/// pixel buffer is released (via [`AssetCache::take_decoded`]) so only the
/// GPU-resident texture is retained.
#[derive(Default)]
pub struct AssetCache {
    states: HashMap<String, AssetLoadState>,
}

impl AssetCache {
    /// Creates an empty cache. Images are only loaded once [`request`] is
    /// called for them.
    ///
    /// [`request`]: AssetCache::request
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests a load for `path`, starting a background decode via `spawn`.
    ///
    /// A path that is already loading, decoded, uploaded, or errored is not
    /// requested again: `spawn` is only invoked, and `true` returned, when the
    /// path is seen for the first time.
    pub fn request<F>(&mut self, path: &str, spawn: F) -> bool
    where
        F: FnOnce() -> mpsc::Receiver<Result<Bc7Image, ImageLoadError>>,
    {
        if self.states.contains_key(path) {
            return false;
        }
        let receiver = spawn();
        self.states
            .insert(path.to_string(), AssetLoadState::Loading(receiver));
        true
    }

    /// Polls every loading asset, moving completed decodes to `Decoded` and
    /// failures to `Error`. Returns the error messages for assets that failed
    /// during this call so the caller can surface them (e.g. via toasts).
    pub fn poll(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        let mut updates: Vec<(String, AssetLoadState)> = Vec::new();

        for (path, state) in &mut self.states {
            if let AssetLoadState::Loading(receiver) = state {
                match receiver.try_recv() {
                    Ok(Ok(decoded)) => {
                        updates.push((path.clone(), AssetLoadState::Decoded(decoded)));
                    }
                    Ok(Err(err)) => {
                        let message = format!("{path}: {err}");
                        errors.push(message.clone());
                        updates.push((path.clone(), AssetLoadState::Error(message)));
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let message = format!("{path}: channel disconnected");
                        errors.push(message.clone());
                        updates.push((path.clone(), AssetLoadState::Error(message)));
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        }

        for (path, new_state) in updates {
            self.states.insert(path, new_state);
        }

        errors
    }

    /// Returns the current load state of `path`, or `None` if it has never been
    /// requested.
    pub fn state(&self, path: &str) -> Option<&AssetLoadState> {
        self.states.get(path)
    }

    /// Returns the paths whose decoded pixels are awaiting texture upload.
    pub fn pending_uploads(&self) -> Vec<String> {
        self.states
            .iter()
            .filter(|(_, state)| matches!(state, AssetLoadState::Decoded(_)))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Forgets an `Uploaded` path entirely so a later [`request`] reloads it
    /// through the normal loading path. Returns `true` if an entry was
    /// evicted. In-flight (`Loading`/`Decoded`) and `Error` entries are left
    /// alone: evicting a load mid-flight would leak its result, and errors are
    /// retained to avoid retry loops.
    ///
    /// [`request`]: AssetCache::request
    pub fn evict(&mut self, path: &str) -> bool {
        if matches!(self.states.get(path), Some(AssetLoadState::Uploaded)) {
            self.states.remove(path);
            true
        } else {
            false
        }
    }

    /// Takes the decoded image for `path`, transitioning it to `Uploaded` and
    /// releasing the cache's hold on the pixel buffer. Returns `None` if the
    /// path is not currently in the `Decoded` state.
    pub fn take_decoded(&mut self, path: &str) -> Option<Bc7Image> {
        let state = self.states.get_mut(path)?;
        if !matches!(state, AssetLoadState::Decoded(_)) {
            return None;
        }
        match std::mem::replace(state, AssetLoadState::Uploaded) {
            AssetLoadState::Decoded(decoded) => Some(decoded),
            _ => unreachable!("state was checked to be Decoded"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded() -> Bc7Image {
        Bc7Image {
            pixel_size: [1, 1],
            padded_size: [4, 4],
            blocks: vec![0; Bc7Image::expected_len([4, 4])],
        }
    }

    /// Delivers a result down a fresh channel and hands the receiver to
    /// `request`, mimicking a background decode that has already finished.
    fn ready_channel(
        result: Result<Bc7Image, ImageLoadError>,
    ) -> mpsc::Receiver<Result<Bc7Image, ImageLoadError>> {
        let (tx, rx) = mpsc::channel();
        tx.send(result).unwrap();
        rx
    }

    /// Guards against schema drift between the app's Map type and the
    /// bundled maps.ron (e.g. fields removed from one but not the other).
    #[test]
    fn bundled_maps_parse_and_reference_bundled_images() {
        let maps = load_maps().expect("bundled maps.ron should parse");
        assert!(!maps.is_empty(), "bundled maps.ron should contain maps");
        for map in &maps {
            assert!(
                Assets::get(&map.image_path).is_some(),
                "{} references missing asset {}",
                map.normalized_name,
                map.image_path
            );
        }
    }

    /// Main Floor only: every embedded map image must be some map's
    /// `image_path`. Catches per-floor images being reintroduced under
    /// `assets/maps/` without anything able to display them.
    #[test]
    fn every_bundled_image_is_a_map_main_floor() {
        let maps = load_maps().expect("bundled maps.ron should parse");
        let referenced: std::collections::HashSet<&str> =
            maps.iter().map(|map| map.image_path.as_str()).collect();
        let orphans: Vec<_> = Assets::iter()
            .filter(|path| path.starts_with("maps/"))
            .filter(|path| !referenced.contains(path.as_ref()))
            .collect();
        assert!(
            orphans.is_empty(),
            "embedded images no map uses: {orphans:?}"
        );
    }

    #[test]
    fn request_starts_a_load() {
        let mut cache = AssetCache::new();

        let started = cache.request("maps/customs.png", || ready_channel(Ok(decoded())));

        assert!(started, "first request should start a load");
        assert!(matches!(
            cache.state("maps/customs.png"),
            Some(AssetLoadState::Loading(_))
        ));
    }

    #[test]
    fn request_is_not_repeated_while_loading() {
        let mut cache = AssetCache::new();
        cache.request("maps/customs.png", || ready_channel(Ok(decoded())));

        let mut spawned = false;
        let started = cache.request("maps/customs.png", || {
            spawned = true;
            ready_channel(Ok(decoded()))
        });

        assert!(!started, "a loading path should not be requested again");
        assert!(!spawned, "spawn must not run for an in-flight path");
    }

    #[test]
    fn poll_moves_loading_to_decoded() {
        let mut cache = AssetCache::new();
        cache.request("maps/customs.png", || ready_channel(Ok(decoded())));

        let errors = cache.poll();

        assert!(errors.is_empty(), "a successful decode reports no errors");
        assert!(matches!(
            cache.state("maps/customs.png"),
            Some(AssetLoadState::Decoded(_))
        ));
    }

    #[test]
    fn poll_moves_loading_to_error() {
        let mut cache = AssetCache::new();
        cache.request("maps/customs.png", || {
            ready_channel(Err(ImageLoadError::AssetNotFound(
                "maps/customs.png".into(),
            )))
        });

        let errors = cache.poll();

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("maps/customs.png"));
        assert!(matches!(
            cache.state("maps/customs.png"),
            Some(AssetLoadState::Error(_))
        ));
    }

    #[test]
    fn poll_reports_a_dropped_sender_as_error() {
        let mut cache = AssetCache::new();
        cache.request("maps/customs.png", || {
            let (_, rx) = mpsc::channel();
            rx // sender dropped immediately -> disconnected
        });

        let errors = cache.poll();

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            cache.state("maps/customs.png"),
            Some(AssetLoadState::Error(_))
        ));
    }

    #[test]
    fn take_decoded_releases_pixels_and_marks_uploaded() {
        let mut cache = AssetCache::new();
        cache.request("maps/customs.png", || ready_channel(Ok(decoded())));
        cache.poll();

        assert_eq!(
            cache.pending_uploads(),
            vec!["maps/customs.png".to_string()]
        );

        let taken = cache.take_decoded("maps/customs.png");

        assert!(taken.is_some(), "decoded pixels should be handed out once");
        assert!(matches!(
            cache.state("maps/customs.png"),
            Some(AssetLoadState::Uploaded)
        ));
        assert!(
            cache.pending_uploads().is_empty(),
            "no uploads remain once decoded pixels are taken"
        );
        assert!(
            cache.take_decoded("maps/customs.png").is_none(),
            "an uploaded path yields no further pixel buffers"
        );
    }

    #[test]
    fn evict_forgets_an_uploaded_path_so_it_reloads() {
        let mut cache = AssetCache::new();
        cache.request("maps/customs.png", || ready_channel(Ok(decoded())));
        cache.poll();
        cache.take_decoded("maps/customs.png");

        assert!(
            cache.evict("maps/customs.png"),
            "uploaded paths are evictable"
        );
        assert!(
            cache.state("maps/customs.png").is_none(),
            "an evicted path is forgotten entirely"
        );

        let mut spawned = false;
        let started = cache.request("maps/customs.png", || {
            spawned = true;
            ready_channel(Ok(decoded()))
        });
        assert!(
            started && spawned,
            "an evicted path reloads via the normal path"
        );
    }

    #[test]
    fn evict_leaves_inflight_and_failed_loads_alone() {
        let mut cache = AssetCache::new();
        cache.request("maps/loading.png", || ready_channel(Ok(decoded())));
        cache.request("maps/broken.png", || {
            ready_channel(Err(ImageLoadError::AssetNotFound("maps/broken.png".into())))
        });
        cache.poll(); // loading.png -> Decoded, broken.png -> Error

        assert!(
            !cache.evict("maps/loading.png"),
            "decoded pixels are not evicted"
        );
        assert!(
            !cache.evict("maps/broken.png"),
            "errors are kept to avoid retry loops"
        );
        assert!(!cache.evict("maps/never-requested.png"));
        assert!(matches!(
            cache.state("maps/loading.png"),
            Some(AssetLoadState::Decoded(_))
        ));
        assert!(matches!(
            cache.state("maps/broken.png"),
            Some(AssetLoadState::Error(_))
        ));
    }

    #[test]
    fn request_is_not_repeated_after_upload() {
        let mut cache = AssetCache::new();
        cache.request("maps/customs.png", || ready_channel(Ok(decoded())));
        cache.poll();
        cache.take_decoded("maps/customs.png");

        let mut spawned = false;
        let started = cache.request("maps/customs.png", || {
            spawned = true;
            ready_channel(Ok(decoded()))
        });

        assert!(!started, "an uploaded path should not be requested again");
        assert!(!spawned, "spawn must not run for an uploaded path");
    }
}
