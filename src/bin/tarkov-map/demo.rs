//! Demo position source: synthesises player fixes without a running game.
//!
//! Enabled with the `TARKOV_MAP_DEMO` environment variable. Every few
//! seconds a new fix is emitted, wandering around the centre of the selected
//! map, so the position card and Center can be
//! shown without Tarkov installed.

use crate::screenshot_watcher::PlayerPosition;
use std::time::{Duration, Instant, SystemTime};
use tarkov_map::Map;

/// Interval between synthetic fixes.
const STEP: Duration = Duration::from_secs(5);

/// Wander radius as a fraction of the map's shorter side.
const WANDER: f64 = 0.08;

pub struct DemoWalker {
    started: Instant,
    next_step: Instant,
    step: u32,
    map: Option<String>,
}

impl DemoWalker {
    /// Returns a walker if `TARKOV_MAP_DEMO` is set (to anything but `0`).
    pub fn from_env() -> Option<Self> {
        let value = std::env::var("TARKOV_MAP_DEMO").ok()?;
        if value == "0" {
            return None;
        }
        log::info!("Demo mode: synthesising player positions (TARKOV_MAP_DEMO)");
        let now = Instant::now();
        Some(Self {
            started: now,
            next_step: now,
            step: 0,
            map: None,
        })
    }

    /// Emits a fresh fix when it is time for one. Restarts the walk from the
    /// centre when the selected map changes.
    pub fn poll(&mut self, map: &Map) -> Option<PlayerPosition> {
        let now = Instant::now();
        if self.map.as_deref() != Some(map.normalized_name.as_str()) {
            self.map = Some(map.normalized_name.clone());
            self.step = 0;
            self.next_step = now;
        }
        if now < self.next_step {
            return None;
        }
        self.next_step = now + STEP;
        self.step += 1;

        let bounds = map.bounds?;
        let (x0, z0, x1, z1) = (bounds[0][0], bounds[0][1], bounds[1][0], bounds[1][1]);
        let center = [(x0 + x1) / 2.0, (z0 + z1) / 2.0];
        let radius = (x1 - x0).abs().min((z1 - z0).abs()) * WANDER;

        // A slow loop around the centre; yaw follows the direction of travel.
        let angle = f64::from(self.step) * 0.6;
        let position = [
            center[0] + angle.cos() * radius,
            0.0,
            center[1] + angle.sin() * radius,
        ];
        let yaw = (angle + std::f64::consts::FRAC_PI_2) as f32;

        log::debug!(
            "demo fix #{} after {:?}: [{:.1}, {:.1}, {:.1}]",
            self.step,
            now - self.started,
            position[0],
            position[1],
            position[2]
        );
        Some(PlayerPosition {
            position,
            yaw,
            taken_at: SystemTime::now(),
        })
    }
}
