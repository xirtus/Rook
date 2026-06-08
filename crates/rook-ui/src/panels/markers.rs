//! Marker List panel — shows all timeline markers in a sortable list.

use rook_core::project::Project;

/// Snapshot of a marker for rendering (avoids borrow conflicts).
struct MarkerSnapshot {
    idx: usize,
    frame: i64,
    label: String,
    color: Option<[u8; 4]>,
    notes: String,
}

pub struct MarkerListPanel {
    add_marker_text: String,
    editing_list_pos: Option<usize>,
    edit_buffer: String,
}

impl Default for MarkerListPanel {
    fn default() -> Self {
        Self {
            add_marker_text: String::new(),
            editing_list_pos: None,
            edit_buffer: String::new(),
        }
    }
}

impl MarkerListPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, project: &mut Project, playhead: &mut i64) {
        ui.heading("📍 Markers");

        let fps = project.timeline.frame_rate.as_f64();

        // ── Add marker bar ──────────────────────────────────────────
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.add_marker_text)
                    .hint_text("Marker name…")
                    .desired_width(ui.available_width() - 50.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("➕ Add").clicked() || enter) && !self.add_marker_text.trim().is_empty()
            {
                project
                    .timeline
                    .markers
                    .push(rook_core::marker::Marker::new(
                        self.add_marker_text.trim().to_string(),
                        *playhead,
                    ));
                self.add_marker_text.clear();
            }
        });

        ui.separator();

        if project.timeline.markers.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("No markers yet")
                        .size(12.0)
                        .color(egui::Color32::from_gray(140)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Press M at playhead, or type a name above")
                        .size(11.0)
                        .color(egui::Color32::from_gray(120)),
                );
            });
            return;
        }

        // ── Snapshot marker data (avoids borrow conflicts) ─────────
        let snapshots: Vec<MarkerSnapshot> = project
            .timeline
            .markers
            .iter()
            .enumerate()
            .map(|(i, m)| MarkerSnapshot {
                idx: i,
                frame: m.frame,
                label: if m.label.is_empty() {
                    format!("Marker {}", i + 1)
                } else {
                    m.label.clone()
                },
                color: m.color,
                notes: m.notes.clone(),
            })
            .collect();

        let mut sorted: Vec<usize> = (0..snapshots.len()).collect();
        sorted.sort_by_key(|&i| snapshots[i].frame);

        ui.label(
            egui::RichText::new(format!("{} markers", sorted.len()))
                .size(11.0)
                .color(egui::Color32::from_gray(140)),
        );
        ui.add_space(4.0);

        // ── Pending actions (applied after rendering) ─────────────
        let mut seek_to: Option<i64> = None;
        let mut remove_idx: Option<usize> = None;
        let mut set_color: Option<(usize, [u8; 4])> = None;
        let mut rename: Option<(usize, String)> = None;
        let mut start_edit: Option<(usize, String)> = None;
        let mut clear_all = false;

        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 8.0)
            .show(ui, |ui| {
                for list_pos in 0..sorted.len() {
                    let snap_idx = sorted[list_pos];
                    let snap = &snapshots[snap_idx];
                    let frame = snap.frame;
                    let secs = frame as f64 / fps;
                    let tc = format!(
                        "{:02}:{:02}:{:02}.{:02}",
                        (secs / 3600.0) as i64,
                        ((secs % 3600.0) / 60.0) as i64,
                        (secs % 60.0) as i64,
                        (secs.fract() * fps).round() as i64 % fps as i64,
                    );
                    let is_active = *playhead == frame;

                    ui.horizontal(|ui| {
                        // Highlight active marker
                        if is_active {
                            ui.painter().rect_filled(
                                ui.max_rect(),
                                3.0,
                                egui::Color32::from_rgba_premultiplied(60, 60, 80, 60),
                            );
                        }

                        // Color dot
                        let (r, g, b) = match snap.color {
                            Some([cr, cg, cb, _]) => (cr, cg, cb),
                            None => (0, 200, 140),
                        };
                        ui.colored_label(egui::Color32::from_rgb(r, g, b), "●");

                        // Seek button
                        if ui.selectable_label(is_active, &tc).clicked() {
                            seek_to = Some(frame);
                        }

                        // Editable label
                        if self.editing_list_pos == Some(list_pos) {
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.edit_buffer)
                                        .desired_width(ui.available_width() - 20.0),
                                )
                                .lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                if !self.edit_buffer.trim().is_empty() {
                                    rename = Some((snap.idx, self.edit_buffer.trim().to_string()));
                                }
                                self.editing_list_pos = None;
                                self.edit_buffer.clear();
                            }
                        } else {
                            if ui
                                .selectable_label(
                                    false,
                                    egui::RichText::new(&snap.label).size(11.0),
                                )
                                .double_clicked()
                            {
                                start_edit = Some((list_pos, snap.label.clone()));
                            }
                            if !snap.notes.is_empty() {
                                ui.label("📝").on_hover_text(&snap.notes);
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let color_opts: [(u8, u8, u8, &str); 5] = [
                                (0, 200, 140, "green"),
                                (255, 180, 40, "orange"),
                                (255, 80, 80, "red"),
                                (80, 160, 255, "blue"),
                                (200, 120, 255, "purple"),
                            ];
                            for &(cr, cg, cb, hint) in &color_opts {
                                if ui
                                    .add_sized(
                                        [12.0, 12.0],
                                        egui::Button::new("")
                                            .fill(egui::Color32::from_rgb(cr, cg, cb)),
                                    )
                                    .on_hover_text(hint)
                                    .clicked()
                                {
                                    set_color = Some((snap.idx, [cr, cg, cb, 255]));
                                }
                            }
                            if ui.button("🗑").clicked() {
                                remove_idx = Some(snap.idx);
                            }
                        });
                    });
                }
            });

        // ── Apply pending actions ──────────────────────────────────
        if let Some(frame) = seek_to {
            *playhead = frame;
            project.timeline.playhead = frame;
        }
        if let Some(idx) = remove_idx {
            project.timeline.markers.remove(idx);
        }
        if let Some((idx, color)) = set_color {
            if let Some(m) = project.timeline.markers.get_mut(idx) {
                m.color = Some(color);
            }
        }
        if let Some((idx, label)) = rename {
            if let Some(m) = project.timeline.markers.get_mut(idx) {
                m.label = label;
            }
        }
        if let Some((list_pos, label)) = start_edit {
            self.editing_list_pos = Some(list_pos);
            self.edit_buffer = label;
        }

        // ── Clear all ─────────────────────────────────────────────
        ui.add_space(4.0);
        if ui.button("🗑 Clear All Markers").clicked() {
            project.timeline.markers.clear();
        }
    }
}
