//! Domain types — pure data + pure functions, no I/O, no time source.

pub mod map;
pub mod position;
pub mod spaces;

pub use map::{
    Attribution, CatalogError, Extract, Faction, Label, Map, MapCatalog, MapId, MapImageKey, Spawn,
};
pub use position::{
    FRESHNESS_THRESHOLD, Freshness, PlayerPosition, ScreenshotNameError, ScreenshotStamp,
    parse_screenshot_name,
};
pub use spaces::{
    Game, GameBox, GamePoint, Image, ImagePoint, ImageSize, ImageVector, Projection, Screen,
};
