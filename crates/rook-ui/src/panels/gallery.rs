//! Gallery panel — asset bin with thumbnails, search, sort, and favorites.
//!
//! Features:
//! - Real video thumbnails from ThumbnailCache
//! - Search/filter by filename, codec, AI labels
//! - Sort by name, duration, type, date
//! - Favorites toggle (star ⭐)
//! - Asset metadata: resolution, codec, fps, duration

use crate::widgets::thumbnail::ThumbnailCache;
use rook_core::ids::AssetId;
use rook_engine::Engine;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortMode {
    Name,
    Duration,
    Kind,
}

impl SortMode {
    fn label(&self) -> &str {
        match self {
            SortMode::Name => "Name",
            SortMode::Duration => "Duration",
            SortMode::Kind => "Kind",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Grid,
    List,
}

/// A saved search filter (smart collection).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct SmartCollection {
    name: String,
    query: String,
}

pub struct GalleryPanel {
    search: String,
    sort: SortMode,
    view: ViewMode,
    favorites: HashSet<AssetId>,
    selected_asset: Option<AssetId>,
    show_metadata: bool,
    /// Smart collections — saved search filters.
    smart_collections: Vec<SmartCollection>,
    /// Whether we're showing the smart collection editor.
    editing_smart: bool,
    /// Name of new smart collection being created.
    new_smart_name: String,
}

impl Default for GalleryPanel {
    fn default() -> Self {
        Self {
            search: String::new(),
            sort: SortMode::Name,
            view: ViewMode::Grid,
            favorites: HashSet::new(),
            selected_asset: None,
            show_metadata: false,
            smart_collections: Vec::new(),
            editing_smart: false,
            new_smart_name: String::new(),
        }
    }
}

impl GalleryPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, engine: &mut Engine, thumb_cache: &ThumbnailCache) {
        ui.heading("📁 Project Assets");

        // Clone assets to avoid borrow conflicts with mutable operations later
        let assets: Vec<rook_core::asset::Asset> = engine.project().assets.clone();
        let fps = engine.project().frame_rate.as_f64();

