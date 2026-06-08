//! Rook colour theme — dark palette, amber accents (like Resolve).
//! Will be wired into egui::Visuals in main.rs when customisation is needed.

pub const BG_DARK: egui::Color32 = egui::Color32::from_rgb(18, 18, 18);
pub const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(28, 28, 28);
pub const BG_TRACK_ODD: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
pub const BG_TRACK_EVEN: egui::Color32 = egui::Color32::from_rgb(20, 20, 20);
pub const CLIP_VIDEO: egui::Color32 = egui::Color32::from_rgb(60, 100, 160);
pub const CLIP_AUDIO: egui::Color32 = egui::Color32::from_rgb(60, 140, 80);
pub const CLIP_SELECTED: egui::Color32 = egui::Color32::from_rgb(200, 140, 40);
pub const PLAYHEAD: egui::Color32 = egui::Color32::RED;
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(255, 160, 40);
