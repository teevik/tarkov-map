/// Width of the sidebar panel in pixels.
pub const SIDEBAR_WIDTH: f32 = 220.0;

/// Height of the custom title bar in pixels.
pub const TITLE_BAR_HEIGHT: f32 = 32.0;

/// Minimum zoom level (1.0 = fit to viewport).
pub const ZOOM_MIN: f32 = 1.0;

/// Maximum zoom level.
pub const ZOOM_MAX: f32 = 10.0;

/// Zoom speed multiplier for scroll/keyboard zoom.
pub const ZOOM_SPEED: f32 = 1.2;

/// Points one wheel notch scrolls in egui (`line_scroll_speed` on native).
/// Scroll-zoom scales [`ZOOM_SPEED`] by delta/notch so the smoothed per-frame
/// deltas multiply back up to one zoom step per notch.
pub const POINTS_PER_SCROLL_NOTCH: f32 = 40.0;

/// Zoom applied when centering on the player from the fit view, so that
/// "center" visibly means something.
pub const CENTER_ZOOM: f32 = 2.5;

/// A position fix older than this is shown as stale in the position card.
pub const FRESH_FIX_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(120);

/// How long a map load may take before the "Loading …" placeholder appears.
/// Fast loads (the common case) show nothing at all, avoiding a flash.
pub const MAP_PLACEHOLDER_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

/// Duration of the fade-in when a newly selected map becomes ready.
pub const MAP_REVEAL_DURATION: std::time::Duration = std::time::Duration::from_millis(220);
