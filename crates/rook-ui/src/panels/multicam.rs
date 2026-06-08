//! Multicam angle viewer panel — shows all angles in a grid and allows
//! live switching during playback.

use egui::{Color32, Vec2};
use rook_core::multicam::{MulticamAudioPolicy, MulticamClip, MulticamSyncMethod};
use rook_engine::Engine;

/// The multicam angle viewer panel.
#[derive(Default)]
pub struct MulticamPanel {
    /// Whether to show the audio policy controls.
    pub show_audio_controls: bool,
}

impl MulticamPanel {
    /// Render the multicam angle grid and controls.
    /// Returns `Some(angle_index)` if the user clicked to switch angles.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        engine: &mut Engine,
        selected_clip_ids: &[rook_core::ClipId],
    ) -> Option<usize> {
        let mut switched: Option<usize> = None;

        ui.heading("📷 Multicam Angles");
        ui.separator();

        // Find the multicam clip for the selected clip
        let selected_mc = selected_clip_ids
            .first()
            .and_then(|cid| engine.project().multicam_for_clip(*cid));

        if let Some(mc) = selected_mc {
            // Clone to avoid borrow issues
            let mc_data = mc.clone();
            let active_idx = mc_data.active_angle_index;
            let angle_count = mc_data.angle_count();

            ui.label(format!("{} — {} angles", mc_data.label, angle_count));
            ui.label(format!(
                "Active: {}",
                mc_data
                    .active_angle()
                    .map(|a| a.label.as_str())
                    .unwrap_or("none")
            ));

            ui.separator();

            // ── Angle grid ──────────────────────────────────────────────
            let cols = if angle_count <= 2 { angle_count } else { 3 };
            let cell_width = (ui.available_width() / cols as f32) - 8.0;
            let cell_height = cell_width * 0.5625; // 16:9 aspect

            egui::Grid::new("multicam_angles")
                .min_col_width(cell_width)
                .max_col_width(cell_width)
                .show(ui, |ui| {
                    for (i, angle) in mc_data.angles.iter().enumerate() {
                        let is_active = i == active_idx;
                        let fill = if is_active {
                            Color32::from_rgb(180, 220, 160) // green highlight
                        } else {
                            Color32::from_gray(40)
                        };

                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(cell_width, cell_height),
                            egui::Sense::click(),
                        );

                        // Draw angle cell background
                        ui.painter().rect_filled(rect, 4.0, fill);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            if is_active { "🔴 LIVE" } else { &angle.label },
                            egui::FontId::proportional(if is_active { 14.0 } else { 12.0 }),
                            if is_active {
                                Color32::BLACK
                            } else {
                                Color32::from_gray(200)
                            },
                        );

                        // Draw angle label below
                        let label_rect = egui::Rect::from_min_size(
                            rect.left_bottom() + egui::vec2(0.0, 2.0),
                            egui::vec2(cell_width, 18.0),
                        );
                        ui.painter().text(
                            label_rect.center(),
                            egui::Align2::CENTER_TOP,
                            format!(
                                "{} ({})",
                                angle.label,
                                if angle.enabled { "enabled" } else { "disabled" }
                            ),
                            egui::FontId::proportional(10.0),
                            if is_active {
                                Color32::WHITE
                            } else {
                                Color32::from_gray(160)
                            },
                        );

                        if resp.clicked() && angle.enabled {
                            switched = Some(i);
                        }

                        if i % cols == cols - 1 || i == angle_count - 1 {
                            ui.end_row();
                        }
                    }
                });

            ui.separator();

            // ── Audio policy ───────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Audio:");
                let mut policy = mc_data.audio_policy;
                let prev_policy = policy;
                egui::ComboBox::from_id_salt("mc_audio_policy")
                    .selected_text(match policy {
                        MulticamAudioPolicy::FollowVideo => "Follow Video",
                        MulticamAudioPolicy::MasterOnly => "Master Only",
                        MulticamAudioPolicy::Separate => "Separate",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut policy,
                            MulticamAudioPolicy::FollowVideo,
                            "Follow Video",
                        );
                        ui.selectable_value(
                            &mut policy,
                            MulticamAudioPolicy::MasterOnly,
                            "Master Only",
                        );
                        ui.selectable_value(&mut policy, MulticamAudioPolicy::Separate, "Separate");
                    });
                if policy != prev_policy {
                    let clip_id = mc_data.clip_id;
                    let cmd = rook_core::commands::EditCommand::SetMulticamAudioPolicy {
                        clip_id,
                        policy,
                    };
                    engine.apply(cmd).ok();
                }
            });

            // ── Switching shortcuts ─────────────────────────────────────
            ui.separator();
            ui.label("Switch: 1–9 keys or click angle above");
            ui.label("Next/Prev: ⌘→ / ⌘←");

            // ── Collapse button ─────────────────────────────────────────
            ui.separator();
            if ui.button("📎 Collapse to Active Angle").clicked() {
                let clip_id = mc_data.clip_id;
                let cmd = rook_core::commands::EditCommand::CollapseMulticam { clip_id };
                engine.apply(cmd).ok();
            }
        } else {
            ui.label("No multicam clip selected.");
            ui.separator();
            ui.label("Select multiple clips and use");
            ui.label("Edit → Create Multicam Clip");
            ui.label("to group them into a multicam clip.");

            // Show "Create Multicam" button if multiple clips selected
            if selected_clip_ids.len() >= 2 {
                ui.separator();
                if ui.button("🎬 Create Multicam Clip").clicked() {
                    let clip_ids = selected_clip_ids.to_vec();
                    let first_track = engine
                        .project()
                        .timeline
                        .tracks
                        .iter()
                        .find(|t| t.kind == rook_core::track::TrackKind::Video)
                        .map(|t| t.id);
                    if let Some(tid) = first_track {
                        let pos = clip_ids
                            .first()
                            .and_then(|cid| engine.project().timeline.clip(*cid))
                            .map(|c| c.timeline_in)
                            .unwrap_or(0);
                        let cmd = rook_core::commands::EditCommand::CreateMulticam {
                            clip_ids,
                            label: "Multicam 1".to_string(),
                            sync_method: MulticamSyncMethod::Waveform,
                            position: pos,
                            track_id: tid,
                        };
                        engine.apply(cmd).ok();
                    }
                }
            }
        }

        switched
    }
}
