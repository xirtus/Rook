//! VU Meter panel — audio level monitoring.
//!
//! Shows per-track and master audio levels based on active clips
//! and their gain settings.

use rook_core::project::Project;

pub struct VuMeterPanel {
    /// Smoothed peak hold per track (index matches track array).
    peak_holds: Vec<f32>,
    /// Smoothed master level.
    master_level: f32,
}

impl Default for VuMeterPanel {
    fn default() -> Self {
        Self {
            peak_holds: Vec::new(),
            master_level: 0.0,
        }
    }
}

impl VuMeterPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, project: &Project, playhead: i64) {
        ui.heading("🔊 VU");

        let total_tracks = project.timeline.tracks.len();
        while self.peak_holds.len() <= total_tracks {
            self.peak_holds.push(0.0);
        }

        let mut master_peak: f32 = 0.0;
        let mut track_readings: Vec<(String, f32, f32)> = Vec::new(); // (name, current, held)

        for (ti, track) in project.timeline.tracks.iter().enumerate() {
            if track.kind != rook_core::track::TrackKind::Audio {
                continue;
            }
            if track.muted || !track.visible {
                continue;
            }

            let mut track_peak: f32 = 0.0;
            let mut active = false;

            for clip in &track.clips {
                if clip.timeline_in <= playhead && clip.timeline_in + clip.duration() > playhead {
                    active = true;
                    // Use gain as level baseline (normalized to 0 dB = 0.5)
                    let db = clip.gain_db.unwrap_or(0.0);
                    let linear = 10.0_f32.powf(db / 20.0);
                    // Scale so 0 dB = 0.5 on the meter
                    let level = (linear * 0.5).clamp(0.0, 1.0);
                    track_peak = track_peak.max(level);
                }
            }

            // Only show tracks that are active or recently active
            if active {
                self.peak_holds[ti] = track_peak;
            } else {
                // Decay peak hold slowly
                self.peak_holds[ti] *= 0.95;
                if self.peak_holds[ti] < 0.01 {
                    self.peak_holds[ti] = 0.0;
                }
            }

            let held = self.peak_holds[ti];
            let track_name = if track.name.is_empty() {
                format!("Track {}", ti + 1)
            } else {
                track.name.clone()
            };
            track_readings.push((track_name, track_peak, held));
            master_peak = master_peak.max(track_peak);
        }

        // Smooth master
        self.master_level = self.master_level * 0.85 + master_peak * 0.15;

        if track_readings.is_empty() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("No audio tracks")
                    .size(11.0)
                    .color(egui::Color32::from_gray(140)),
            );
            return;
        }

        let bar_w = ui.available_width() - 8.0;
        let bar_h = 10.0;

        // Per-track meters
        for (name, _current, held) in &track_readings {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).size(10.0));
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(bar_w, bar_h), egui::Sense::hover());
                draw_level_bar(ui, rect, *held);
            });
        }

        // Master
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("MASTER").size(11.0).strong());
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(bar_w, bar_h + 4.0), egui::Sense::hover());
            draw_level_bar(ui, rect, self.master_level);
        });

        // Master dB readout
        let db = if self.master_level > 0.001 {
            20.0 * self.master_level.log10()
        } else {
            -60.0
        };
        let db_color = if db > -0.5 {
            egui::Color32::RED
        } else if db > -6.0 {
            egui::Color32::YELLOW
        } else {
            egui::Color32::GREEN
        };
        ui.label(
            egui::RichText::new(format!("{:.1} dB", db))
                .size(14.0)
                .color(db_color),
        );
    }
}

fn draw_level_bar(ui: &egui::Ui, rect: egui::Rect, level: f32) {
    let level = level.clamp(0.0, 1.0);
    let painter = ui.painter();

    // Background
    painter.rect_filled(rect, 1.0, egui::Color32::from_gray(24));

    // Level fill
    if level > 0.001 {
        let color = if level > 0.95 {
            egui::Color32::RED
        } else if level > 0.75 {
            egui::Color32::YELLOW
        } else {
            egui::Color32::from_rgb(40, 200, 80)
        };
        let fill_w = rect.width() * level;
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
        painter.rect_filled(fill_rect, 1.0, color);
    }

    // dB scale markings (every 10 dB from -60 to 0)
    let marks: [(f32, &str); 4] = [(-60.0, ""), (-40.0, "-40"), (-20.0, "-20"), (0.0, "0")];
    for &(db, label) in &marks {
        let x = rect.min.x + rect.width() * ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(1.0, egui::Color32::from_gray(50)),
        );
        if !label.is_empty() {
            painter.text(
                egui::pos2(x + 2.0, rect.max.y - 2.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::monospace(8.0),
                egui::Color32::from_gray(100),
            );
        }
    }
}
