//! Color constants for map overlays and UI elements.

use eframe::egui::Color32;

// Spawn markers
pub const SPAWN_FILL: Color32 = Color32::from_rgb(50, 205, 50);
pub const SPAWN_STROKE: Color32 = Color32::from_rgb(0, 100, 0);

// PMC extract markers
pub const PMC_EXTRACT_FILL: Color32 = Color32::from_rgb(65, 105, 225);
pub const PMC_EXTRACT_STROKE: Color32 = Color32::from_rgb(25, 25, 112);

// Scav extract markers
pub const SCAV_EXTRACT_FILL: Color32 = Color32::from_rgb(255, 165, 0);
pub const SCAV_EXTRACT_STROKE: Color32 = Color32::from_rgb(139, 69, 19);

// Shared extract markers
pub const SHARED_EXTRACT_FILL: Color32 = Color32::from_rgb(186, 85, 211);
pub const SHARED_EXTRACT_STROKE: Color32 = Color32::from_rgb(75, 0, 130);

// Shared marker Overlay language
pub const MARKER_DISC: Color32 = Color32::from_rgba_unmultiplied_const(25, 25, 25, 235);
pub const TRANSIT: Color32 = Color32::from_rgb(80, 220, 230);
#[allow(dead_code)] // Reserved for the Switches Overlay ticket.
pub const SWITCH: Color32 = Color32::from_rgb(255, 225, 60);
#[allow(dead_code)] // Reserved for the BTR Stop Overlay ticket.
pub const BTR_STOP: Color32 = Color32::from_rgb(240, 180, 40);
#[allow(dead_code)] // Reserved for the Boss Spawn Overlay ticket.
pub const BOSS_BONE: Color32 = Color32::from_rgb(245, 245, 245);

// Player marker
pub const PLAYER_MARKER_FILL: Color32 = Color32::from_rgb(255, 50, 50);
pub const PLAYER_MARKER_STROKE: Color32 = Color32::from_rgb(139, 0, 0);

// Sniper Zone areas
pub const SNIPER_ZONE_FILL: Color32 = Color32::from_rgba_unmultiplied_const(255, 60, 60, 20);
pub const SNIPER_ZONE_STROKE: Color32 = Color32::from_rgba_unmultiplied_const(255, 70, 70, 240);

// Minefield areas
pub const MINEFIELD_FILL: Color32 = Color32::from_rgba_unmultiplied_const(255, 140, 0, 30);
pub const MINEFIELD_STROKE: Color32 = Color32::from_rgba_unmultiplied_const(255, 150, 20, 240);

// Text colors
pub const LABEL_TEXT: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 220);
pub const LABEL_SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 180);
pub const EXTRACT_TEXT_SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 200);

// Position tracking status
pub const TRACKING_LIVE: Color32 = Color32::from_rgb(92, 214, 122);
pub const TRACKING_STALE: Color32 = Color32::from_rgb(230, 170, 60);
pub const TRACKING_OFF: Color32 = Color32::from_rgb(120, 120, 120);
