//! Screenshot watcher for tracking player position from Tarkov screenshots.
//!
//! Tarkov saves coordinates in screenshot filenames in the format:
//! `2026-01-07[19-56]_-198.89, 22.74, -345.97_0.32263, 0.47266, -0.18602, 0.79869_15.61 (0).png`
//!                    ^--- position (x, y, z) ---^  ^--- quaternion (x, y, z, w) ---^

use eframe::egui;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Player position and rotation data extracted from a screenshot filename.
#[derive(Debug, Clone, Copy)]
pub struct PlayerPosition {
    /// Position in game coordinates [x, y, z] where y is height
    pub position: [f64; 3],
    /// Yaw rotation in radians (direction the player is facing)
    pub yaw: f32,
}

/// Watches the Tarkov screenshots folder for new screenshots and extracts player position.
pub struct ScreenshotWatcher {
    /// Receiver for position updates from the file watcher
    position_rx: Receiver<PlayerPosition>,
    /// The watcher must be kept alive for events to fire
    _watcher: RecommendedWatcher,
    /// Current player position (most recent)
    current_position: Option<PlayerPosition>,
}

impl ScreenshotWatcher {
    /// Creates a new screenshot watcher.
    ///
    /// Returns `None` if the screenshots folder doesn't exist or watching fails.
    pub fn new(ctx: egui::Context) -> Option<Self> {
        let screenshots_path = Self::screenshots_path()?;

        if !screenshots_path.exists() {
            log::warn!(
                "Screenshots folder does not exist: {}",
                screenshots_path.display()
            );
            return None;
        }

        let (position_tx, position_rx) = mpsc::channel();

        // Find and parse the newest screenshot on startup
        let initial_position = Self::find_newest_screenshot(&screenshots_path)
            .and_then(|path| Self::parse_screenshot_filename(&path));

        if let Some(pos) = initial_position {
            log::info!(
                "Initial player position: [{:.2}, {:.2}, {:.2}], yaw: {:.2}°",
                pos.position[0],
                pos.position[1],
                pos.position[2],
                pos.yaw.to_degrees()
            );
        }

        // Set up file watcher
        let tx = position_tx.clone();
        let ctx_clone = ctx.clone();
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                // Only handle file creation events
                if matches!(event.kind, EventKind::Create(_)) {
                    for path in event.paths {
                        if path.extension().is_some_and(|ext| ext == "png")
                            && let Some(position) = Self::parse_screenshot_filename(&path)
                        {
                            log::info!(
                                "New player position: [{:.2}, {:.2}, {:.2}], yaw: {:.2}°",
                                position.position[0],
                                position.position[1],
                                position.position[2],
                                position.yaw.to_degrees()
                            );
                            let _ = tx.send(position);
                            ctx_clone.request_repaint();
                        }
                    }
                }
            }
        })
        .ok()?;

        watcher
            .watch(&screenshots_path, RecursiveMode::NonRecursive)
            .ok()?;

        log::info!(
            "Watching screenshots folder: {}",
            screenshots_path.display()
        );

        Some(Self {
            position_rx,
            _watcher: watcher,
            current_position: initial_position,
        })
    }

    /// Returns the path to the Tarkov screenshots folder.
    fn screenshots_path() -> Option<PathBuf> {
        let documents = dirs::document_dir()?;
        Some(documents.join("Escape from Tarkov").join("Screenshots"))
    }

    /// Finds the newest PNG screenshot in the given directory.
    fn find_newest_screenshot(dir: &PathBuf) -> Option<PathBuf> {
        fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
            .max_by_key(|entry| entry.metadata().ok().and_then(|m| m.modified().ok()))
            .map(|entry| entry.path())
    }

    /// Parses a screenshot filename to extract player position and rotation.
    ///
    /// Expected format: `DATE[TIME]_X, Y, Z_QX, QY, QZ, QW_OTHER (N).png`
    fn parse_screenshot_filename(path: &Path) -> Option<PlayerPosition> {
        let filename = path.file_name()?.to_str()?;

        // Regex to match the position and quaternion in the filename
        // Format: ..._X, Y, Z_QX, QY, QZ, QW_...
        let re = Regex::new(
            r"_(?<x>-?[\d]+\.[\d]+), (?<y>-?[\d]+\.[\d]+), (?<z>-?[\d]+\.[\d]+)_(?<qx>-?[\d]+\.[\d]+), (?<qy>-?[\d]+\.[\d]+), (?<qz>-?[\d]+\.[\d]+), (?<qw>-?[\d]+\.[\d]+)_",
        )
        .ok()?;

        let caps = re.captures(filename)?;

        let x: f64 = caps.name("x")?.as_str().parse().ok()?;
        let y: f64 = caps.name("y")?.as_str().parse().ok()?;
        let z: f64 = caps.name("z")?.as_str().parse().ok()?;

        let qx: f32 = caps.name("qx")?.as_str().parse().ok()?;
        let qy: f32 = caps.name("qy")?.as_str().parse().ok()?;
        let qz: f32 = caps.name("qz")?.as_str().parse().ok()?;
        let qw: f32 = caps.name("qw")?.as_str().parse().ok()?;

        let yaw = quaternion_to_yaw(qx, qy, qz, qw);

        Some(PlayerPosition {
            position: [x, y, z],
            yaw,
        })
    }

    /// Polls for new position updates and returns the current position.
    pub fn poll(&mut self) -> Option<PlayerPosition> {
        // Drain all pending updates, keeping only the most recent
        loop {
            match self.position_rx.try_recv() {
                Ok(position) => {
                    self.current_position = Some(position);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    log::warn!("Screenshot watcher channel disconnected");
                    break;
                }
            }
        }

        self.current_position
    }
}

/// Converts a quaternion rotation to yaw angle in radians.
///
/// Based on the TarkovMonitor implementation which uses parameter order (x, z, y, w)
/// meaning y and z are swapped in the formula relative to standard quaternion conventions.
fn quaternion_to_yaw(x: f32, y: f32, z: f32, w: f32) -> f32 {
    // TarkovMonitor's formula with their (x, z, y, w) convention:
    // siny_cosp = 2 * (w * z + x * y) where their z=our y, their y=our z
    // So we need: 2 * (w * y + x * z)
    let siny_cosp = 2.0 * (w * y + x * z);
    let cosy_cosp = 1.0 - 2.0 * (z * z + y * y);
    f32::atan2(siny_cosp, cosy_cosp)
}
