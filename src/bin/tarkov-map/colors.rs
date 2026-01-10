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

// Text colors
pub const LABEL_TEXT: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 220);
pub const LABEL_SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 180);
pub const EXTRACT_TEXT_SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 200);
