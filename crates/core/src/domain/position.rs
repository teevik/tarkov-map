//! Player Position, Freshness, and the pure screenshot-filename parser.

use std::fmt;
use std::time::{Duration, SystemTime};

use euclid::Angle;

use super::spaces::GamePoint;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerPosition {
    /// game x, z — flows straight into the Projection
    pub ground: GamePoint,
    /// game y — displayed only, never used to choose what is drawn
    pub height: f64,
    pub heading: Angle<f64>,
    /// the screenshot file's mtime
    pub taken_at: SystemTime,
}

pub const FRESHNESS_THRESHOLD: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Live,
    Stale,
}

impl PlayerPosition {
    pub fn age(&self, now: SystemTime) -> Duration {
        now.duration_since(self.taken_at).unwrap_or_default()
    }

    pub fn freshness(&self, now: SystemTime) -> Freshness {
        if self.age(now) < FRESHNESS_THRESHOLD {
            Freshness::Live
        } else {
            Freshness::Stale
        }
    }
}

/// What a screenshot filename says; `taken_at` is the file's business, not the name's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenshotStamp {
    pub ground: GamePoint,
    pub height: f64,
    pub heading: Angle<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenshotNameError {
    NotAScreenshot,
    BadNumber { field: &'static str },
}

impl fmt::Display for ScreenshotNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAScreenshot => f.write_str("not a Tarkov screenshot filename"),
            Self::BadNumber { field } => write!(f, "field `{field}` is not a number"),
        }
    }
}

impl std::error::Error for ScreenshotNameError {}

/// Parses `DATE[TIME]_X, Y, Z_QX, QY, QZ, QW_OTHER (N).png` by hand (no regex).
pub fn parse_screenshot_name(name: &str) -> Result<ScreenshotStamp, ScreenshotNameError> {
    let stem = name
        .strip_suffix(".png")
        .or_else(|| name.strip_suffix(".PNG"))
        .ok_or(ScreenshotNameError::NotAScreenshot)?;
    let mut parts = stem.split('_');
    let _date = parts.next().ok_or(ScreenshotNameError::NotAScreenshot)?;
    let pos = parts.next().ok_or(ScreenshotNameError::NotAScreenshot)?;
    let quat = parts.next().ok_or(ScreenshotNameError::NotAScreenshot)?;

    let [x, y, z] = numbers::<3>(pos, ["x", "y", "z"])?;
    let [qx, qy, qz, qw] = numbers::<4>(quat, ["qx", "qy", "qz", "qw"])?;

    Ok(ScreenshotStamp {
        ground: euclid::point2(x, z),
        height: y,
        heading: Angle::radians(quaternion_to_yaw(qx, qy, qz, qw)),
    })
}

fn numbers<const N: usize>(
    s: &str,
    fields: [&'static str; N],
) -> Result<[f64; N], ScreenshotNameError> {
    let mut out = [0.0; N];
    let mut it = s.split(", ");
    for (slot, field) in out.iter_mut().zip(fields) {
        let raw = it.next().ok_or(ScreenshotNameError::NotAScreenshot)?;
        *slot = raw
            .parse()
            .map_err(|_| ScreenshotNameError::BadNumber { field })?;
    }
    if it.next().is_some() {
        return Err(ScreenshotNameError::NotAScreenshot);
    }
    Ok(out)
}

/// TarkovMonitor's formula, verbatim (their (x, z, y, w) convention).
fn quaternion_to_yaw(x: f64, y: f64, z: f64, w: f64) -> f64 {
    let siny_cosp = 2.0 * (w * y + x * z);
    let cosy_cosp = 1.0 - 2.0 * (z * z + y * y);
    siny_cosp.atan2(cosy_cosp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn parses_the_documented_example() {
        let s = parse_screenshot_name(
            "2024-01-02[12-34]_-12.5, 3.25, 48.0_0.0, 0.7071068, 0.0, 0.7071068_12345 (0).png",
        )
        .unwrap();
        assert_eq!(s.ground, euclid::point2(-12.5, 48.0));
        assert_eq!(s.height, 3.25);
        assert!((s.heading.radians - FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn identity_quaternion_is_zero_yaw() {
        let s = parse_screenshot_name("d_0.0, 0.0, 0.0_0.0, 0.0, 0.0, 1.0_x.png").unwrap();
        assert_eq!(s.heading.radians, 0.0);
    }

    #[test]
    fn rejects_non_screenshots_and_bad_numbers() {
        assert_eq!(
            parse_screenshot_name("notes.txt"),
            Err(ScreenshotNameError::NotAScreenshot)
        );
        assert_eq!(
            parse_screenshot_name("d_a, 0.0, 0.0_0.0, 0.0, 0.0, 1.0_x.png"),
            Err(ScreenshotNameError::BadNumber { field: "x" })
        );
    }

    #[test]
    fn freshness_flips_at_120s() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let p = PlayerPosition {
            ground: euclid::point2(0.0, 0.0),
            height: 0.0,
            heading: Angle::zero(),
            taken_at: t0,
        };
        assert_eq!(p.freshness(t0 + Duration::from_secs(119)), Freshness::Live);
        assert_eq!(p.freshness(t0 + Duration::from_secs(120)), Freshness::Stale);
    }
}