        // ── Smart Collections ──────────────────────────────────────────
        if !self.smart_collections.is_empty() {
            ui.collapsing("📂 Smart Collections", |ui| {
                let mut to_remove: Option<usize> = None;
                for (i, sc) in self.smart_collections.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(false, &sc.name).clicked() {
                            self.search = sc.query.clone();
                        }
                        if ui.button("✕").on_hover_text("Remove collection").clicked() {
                            to_remove = Some(i);
                        }
                    });
                }
                if let Some(i) = to_remove {
                    self.smart_collections.remove(i);
                }
                ui.separator();
            });
        }
        // Save current search as smart collection
        ui.horizontal(|ui| {
            if ui
                .small_button("💾 Save Search")
                .on_hover_text("Save current search as a smart collection")
                .clicked()
            {
                self.editing_smart = !self.editing_smart;
                if self.editing_smart {
                    self.new_smart_name.clear();
                }
            }
        });
        if self.editing_smart {
            ui.horizontal(|ui| {
                ui.label("Name:");
                let resp = ui.text_edit_singleline(&mut self.new_smart_name);
                if ui.button("✓ Save").clicked() && !self.new_smart_name.trim().is_empty() {
                    self.smart_collections.push(SmartCollection {
                        name: self.new_smart_name.trim().to_string(),
                        query: self.search.clone(),
                    });
                    self.editing_smart = false;
                    self.new_smart_name.clear();
                }
                if ui.button("✕ Cancel").clicked() {
                    self.editing_smart = false;
                    self.new_smart_name.clear();
                }
            });
            {
                let _ = self.new_smart_name; // keep it referenced
            }
        }
        ui.separator();

        // ── Search + Sort toolbar ─────────────────────────────────────
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("🔍 Filter by name, codec, tag…")
                    .desired_width(ui.available_width() - 60.0),
            );
            if ui.button("✕").clicked() {
                self.search.clear();
            }
            // Used/Unused quick filter
            if ui
                .small_button("Used")
                .on_hover_text("Show only assets used on timeline")
                .clicked()
            {
                self.search = "used:".to_string();
            }
            if ui
                .small_button("Unused")
                .on_hover_text("Show only unused assets")
                .clicked()
            {
                self.search = "unused:".to_string();
            }
        });

        ui.horizontal(|ui| {
            ui.label("Sort:");
            egui::ComboBox::from_id_salt("gallery_sort")
                .selected_text(self.sort.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sort, SortMode::Name, "Name");
                    ui.selectable_value(&mut self.sort, SortMode::Duration, "Duration");
                    ui.selectable_value(&mut self.sort, SortMode::Kind, "Kind");
                });

            ui.separator();

            let fav_count = self.favorites.len();
            if fav_count > 0 {
                if ui
                    .selectable_label(false, format!("⭐ Favorites ({})", fav_count))
                    .clicked()
                {
                    self.search = "fav:".to_string();
                }
            }
        });

        ui.separator();

        if assets.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("No assets imported").size(14.0));
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Use File → Import Media or ⌘I")
                        .size(11.0)
                        .color(egui::Color32::from_gray(140)),
                );
            });
            return;
        }

        // ── Filter assets ──────────────────────────────────────────────
        let filter_favs = self.search.starts_with("fav:");
        let filter_used = self.search.starts_with("used:");
        let filter_unused = self.search.starts_with("unused:");
        let search_term = if filter_favs {
            self.search[4..].trim().to_lowercase()
        } else if filter_used {
            self.search[5..].trim().to_lowercase()
        } else if filter_unused {
            self.search[7..].trim().to_lowercase()
        } else {
            self.search.trim().to_lowercase()
        };

        // Collect used asset IDs from timeline
        let used_ids: std::collections::HashSet<AssetId> = engine
            .project()
            .timeline
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.asset_id)
            .collect();

        let mut filtered: Vec<&rook_core::asset::Asset> = assets
            .iter()
            .filter(|a| {
                // Used/Unused filter
                if filter_used && !used_ids.contains(&a.id()) {
                    return false;
                }
                if filter_unused && used_ids.contains(&a.id()) {
                    return false;
                }
                // Favorites filter
                if filter_favs && !self.favorites.contains(&a.id()) {
                    return false;
                }
                // Text search
                if search_term.is_empty() {
                    return true;
                }
                let name = a.filename_stem().to_lowercase();
                if name.contains(&search_term) {
                    return true;
                }
                let codec = match a {
                    rook_core::asset::Asset::Video(v) => {
                        v.metadata.video.as_ref().map(|m| m.codec.to_lowercase())
                    }
                    rook_core::asset::Asset::Audio(a) => {
                        a.metadata.audio.as_ref().map(|m| m.codec.to_lowercase())
                    }
                    _ => None,
                };
                if let Some(ref c) = codec {
                    if c.contains(&search_term) {
                        return true;
                    }
                }
                let ai = &a.metadata().ai_labels;
                if ai.iter().any(|l| l.to_lowercase().contains(&search_term)) {
                    return true;
                }
                if let Some(ref desc) = a.metadata().ai_description {
                    if desc.to_lowercase().contains(&search_term) {
                        return true;
                    }
                }
                false
            })
            .collect();

        // ── Sort ───────────────────────────────────────────────────────
        match self.sort {
            SortMode::Name => filtered.sort_by_key(|a| a.filename_stem().to_lowercase()),
            SortMode::Duration => {
                filtered.sort_by_key(|a| -(a.metadata().duration_frames.unwrap_or(0)))
            }
            SortMode::Kind => filtered.sort_by_key(|a| match a {
                rook_core::asset::Asset::Video(_) => 0u8,
                rook_core::asset::Asset::Audio(_) => 1,
                rook_core::asset::Asset::Image(_) => 2,
                rook_core::asset::Asset::Subtitle(_) => 3,
            }),
        }

        ui.label(
            egui::RichText::new(format!("{} of {} assets", filtered.len(), assets.len()))
                .size(10.0)
                .color(egui::Color32::from_gray(140)),
        );

        // ── Asset grid ─────────────────────────────────────────────────
        egui::ScrollArea::vertical().show(ui, |ui| {
            let cell_w = 108.0;
            let cols = (ui.available_width() / cell_w).floor().max(1.0) as usize;

            egui::Grid::new("gallery_grid")
                .min_col_width(cell_w)
                .max_col_width(cell_w)
                .show(ui, |ui| {
                    for (i, asset) in filtered.iter().enumerate() {
                        let asset_id = asset.id();
                        let is_fav = self.favorites.contains(&asset_id);
                        let is_selected = self.selected_asset == Some(asset_id);

                        // ── Thumbnail ───────────────────────────────
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(96.0, 64.0), egui::Sense::click());
                        let bg = if is_selected {
                            egui::Color32::from_rgb(60, 80, 120)
                        } else {
                            egui::Color32::from_gray(28)
                        };
                        ui.painter().rect_filled(rect, 3.0, bg);

                        // Try to show real thumbnail — with filmstrip hover-scrub
                        let path = std::path::PathBuf::from(asset.path());
                        if let Some(strip) = thumb_cache.get_or_extract(asset_id, &path, fps) {
                            // Determine which frame to show based on hover position
                            let hover_pos = ui.ctx().pointer_hover_pos();
                            let thumb_idx = if let Some(pos) = hover_pos {
                                if rect.contains(pos) && rect.width() > 0.0 {
                                    let frac =
                                        ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                                    // Map to thumbnail index
                                    let idx = (frac * strip.thumbs.len() as f32) as usize;
                                    idx.min(strip.thumbs.len().saturating_sub(1))
                                } else {
                                    0 // default to first frame
                                }
                            } else {
                                0
                            };
                            if let Some(thumb) = strip.thumbs.get(thumb_idx) {
                                let tex_id =
                                    thumb_cache.texture(ui.ctx(), asset_id, thumb_idx, thumb);
                                ui.painter().image(
                                    tex_id,
                                    rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    egui::Color32::WHITE,
                                );
                                // Show frame indicator when scrubbing
                                if thumb_idx > 0 {
                                    let dur_frames = asset.metadata().duration_frames.unwrap_or(1);
                                    let frame = (thumb_idx as f32 / strip.thumbs.len() as f32
                                        * dur_frames as f32)
                                        as i64;
                                    let secs = frame as f64 / fps;
                                    ui.painter().text(
                                        egui::pos2(rect.center().x, rect.bottom() - 2.0),
                                        egui::Align2::CENTER_BOTTOM,
                                        format!("{:.1}s", secs),
                                        egui::FontId::proportional(9.0),
                                        egui::Color32::from_rgba_premultiplied(255, 255, 255, 200),
                                    );
                                }
                            }
                        }

                        // Type badge
                        let type_icon = match asset {
                            rook_core::asset::Asset::Video(_) => "🎬",
                            rook_core::asset::Asset::Audio(_) => "🎵",
                            rook_core::asset::Asset::Image(_) => "🖼",
                            rook_core::asset::Asset::Subtitle(_) => "📝",
                        };
                        ui.painter().text(
                            rect.right_top() + egui::vec2(-20.0, 2.0),
                            egui::Align2::RIGHT_TOP,
                            type_icon,
                            egui::FontId::proportional(12.0),
                            egui::Color32::WHITE,
                        );

                        // Favorite star
                        if is_fav {
                            ui.painter().text(
                                rect.left_top() + egui::vec2(3.0, 2.0),
                                egui::Align2::LEFT_TOP,
                                "⭐",
                                egui::FontId::proportional(14.0),
                                egui::Color32::from_rgb(255, 200, 40),
                            );
                        }

                        if resp.clicked() {
                            self.selected_asset = Some(asset_id);
                        }
                        if resp.double_clicked() {
                            self.favorites_toggle(asset_id);
                        }

                        // ── Name ─────────────────────────────────────
                        let name = asset.filename_stem();
                        let short = if name.len() > 14 {
                            format!("{}…", &name[..13])
                        } else {
                            name.to_string()
                        };
                        ui.label(egui::RichText::new(&short).size(10.0));

                        // ── Duration / metadata ──────────────────────
                        if let Some(dur) = asset.metadata().duration_frames {
                            let secs = dur as f64 / fps;
                            let dur_str = if secs >= 60.0 {
                                format!("{}:{:02.0}", (secs / 60.0) as i64, secs % 60.0)
                            } else {
                                format!("{:.1}s", secs)
                            };
                            ui.label(
                                egui::RichText::new(dur_str)
                                    .size(9.0)
                                    .color(egui::Color32::from_gray(150)),
                            );
                        }

                        ui.end_row();
                    }
                });
        });

        // ── Selected asset metadata ────────────────────────────────────
        if let Some(sel_id) = self.selected_asset {
            if let Some(asset) = assets.iter().find(|a| a.id() == sel_id) {
                ui.separator();
                ui.collapsing("📋 Asset Info", |ui| {
                    ui.label(egui::RichText::new(asset.filename_stem()).strong());
                    ui.label(format!("Path: {}", asset.path()));
                    let is_fav = self.favorites.contains(&sel_id);
                    let fav_label = if is_fav { "⭐ Unfavorite" } else { "☆ Favorite" };
                    if ui.button(fav_label).clicked() {
                        self.favorites_toggle(sel_id);
                    }

                    // Relink button — check if file exists
                    let path_exists = std::path::Path::new(asset.path()).exists();
                    if !path_exists {
                        ui.colored_label(egui::Color32::from_rgb(255, 100, 80),
                            "⚠ Media file not found");
                        if ui.button("🔗 Relink…").clicked() {
                            if let Some(new_path) = rfd::FileDialog::new()
                                .add_filter("Media", &["mp4", "mov", "m4v", "mkv", "webm", "mp3", "wav", "jpg", "png"])
                                .pick_file()
                            {
                                if let Err(e) = engine.relink_asset(sel_id, &new_path) {
                                    tracing::error!(?e, "relink failed");
                                } else {
                                    tracing::info!(asset_id = ?sel_id, new_path = %new_path.display(), "asset relinked");
                                }
                            }
                        }
                    }

                    // ── Proxy status ────────────────────────────────
                    let proxy_status = engine.proxy().status(sel_id);
                    match proxy_status {
                        Some(rook_engine::ProxyStatus::Ready(ref path)) => {
                            ui.label(format!("📽 Proxy: {}", path.display()));
                            if ui.small_button("🗑 Delete Proxy").clicked() {
                                let _ = std::fs::remove_file(path);
                            }
                        }
                        Some(rook_engine::ProxyStatus::Building { progress }) => {
                            ui.label(format!("📽 Building proxy… {:.0}%", progress * 100.0));
                            ui.add(egui::ProgressBar::new(progress)
                                .desired_width(ui.available_width())
                                .animate(true));
                        }
                        Some(rook_engine::ProxyStatus::Failed(ref err)) => {
                            ui.colored_label(egui::Color32::from_rgb(255, 140, 80),
                                format!("📽 Proxy failed: {}", err));
                            if ui.small_button("🔄 Retry").clicked() {
                                let path = std::path::PathBuf::from(asset.path());
                                engine.proxy().request_proxy(sel_id, &path);
                            }
                        }
                        _ => {
                            // Check if it's a video asset (worth proxying)
                            if matches!(asset, rook_core::asset::Asset::Video(_)) {
                                if ui.small_button("📽 Generate Proxy").clicked() {
                                    let path = std::path::PathBuf::from(asset.path());
                                    engine.proxy().request_proxy(sel_id, &path);
                                }
                            }
                        }
                    }

                    if let Some(fs) = asset.metadata().file_size_bytes {
                        let mb = fs as f64 / 1_048_576.0;
                        ui.label(format!("Size: {:.1} MB", mb));
                    }
                    if let Some(dur) = asset.metadata().duration_frames {
                        let secs = dur as f64 / fps;
                        ui.label(format!("Duration: {:.1}s ({} frames)", secs, dur));
                    }
                    if let rook_core::asset::Asset::Video(v) = asset {
                        if let Some(ref vm) = v.metadata.video {
                            ui.label(format!("Video: {}×{}, {}, {:.0} fps",
                                vm.width, vm.height, vm.codec, vm.fps));
                            ui.label(format!("Audio: {}", if vm.has_audio { "Yes" } else { "No" }));
                        }
                    }
                    if let rook_core::asset::Asset::Audio(a) = asset {
                        if let Some(ref am) = a.metadata.audio {
                            ui.label(format!("Audio: {}, {} Hz, {} ch",
                                am.codec, am.sample_rate, am.channels));
                        }
                    }
                    if !asset.metadata().ai_labels.is_empty() {
                        ui.label(format!("Tags: {}", asset.metadata().ai_labels.join(", ")));
                    }
                    // Add tag input
                    ui.horizontal(|ui| {
                        let mut new_tag = String::new();
                        let resp = ui.add(egui::TextEdit::singleline(&mut new_tag)
                            .hint_text("Add tag…").desired_width(80.0));
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let trimmed = new_tag.trim().to_string();
                            if !trimmed.is_empty() {
                                if let Some(asset) = engine.project_mut().asset_mut(sel_id) {
                                    asset.metadata_mut().ai_labels.push(trimmed);
                                }
                            }
                        }
                    });
                    if let Some(ref desc) = asset.metadata().ai_description {
                        ui.label(desc);
                    }
                });
            }
        }

        ui.separator();
        ui.label(
            egui::RichText::new(format!(
                "{} assets · {} ⭐",
                assets.len(),
                self.favorites.len()
            ))
            .size(11.0)
            .color(egui::Color32::from_gray(140)),
        );
    }

    fn favorites_toggle(&mut self, id: AssetId) {
        if self.favorites.contains(&id) {
            self.favorites.remove(&id);
        } else {
            self.favorites.insert(id);
        }
    }
}
