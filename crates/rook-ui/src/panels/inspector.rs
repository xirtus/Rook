//! Inspector panel — selected clip properties, fully editable.
//!
//! Keyframe editor: diamond buttons (💎) next to animatable properties
//! toggle keyframes at the playhead. A keyframe list shows all keyframes
//! on the selected clip with value, frame, and easing editing.

use rook_core::effect::{EffectInstance, EffectKind};
use rook_core::keyframe::{Keyframe, KeyframeProperty};
use rook_core::project::Project;

#[derive(Default)]
pub struct InspectorPanel {
    add_effect_selection: Option<EffectKind>,
}

impl InspectorPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, project: &mut Project, playhead: &i64) {
        ui.heading("Inspector");

        let selected = &project.timeline.selected_clip_ids;

        if selected.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label("No clip selected");
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Click a clip on the timeline to inspect it")
                        .size(11.0)
                        .color(egui::Color32::from_gray(140)),
                );
            });
            return;
        }

        // Snapshot read-only data before mutable section
        let clip_id = selected[0];
        let clip_local_frame = *playhead; // will be converted to local frame where needed
        let (
            label,
            asset_name,
            dur,
            fps,
            src_out,
            has_fade,
            fade_frames,
            has_mask,
            has_kf,
            kf_list,
            kf_at_playhead,
            ai_desc,
            ai_labels,
            generator_data,
        ) = {
            if let Some(clip) = project.timeline.clip(clip_id) {
                let asset_name = project
                    .asset(clip.asset_id)
                    .map(|a| a.filename_stem().to_string());
                let dur = clip.duration();
                let fps = project.frame_rate.as_f64();
                let src_out = clip.source_in + clip.source_duration;
                let has_fade = clip.fade.is_some();
                let fade_frames = clip.fade.as_ref().map(|f| (f.in_frames, f.out_frames));
                let has_mask = clip.mask.is_some();
                let has_kf = !clip.keyframes.is_empty();
                let kf_list: Vec<_> = clip
                    .keyframes
                    .iter()
                    .map(|k| {
                        (
                            k.at_frame,
                            format!("{:?}", k.property),
                            k.id,
                            k.value,
                            format!("{:?}", k.easing),
                        )
                    })
                    .collect();
                let local_frame = *playhead - clip.timeline_in;
                let kf_at_playhead: std::collections::HashSet<String> = clip
                    .keyframes
                    .iter()
                    .filter(|k| k.at_frame == local_frame)
                    .map(|k| format!("{:?}", k.property))
                    .collect();
                let generator_data = clip.generator.clone();
                let ai_desc = project
                    .asset(clip.asset_id)
                    .and_then(|a| a.metadata().ai_description.clone());
                let ai_labels = project
                    .asset(clip.asset_id)
                    .map(|a| a.metadata().ai_labels.clone())
                    .unwrap_or_default();
                (
                    clip.label.clone(),
                    asset_name,
                    dur,
                    fps,
                    src_out,
                    has_fade,
                    fade_frames,
                    has_mask,
                    has_kf,
                    kf_list,
                    kf_at_playhead,
                    ai_desc,
                    ai_labels,
                    generator_data,
                )
            } else {
                return;
            }
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            // ── Clip identity ───────────────────────────────────────
            ui.horizontal(|ui| {
                let mut clip_label = label.clone();
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut clip_label)
                        .desired_width(ui.available_width() - 24.0)
                        .font(egui::TextStyle::Heading.resolve(ui.style())),
                );
                if resp.changed() {
                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                        clip.label = clip_label;
                    }
                }
            });
            if let Some(ref name) = asset_name {
                ui.label(
                    egui::RichText::new(name)
                        .size(11.0)
                        .color(egui::Color32::from_gray(160)),
                );
            }
            ui.separator();

            // ── Transition (if set) ──────────────────────────────────
            let has_transition = project
                .timeline
                .clip(clip_id)
                .map(|c| c.transition.is_some())
                .unwrap_or(false);
            if has_transition {
                ui.collapsing("↔ Transition", |ui| {
                    let (mut t_kind, mut t_dur, mut t_rev) = {
                        let clip = project.timeline.clip(clip_id).unwrap();
                        if let Some(ref t) = clip.transition {
                            let kind_idx = match t.kind {
                                rook_core::clip::TransitionKind::CrossDissolve => 0,
                                rook_core::clip::TransitionKind::Dissolve => 1,
                                rook_core::clip::TransitionKind::Wipe => 2,
                                rook_core::clip::TransitionKind::Slide => 3,
                            };
                            (kind_idx, t.duration_frames, t.reversed)
                        } else {
                            return;
                        }
                    };
                    let mut kind_idx = t_kind;
                    egui::ComboBox::from_id_salt("transition_kind")
                        .selected_text(match kind_idx {
                            0 => "Cross Dissolve",
                            1 => "Dissolve",
                            2 => "Wipe",
                            3 => "Slide",
                            _ => "Cross Dissolve",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut kind_idx, 0, "Cross Dissolve");
                            ui.selectable_value(&mut kind_idx, 1, "Dissolve");
                            ui.selectable_value(&mut kind_idx, 2, "Wipe");
                            ui.selectable_value(&mut kind_idx, 3, "Slide");
                        });
                    let mut dur = t_dur;
                    ui.horizontal(|ui| {
                        ui.label("Frames:");
                        if ui
                            .add(
                                egui::DragValue::new(&mut dur)
                                    .speed(1.0)
                                    .clamp_range(1..=300),
                            )
                            .changed()
                        {
                            if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                if let Some(ref mut t) = clip.transition {
                                    t.duration_frames = dur;
                                }
                            }
                        }
                    });
                    let mut rev = t_rev;
                    let rev_label = if rev { "↩ Reversed" } else { "↩ Reverse" };
                    if ui.toggle_value(&mut rev, rev_label).changed() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            if let Some(ref mut t) = clip.transition {
                                t.reversed = rev;
                            }
                        }
                    }
                    if kind_idx != t_kind {
                        let new_kind = match kind_idx {
                            1 => rook_core::clip::TransitionKind::Dissolve,
                            2 => rook_core::clip::TransitionKind::Wipe,
                            3 => rook_core::clip::TransitionKind::Slide,
                            _ => rook_core::clip::TransitionKind::CrossDissolve,
                        };
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            if let Some(ref mut t) = clip.transition {
                                t.kind = new_kind;
                            }
                        }
                    }
                    if ui.button("🗑 Remove Transition").clicked() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            clip.transition = None;
                        }
                    }
                });
            } else {
                // Show "Add Transition" button if clip has a predecessor
                ui.horizontal(|ui| {
                    if ui.button("➕ Transition").clicked() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            clip.transition = Some(rook_core::clip::Transition {
                                kind: rook_core::clip::TransitionKind::CrossDissolve,
                                duration_frames: 24,
                                reversed: false,
                                curve: rook_core::clip::FadeCurve::Linear,
                            });
                        }
                    }
                    ui.label(
                        egui::RichText::new("cross-dissolve from prev clip")
                            .size(10.0)
                            .color(egui::Color32::from_gray(120)),
                    );
                });
            }
            ui.separator();

            // ── Generator (if present) ─────────────────────────────
            if let Some(ref generator) = generator_data {
                ui.collapsing("🎬 Generator", |ui| {
                    match generator {
                        rook_core::clip::Generator::Text {
                            content,
                            font_size,
                            color,
                        } => {
                            let mut text = content.clone();
                            let mut size = *font_size;
                            let mut r = color[0];
                            let mut g = color[1];
                            let mut b = color[2];
                            let mut a = color[3];

                            ui.label("Text:");
                            if ui.text_edit_multiline(&mut text).changed() {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(rook_core::clip::Generator::Text {
                                        content: ref mut c,
                                        ..
                                    }) = clip.generator
                                    {
                                        *c = text;
                                    }
                                }
                            }
                            ui.horizontal(|ui| {
                                ui.label("Font size:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut size)
                                            .speed(1.0)
                                            .clamp_range(8.0..=200.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(rook_core::clip::Generator::Text {
                                            font_size: ref mut fs,
                                            ..
                                        }) = clip.generator
                                        {
                                            *fs = size;
                                        }
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("R:");
                                ui.add(
                                    egui::DragValue::new(&mut r)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                                ui.label("G:");
                                ui.add(
                                    egui::DragValue::new(&mut g)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                                ui.label("B:");
                                ui.add(
                                    egui::DragValue::new(&mut b)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                            });
                            let color_changed = r != color[0] || g != color[1] || b != color[2];
                            if color_changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(rook_core::clip::Generator::Text {
                                        color: ref mut col,
                                        ..
                                    }) = clip.generator
                                    {
                                        *col = [r, g, b, a];
                                    }
                                }
                            }
                            // Preview swatch
                            let preview = egui::Color32::from_rgba_premultiplied(
                                (r * 255.0) as u8,
                                (g * 255.0) as u8,
                                (b * 255.0) as u8,
                                255,
                            );
                            ui.add(
                                egui::Button::new("Text Color")
                                    .fill(preview)
                                    .min_size(egui::vec2(100.0, 20.0)),
                            );
                        }
                        rook_core::clip::Generator::Solid { color } => {
                            let mut r = color[0];
                            let mut g = color[1];
                            let mut b = color[2];
                            let mut a = color[3];
                            ui.horizontal(|ui| {
                                ui.label("R:");
                                ui.add(
                                    egui::DragValue::new(&mut r)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                                ui.label("G:");
                                ui.add(
                                    egui::DragValue::new(&mut g)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                                ui.label("B:");
                                ui.add(
                                    egui::DragValue::new(&mut b)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                            });
                            let changed = r != color[0] || g != color[1] || b != color[2];
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(rook_core::clip::Generator::Solid {
                                        color: ref mut col,
                                    }) = clip.generator
                                    {
                                        *col = [r, g, b, a];
                                    }
                                }
                            }
                            let preview = egui::Color32::from_rgba_premultiplied(
                                (r * 255.0) as u8,
                                (g * 255.0) as u8,
                                (b * 255.0) as u8,
                                255,
                            );
                            ui.add(
                                egui::Button::new("Color")
                                    .fill(preview)
                                    .min_size(egui::vec2(100.0, 20.0)),
                            );
                        }
                        rook_core::clip::Generator::Credits {
                            content,
                            font_size,
                            color,
                            scroll_speed,
                        } => {
                            let mut text = content.clone();
                            let mut size = *font_size;
                            let mut r = color[0];
                            let mut g = color[1];
                            let mut b = color[2];
                            let mut a = color[3];
                            let mut speed = *scroll_speed;

                            ui.label("Credits Text:");
                            if ui.text_edit_multiline(&mut text).changed() {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(rook_core::clip::Generator::Credits {
                                        content: ref mut c,
                                        ..
                                    }) = clip.generator
                                    {
                                        *c = text;
                                    }
                                }
                            }
                            ui.horizontal(|ui| {
                                ui.label("Font size:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut size)
                                            .speed(1.0)
                                            .clamp_range(8.0..=120.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(rook_core::clip::Generator::Credits {
                                            font_size: ref mut fs,
                                            ..
                                        }) = clip.generator
                                        {
                                            *fs = size;
                                        }
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Scroll speed:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut speed)
                                            .speed(1.0)
                                            .suffix(" px/s")
                                            .clamp_range(10.0..=300.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(rook_core::clip::Generator::Credits {
                                            scroll_speed: ref mut ss,
                                            ..
                                        }) = clip.generator
                                        {
                                            *ss = speed;
                                        }
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("R:");
                                ui.add(
                                    egui::DragValue::new(&mut r)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                                ui.label("G:");
                                ui.add(
                                    egui::DragValue::new(&mut g)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                                ui.label("B:");
                                ui.add(
                                    egui::DragValue::new(&mut b)
                                        .speed(0.01)
                                        .clamp_range(0.0..=1.0),
                                );
                            });
                            let color_changed = r != color[0] || g != color[1] || b != color[2];
                            if color_changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(rook_core::clip::Generator::Credits {
                                        color: ref mut col,
                                        ..
                                    }) = clip.generator
                                    {
                                        *col = [r, g, b, a];
                                    }
                                }
                            }
                            let preview = egui::Color32::from_rgba_premultiplied(
                                (r * 255.0) as u8,
                                (g * 255.0) as u8,
                                (b * 255.0) as u8,
                                255,
                            );
                            ui.add(
                                egui::Button::new("Credits Color")
                                    .fill(preview)
                                    .min_size(egui::vec2(100.0, 20.0)),
                            );
                        }
                        _ => {
                            ui.label("(custom generator)");
                        }
                    }
                    if ui.button("🗑 Remove Generator").clicked() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            clip.generator = None;
                        }
                    }
                });
            }

            // ── Timing ──────────────────────────────────────────────
            ui.collapsing("⏱ Timing", |ui| {
                let clip = project.timeline.clip(clip_id).unwrap();
                ui.label(format!(
                    "Duration: {:.1}s ({} frames)",
                    dur as f64 / fps,
                    dur
                ));
                ui.label(format!(
                    "Timeline In: {}f ({:.2}s)",
                    clip.timeline_in,
                    clip.timeline_in as f64 / fps
                ));
                ui.label(format!("Source In: {}f", clip.source_in));
                ui.label(format!("Source Out: {}f", src_out));
            });

            // ── Transform (editable) ──────────────────────────────
            ui.collapsing("📐 Transform", |ui| {
                // Snapshot transform values
                let (px, py, sx, sy, rot, ax, ay, fh, fv) = {
                    let clip = project.timeline.clip(clip_id).unwrap();
                    let t = &clip.transform;
                    (
                        t.position.x,
                        t.position.y,
                        t.scale.x,
                        t.scale.y,
                        t.rotation_deg,
                        t.anchor.x,
                        t.anchor.y,
                        t.flip_h,
                        t.flip_v,
                    )
                };
                let mut pos_x = px;
                let mut pos_y = py;
                let mut scale_x = sx;
                let mut scale_y = sy;
                let mut rotation = rot;
                let mut anchor_x = ax;
                let mut anchor_y = ay;

                // Helper: diamond button for keyframe toggle
                let local_frame = *playhead
                    - project
                        .timeline
                        .clip(clip_id)
                        .map(|c| c.timeline_in)
                        .unwrap_or(0);
                let kf_diamond = |ui: &mut egui::Ui, prop: KeyframeProperty, val: f64| -> bool {
                    let prop_str = format!("{:?}", prop);
                    let has = kf_at_playhead.contains(&prop_str);
                    let txt = if has { "◆" } else { "◇" };
                    let btn = ui.add_sized(
                        [16.0, 16.0],
                        egui::Button::new(txt).fill(egui::Color32::TRANSPARENT),
                    );
                    if btn.clicked() {
                        return true;
                    }
                    false
                };
                let mut kf_toggle: Option<(KeyframeProperty, f64)> = None;

                let mut changed = false;
                egui::Grid::new("transform_edit")
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.label("Position X");
                        if ui
                            .add(egui::DragValue::new(&mut pos_x).speed(1.0))
                            .changed()
                        {
                            changed = true;
                        }
                        if kf_diamond(ui, KeyframeProperty::PositionX, pos_x as f64) {
                            kf_toggle = Some((KeyframeProperty::PositionX, pos_x as f64));
                        }
                        ui.end_row();
                        ui.label("Position Y");
                        if ui
                            .add(egui::DragValue::new(&mut pos_y).speed(1.0))
                            .changed()
                        {
                            changed = true;
                        }
                        if kf_diamond(ui, KeyframeProperty::PositionY, pos_y as f64) {
                            kf_toggle = Some((KeyframeProperty::PositionY, pos_y as f64));
                        }
                        ui.end_row();
                        ui.label("Scale X");
                        if ui
                            .add(
                                egui::DragValue::new(&mut scale_x)
                                    .speed(0.01)
                                    .clamp_range(0.01..=10.0),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if kf_diamond(ui, KeyframeProperty::ScaleX, scale_x as f64) {
                            kf_toggle = Some((KeyframeProperty::ScaleX, scale_x as f64));
                        }
                        ui.end_row();
                        ui.label("Scale Y");
                        if ui
                            .add(
                                egui::DragValue::new(&mut scale_y)
                                    .speed(0.01)
                                    .clamp_range(0.01..=10.0),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if kf_diamond(ui, KeyframeProperty::ScaleY, scale_y as f64) {
                            kf_toggle = Some((KeyframeProperty::ScaleY, scale_y as f64));
                        }
                        ui.end_row();
                        ui.label("Rotation °");
                        if ui
                            .add(egui::DragValue::new(&mut rotation).speed(1.0))
                            .changed()
                        {
                            changed = true;
                        }
                        if kf_diamond(ui, KeyframeProperty::Rotation, rotation as f64) {
                            kf_toggle = Some((KeyframeProperty::Rotation, rotation as f64));
                        }
                        ui.end_row();
                        ui.label("Anchor X");
                        if ui
                            .add(
                                egui::DragValue::new(&mut anchor_x)
                                    .speed(0.01)
                                    .clamp_range(0.0..=1.0),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        ui.label(""); // no keyframe for anchor
                        ui.end_row();
                        ui.label("Anchor Y");
                        if ui
                            .add(
                                egui::DragValue::new(&mut anchor_y)
                                    .speed(0.01)
                                    .clamp_range(0.0..=1.0),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        ui.label(""); // no keyframe for anchor
                        ui.end_row();
                    });

                if changed {
                    let clip = project.timeline.clip_mut(clip_id).unwrap();
                    clip.transform.position.x = pos_x;
                    clip.transform.position.y = pos_y;
                    clip.transform.scale.x = scale_x;
                    clip.transform.scale.y = scale_y;
                    clip.transform.rotation_deg = rotation % 360.0;
                    clip.transform.anchor.x = anchor_x;
                    clip.transform.anchor.y = anchor_y;
                }

                // Handle keyframe toggle from diamonds
                if let Some((prop, val)) = kf_toggle {
                    let local_frame = *playhead
                        - project
                            .timeline
                            .clip(clip_id)
                            .map(|c| c.timeline_in)
                            .unwrap_or(0);
                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                        Self::toggle_keyframe(clip, local_frame, prop, val);
                    }
                }

                ui.separator();

                let mut flip_h = fh;
                let mut flip_v = fv;
                let fh_label = if flip_h {
                    "↔ Flip H ✓"
                } else {
                    "↔ Flip H"
                };
                let fv_label = if flip_v {
                    "↕ Flip V ✓"
                } else {
                    "↕ Flip V"
                };
                let fh_changed = ui
                    .horizontal(|ui| ui.toggle_value(&mut flip_h, fh_label))
                    .inner;
                let fv_changed = ui
                    .horizontal(|ui| ui.toggle_value(&mut flip_v, fv_label))
                    .inner;
                if fh_changed.changed() || fv_changed.changed() {
                    let clip = project.timeline.clip_mut(clip_id).unwrap();
                    if fh_changed.changed() {
                        clip.transform.flip_h = flip_h;
                    }
                    if fv_changed.changed() {
                        clip.transform.flip_v = flip_v;
                    }
                }
            });

            // ── Crop (editable) ──────────────────────────────────
            ui.collapsing("✂ Crop", |ui| {
                let (ct, cr, cb, cl) = {
                    let clip = project.timeline.clip(clip_id).unwrap();
                    let c = &clip.transform.crop;
                    (c.top, c.right, c.bottom, c.left)
                };
                let mut top = ct;
                let mut right = cr;
                let mut bottom = cb;
                let mut left = cl;
                let mut crop_changed = false;
                egui::Grid::new("crop_edit").num_columns(2).show(ui, |ui| {
                    ui.label("Top");
                    if ui
                        .add(
                            egui::DragValue::new(&mut top)
                                .speed(1.0)
                                .clamp_range(0.0..=10000.0),
                        )
                        .changed()
                    {
                        crop_changed = true;
                    }
                    ui.end_row();
                    ui.label("Right");
                    if ui
                        .add(
                            egui::DragValue::new(&mut right)
                                .speed(1.0)
                                .clamp_range(0.0..=10000.0),
                        )
                        .changed()
                    {
                        crop_changed = true;
                    }
                    ui.end_row();
                    ui.label("Bottom");
                    if ui
                        .add(
                            egui::DragValue::new(&mut bottom)
                                .speed(1.0)
                                .clamp_range(0.0..=10000.0),
                        )
                        .changed()
                    {
                        crop_changed = true;
                    }
                    ui.end_row();
                    ui.label("Left");
                    if ui
                        .add(
                            egui::DragValue::new(&mut left)
                                .speed(1.0)
                                .clamp_range(0.0..=10000.0),
                        )
                        .changed()
                    {
                        crop_changed = true;
                    }
                    ui.end_row();
                });
                if crop_changed {
                    let clip = project.timeline.clip_mut(clip_id).unwrap();
                    clip.transform.crop.top = top;
                    clip.transform.crop.right = right;
                    clip.transform.crop.bottom = bottom;
                    clip.transform.crop.left = left;
                }
            });

            // ── Compositing (editable) ───────────────────────────
            ui.collapsing("🎨 Compositing", |ui| {
                let op = {
                    let clip = project.timeline.clip(clip_id).unwrap();
                    clip.transform.opacity
                };
                let mut opacity_pct = (op * 100.0).round();
                let mut op_kf_toggle = false;
                let op_resp = ui
                    .horizontal(|ui| {
                        ui.label("Opacity %");
                        let resp = ui.add(
                            egui::DragValue::new(&mut opacity_pct)
                                .speed(1.0)
                                .clamp_range(0.0..=100.0),
                        );
                        let prop_str = format!("{:?}", KeyframeProperty::Opacity);
                        let has = kf_at_playhead.contains(&prop_str);
                        let txt = if has { "◆" } else { "◇" };
                        if ui
                            .add_sized(
                                [16.0, 16.0],
                                egui::Button::new(txt).fill(egui::Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            op_kf_toggle = true;
                        }
                        resp
                    })
                    .inner;
                if op_resp.changed() {
                    let clip = project.timeline.clip_mut(clip_id).unwrap();
                    clip.transform.opacity = (opacity_pct / 100.0).clamp(0.0, 1.0);
                }
                if op_kf_toggle {
                    let local_frame = *playhead
                        - project
                            .timeline
                            .clip(clip_id)
                            .map(|c| c.timeline_in)
                            .unwrap_or(0);
                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                        Self::toggle_keyframe(
                            clip,
                            local_frame,
                            KeyframeProperty::Opacity,
                            (opacity_pct / 100.0) as f64,
                        );
                    }
                }
                let orig_blend = {
                    let clip = project.timeline.clip(clip_id).unwrap();
                    clip.blend_mode
                };
                let modes: &[&str] = &[
                    "Normal",
                    "Darken",
                    "Multiply",
                    "ColorBurn",
                    "Lighten",
                    "Screen",
                    "PlusLighter",
                    "ColorDodge",
                    "Overlay",
                    "SoftLight",
                    "HardLight",
                    "Difference",
                    "Exclusion",
                    "Hue",
                    "Saturation",
                    "Color",
                    "Luminosity",
                ];
                let blend_to_idx = |b: rook_core::clip::BlendMode| match b {
                    rook_core::clip::BlendMode::Normal => 0,
                    rook_core::clip::BlendMode::Darken => 1,
                    rook_core::clip::BlendMode::Multiply => 2,
                    rook_core::clip::BlendMode::ColorBurn => 3,
                    rook_core::clip::BlendMode::Lighten => 4,
                    rook_core::clip::BlendMode::Screen => 5,
                    rook_core::clip::BlendMode::PlusLighter => 6,
                    rook_core::clip::BlendMode::ColorDodge => 7,
                    rook_core::clip::BlendMode::Overlay => 8,
                    rook_core::clip::BlendMode::SoftLight => 9,
                    rook_core::clip::BlendMode::HardLight => 10,
                    rook_core::clip::BlendMode::Difference => 11,
                    rook_core::clip::BlendMode::Exclusion => 12,
                    rook_core::clip::BlendMode::Hue => 13,
                    rook_core::clip::BlendMode::Saturation => 14,
                    rook_core::clip::BlendMode::Color => 15,
                    rook_core::clip::BlendMode::Luminosity => 16,
                };
                let mut blend_idx = blend_to_idx(orig_blend);
                let _ = ui.horizontal(|ui| {
                    ui.label("Blend");
                    egui::ComboBox::from_id_salt("blend_mode")
                        .selected_text(modes[blend_idx])
                        .show_ui(ui, |ui| {
                            for (i, name) in modes.iter().enumerate() {
                                ui.selectable_value(&mut blend_idx, i, *name);
                            }
                        });
                });
                if blend_idx != blend_to_idx(orig_blend) {
                    let clip = project.timeline.clip_mut(clip_id).unwrap();
                    clip.blend_mode = match blend_idx {
                        1 => rook_core::clip::BlendMode::Darken,
                        2 => rook_core::clip::BlendMode::Multiply,
                        3 => rook_core::clip::BlendMode::ColorBurn,
                        4 => rook_core::clip::BlendMode::Lighten,
                        5 => rook_core::clip::BlendMode::Screen,
                        6 => rook_core::clip::BlendMode::PlusLighter,
                        7 => rook_core::clip::BlendMode::ColorDodge,
                        8 => rook_core::clip::BlendMode::Overlay,
                        9 => rook_core::clip::BlendMode::SoftLight,
                        10 => rook_core::clip::BlendMode::HardLight,
                        11 => rook_core::clip::BlendMode::Difference,
                        12 => rook_core::clip::BlendMode::Exclusion,
                        13 => rook_core::clip::BlendMode::Hue,
                        14 => rook_core::clip::BlendMode::Saturation,
                        15 => rook_core::clip::BlendMode::Color,
                        16 => rook_core::clip::BlendMode::Luminosity,
                        _ => rook_core::clip::BlendMode::Normal,
                    };
                }
            });

            // ── Speed (editable) ─────────────────────────────────
            ui.collapsing("⏩ Retiming", |ui| {
                let (sp, has_curve, curve_len) = {
                    let clip = project.timeline.clip(clip_id).unwrap();
                    (
                        clip.speed,
                        !clip.speed_curve.is_empty(),
                        clip.speed_curve.len(),
                    )
                };
                let mut speed = sp;
                let speed_changed = ui
                    .horizontal(|ui| {
                        ui.label("Speed");
                        ui.add(
                            egui::DragValue::new(&mut speed)
                                .speed(0.1)
                                .clamp_range(0.1..=10.0),
                        )
                    })
                    .inner;
                if speed_changed.changed() {
                    let clip = project.timeline.clip_mut(clip_id).unwrap();
                    clip.speed = speed;
                }
                let eff_dur = {
                    let clip = project.timeline.clip(clip_id).unwrap();
                    clip.duration()
                };
                ui.label(format!("Effective duration: {}f", eff_dur));
                if has_curve {
                    ui.label(format!("Speed ramp: {} points", curve_len));
                }
            });

            // ── Audio (editable) ─────────────────────────────────
            ui.collapsing("🔊 Audio", |ui| {
                let (g, mt) = {
                    let clip = project.timeline.clip(clip_id).unwrap();
                    (clip.gain_db.unwrap_or(0.0), clip.mute_audio)
                };
                let mut gain = g;
                let mut gain_kf_toggle = false;
                let gain_resp = ui
                    .horizontal(|ui| {
                        ui.label("Gain dB");
                        let resp = ui.add(
                            egui::DragValue::new(&mut gain)
                                .speed(0.1)
                                .clamp_range(-96.0..=24.0),
                        );
                        let prop_str = format!("{:?}", KeyframeProperty::Volume);
                        let has = kf_at_playhead.contains(&prop_str);
                        let txt = if has { "◆" } else { "◇" };
                        if ui
                            .add_sized(
                                [16.0, 16.0],
                                egui::Button::new(txt).fill(egui::Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            gain_kf_toggle = true;
                        }
                        resp
                    })
                    .inner;
                if gain_resp.changed() {
                    let clip = project.timeline.clip_mut(clip_id).unwrap();
                    clip.gain_db = Some(gain);
                }
                if gain_kf_toggle {
                    let local_frame = *playhead
                        - project
                            .timeline
                            .clip(clip_id)
                            .map(|c| c.timeline_in)
                            .unwrap_or(0);
                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                        Self::toggle_keyframe(
                            clip,
                            local_frame,
                            KeyframeProperty::Volume,
                            gain as f64,
                        );
                    }
                }
                let mut muted = mt;
                let mute_label = if muted { "🔇 Muted" } else { "🔊 Muted" };
                if ui.toggle_value(&mut muted, mute_label).changed() {
                    let clip = project.timeline.clip_mut(clip_id).unwrap();
                    clip.mute_audio = muted;
                }
            });

            // ── Fade ────────────────────────────────────────────────
            ui.collapsing("🌅 Fade", |ui| {
                let has_fade_now = project
                    .timeline
                    .clip(clip_id)
                    .map(|c| c.fade.is_some())
                    .unwrap_or(false);
                if !has_fade_now {
                    if ui.button("➕ Add Fade").clicked() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            clip.fade = Some(rook_core::clip::Fade {
                                in_frames: 24,
                                out_frames: 24,
                                curve: rook_core::clip::FadeCurve::Linear,
                            });
                        }
                    }
                } else {
                    let (mut in_f, mut out_f, curve_val) = {
                        let clip = project.timeline.clip(clip_id).unwrap();
                        let f = clip.fade.as_ref().unwrap();
                        (f.in_frames, f.out_frames, f.curve)
                    };
                    ui.horizontal(|ui| {
                        ui.label("In (frames):");
                        if ui
                            .add(
                                egui::DragValue::new(&mut in_f)
                                    .speed(1.0)
                                    .clamp_range(0..=10000),
                            )
                            .changed()
                        {
                            if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                if let Some(ref mut f) = clip.fade {
                                    f.in_frames = in_f;
                                }
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Out (frames):");
                        if ui
                            .add(
                                egui::DragValue::new(&mut out_f)
                                    .speed(1.0)
                                    .clamp_range(0..=10000),
                            )
                            .changed()
                        {
                            if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                if let Some(ref mut f) = clip.fade {
                                    f.out_frames = out_f;
                                }
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Curve:");
                        let mut curve_idx = match curve_val {
                            rook_core::clip::FadeCurve::Linear => 0,
                            rook_core::clip::FadeCurve::Ease => 1,
                            rook_core::clip::FadeCurve::EaseIn => 2,
                            rook_core::clip::FadeCurve::EaseOut => 3,
                        };
                        let curves = &["Linear", "Ease", "Ease In", "Ease Out"];
                        egui::ComboBox::from_id_salt("fade_curve")
                            .selected_text(curves[curve_idx])
                            .show_ui(ui, |ui| {
                                for (i, name) in curves.iter().enumerate() {
                                    ui.selectable_value(&mut curve_idx, i, *name);
                                }
                            });
                        if curve_idx
                            != match curve_val {
                                rook_core::clip::FadeCurve::Linear => 0,
                                rook_core::clip::FadeCurve::Ease => 1,
                                rook_core::clip::FadeCurve::EaseIn => 2,
                                rook_core::clip::FadeCurve::EaseOut => 3,
                            }
                        {
                            let new_curve = match curve_idx {
                                1 => rook_core::clip::FadeCurve::Ease,
                                2 => rook_core::clip::FadeCurve::EaseIn,
                                3 => rook_core::clip::FadeCurve::EaseOut,
                                _ => rook_core::clip::FadeCurve::Linear,
                            };
                            if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                if let Some(ref mut f) = clip.fade {
                                    f.curve = new_curve;
                                }
                            }
                        }
                    });
                    if ui.button("🗑 Remove Fade").clicked() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            clip.fade = None;
                        }
                    }
                }
            });

            // ── Mask (editable) ────────────────────────────────────
            ui.collapsing("🎭 Mask", |ui| {
                let has_mask_now = project
                    .timeline
                    .clip(clip_id)
                    .map(|c| c.mask.is_some())
                    .unwrap_or(false);
                if !has_mask_now {
                    if ui.button("➕ Add Mask").clicked() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            clip.mask = Some(rook_core::clip::ClipMask {
                                kind: rook_core::clip::MaskKind::Rectangle,
                                x: 0.0,
                                y: 0.0,
                                width: 200.0,
                                height: 200.0,
                                feather: 0.0,
                                invert: false,
                            });
                        }
                    }
                } else {
                    let (mut mx, mut my, mut mw, mut mh, mut mf, mut mi, mk) = {
                        let clip = project.timeline.clip(clip_id).unwrap();
                        if let Some(ref mask) = clip.mask {
                            (
                                mask.x,
                                mask.y,
                                mask.width,
                                mask.height,
                                mask.feather,
                                mask.invert,
                                mask.kind,
                            )
                        } else {
                            return;
                        }
                    };
                    let mut kind_idx = match mk {
                        rook_core::clip::MaskKind::Rectangle => 0,
                        rook_core::clip::MaskKind::Ellipse => 1,
                        rook_core::clip::MaskKind::Freehand => 2,
                    };
                    let mut changed = false;
                    egui::Grid::new("mask_edit").num_columns(2).show(ui, |ui| {
                        ui.label("Type:");
                        egui::ComboBox::from_id_salt("mask_kind")
                            .selected_text(match kind_idx {
                                0 => "Rectangle",
                                1 => "Ellipse",
                                _ => "Freehand",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut kind_idx, 0, "Rectangle");
                                ui.selectable_value(&mut kind_idx, 1, "Ellipse");
                                ui.selectable_value(&mut kind_idx, 2, "Freehand");
                            });
                        let new_kind = match kind_idx {
                            1 => rook_core::clip::MaskKind::Ellipse,
                            2 => rook_core::clip::MaskKind::Freehand,
                            _ => rook_core::clip::MaskKind::Rectangle,
                        };
                        if new_kind != mk {
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("X");
                        if ui.add(egui::DragValue::new(&mut mx).speed(1.0)).changed() {
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("Y");
                        if ui.add(egui::DragValue::new(&mut my).speed(1.0)).changed() {
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("Width");
                        if ui
                            .add(
                                egui::DragValue::new(&mut mw)
                                    .speed(1.0)
                                    .clamp_range(1.0..=10000.0),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("Height");
                        if ui
                            .add(
                                egui::DragValue::new(&mut mh)
                                    .speed(1.0)
                                    .clamp_range(1.0..=10000.0),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("Feather");
                        if ui
                            .add(
                                egui::DragValue::new(&mut mf)
                                    .speed(0.5)
                                    .clamp_range(0.0..=500.0),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        ui.end_row();
                    });
                    let invert_label = if mi { "↔ Invert ✓" } else { "↔ Invert" };
                    if ui.toggle_value(&mut mi, invert_label).changed() {
                        changed = true;
                    }
                    if changed {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            if let Some(ref mut mask) = clip.mask {
                                mask.kind = match kind_idx {
                                    1 => rook_core::clip::MaskKind::Ellipse,
                                    2 => rook_core::clip::MaskKind::Freehand,
                                    _ => rook_core::clip::MaskKind::Rectangle,
                                };
                                mask.x = mx;
                                mask.y = my;
                                mask.width = mw;
                                mask.height = mh;
                                mask.feather = mf;
                                mask.invert = mi;
                            }
                        }
                    }
                    if ui.button("🗑 Remove Mask").clicked() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            clip.mask = None;
                        }
                    }
                }
            });

            // ── Effects ─────────────────────────────────────────────
            ui.collapsing("⚡ Effects", |ui| {
                // Add effect dropdown
                ui.horizontal(|ui| {
                    let add_label = self
                        .add_effect_selection
                        .as_ref()
                        .map(|k| format!("Add {:?}…", k))
                        .unwrap_or_else(|| "➕ Add Effect…".to_string());
                    egui::ComboBox::from_id_salt("add_effect")
                        .selected_text(add_label)
                        .show_ui(ui, |ui| {
                            let effects: &[EffectKind] = &[
                                // Video
                                EffectKind::GaussianBlur,
                                EffectKind::Sharpen,
                                EffectKind::Glow,
                                EffectKind::Brightness,
                                EffectKind::Contrast,
                                EffectKind::Saturation,
                                EffectKind::HueRotate,
                                EffectKind::Exposure,
                                EffectKind::ColorBalance,
                                EffectKind::ColorCurves,
                                EffectKind::ColorWheels,
                                EffectKind::Lut3D,
                                EffectKind::Vignette,
                                EffectKind::FilmGrain,
                                EffectKind::Noise,
                                EffectKind::ChromaKey,
                                EffectKind::LumaKey,
                                EffectKind::Transform,
                                EffectKind::Distort,
                                EffectKind::TextOverlay,
                                EffectKind::Timecode,
                                // Audio
                                EffectKind::Eq,
                                EffectKind::Compressor,
                                EffectKind::Limiter,
                                EffectKind::NoiseGate,
                                EffectKind::Reverb,
                                EffectKind::Delay,
                                EffectKind::PitchShift,
                            ];
                            ui.label("Video Effects:");
                            for eff in effects.iter().filter(|e| {
                                !matches!(
                                    e,
                                    EffectKind::Eq
                                        | EffectKind::Compressor
                                        | EffectKind::Limiter
                                        | EffectKind::NoiseGate
                                        | EffectKind::Reverb
                                        | EffectKind::Delay
                                        | EffectKind::PitchShift
                                )
                            }) {
                                let label = format!("{:?}", eff);
                                if ui
                                    .selectable_label(
                                        self.add_effect_selection.as_ref() == Some(eff),
                                        &label,
                                    )
                                    .clicked()
                                {
                                    self.add_effect_selection = Some(eff.clone());
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        let instance = EffectInstance::new(eff.clone())
                                            .with_param("enabled", true);
                                        clip.filters.push(instance);
                                    }
                                }
                            }
                            ui.separator();
                            ui.label("Audio Effects:");
                            for eff in effects.iter().filter(|e| {
                                matches!(
                                    e,
                                    EffectKind::Eq
                                        | EffectKind::Compressor
                                        | EffectKind::Limiter
                                        | EffectKind::NoiseGate
                                        | EffectKind::Reverb
                                        | EffectKind::Delay
                                        | EffectKind::PitchShift
                                )
                            }) {
                                let label = format!("{:?}", eff);
                                if ui
                                    .selectable_label(
                                        self.add_effect_selection.as_ref() == Some(eff),
                                        &label,
                                    )
                                    .clicked()
                                {
                                    self.add_effect_selection = Some(eff.clone());
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        let instance = EffectInstance::new(eff.clone())
                                            .with_param("enabled", true);
                                        clip.filters.push(instance);
                                    }
                                }
                            }
                        });
                    if self.add_effect_selection.is_some() {
                        self.add_effect_selection = None;
                    }
                });

                // List existing effects
                let filter_snapshot: Vec<_> = {
                    if let Some(clip) = project.timeline.clip(clip_id) {
                        clip.filters
                            .iter()
                            .enumerate()
                            .map(|(i, f)| (i, f.id(), f.kind.clone(), f.enabled, f.params.clone()))
                            .collect()
                    } else {
                        vec![]
                    }
                };

                for (idx, eid, kind, enabled, params) in &filter_snapshot {
                    ui.add_space(4.0);
                    ui.separator();
                    let eid = *eid;
                    let kind = kind.clone();
                    let idx = *idx;
                    let mut enabled = *enabled;
                    let total = filter_snapshot.len();

                    ui.horizontal(|ui| {
                        // Reorder: move up
                        if idx > 0 {
                            if ui.button("▲").on_hover_text("Move up").clicked() {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    clip.filters.swap(idx, idx - 1);
                                }
                            }
                        } else {
                            ui.add_sized(egui::vec2(20.0, 16.0), egui::Label::new(""));
                        }
                        // Reorder: move down
                        if idx + 1 < total {
                            if ui.button("▼").on_hover_text("Move down").clicked() {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    clip.filters.swap(idx, idx + 1);
                                }
                            }
                        } else {
                            ui.add_sized(egui::vec2(20.0, 16.0), egui::Label::new(""));
                        }
                        let enabled_label = if enabled { "✓" } else { "✗" };
                        if ui
                            .selectable_label(enabled, format!("{} {:?}", enabled_label, kind))
                            .clicked()
                        {
                            if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                if let Some(f) = clip.filters.get_mut(idx) {
                                    f.enabled = !f.enabled;
                                }
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("🗑").clicked() {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    clip.filters.retain(|f| f.id() != eid);
                                }
                            }
                        });
                    });

                    // Show parameters for specific effect kinds
                    match kind {
                        EffectKind::GaussianBlur => {
                            let sigma = params.get("sigma").and_then(|v| v.as_f64()).unwrap_or(5.0);
                            let dir = params
                                .get("direction")
                                .and_then(|v| v.as_str())
                                .unwrap_or("both");
                            let mut new_sigma = sigma;
                            let mut new_dir = dir.to_string();
                            ui.horizontal(|ui| {
                                ui.label("Sigma:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_sigma)
                                            .speed(0.1)
                                            .clamp_range(0.1..=100.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param("sigma", serde_json::json!(new_sigma));
                                        }
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Direction:");
                                let mut dir_h = new_dir == "horizontal";
                                let mut dir_v = new_dir == "vertical";
                                let mut dir_b = new_dir == "both";
                                let h_changed = ui.selectable_label(dir_h, "H").clicked();
                                let v_changed = ui.selectable_label(dir_v, "V").clicked();
                                let b_changed = ui.selectable_label(dir_b, "Both").clicked();
                                if h_changed || v_changed || b_changed {
                                    let d = if h_changed || (dir_h && !v_changed && !b_changed) {
                                        "horizontal"
                                    } else if v_changed {
                                        "vertical"
                                    } else {
                                        "both"
                                    };
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param("direction", serde_json::json!(d));
                                        }
                                    }
                                }
                            });
                        }
                        EffectKind::Exposure => {
                            let val = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let mut new_val = val;
                            ui.horizontal(|ui| {
                                ui.label("Exposure:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_val)
                                            .speed(0.01)
                                            .clamp_range(-5.0..=5.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param("amount", serde_json::json!(new_val));
                                        }
                                    }
                                }
                            });
                        }
                        EffectKind::ColorBalance => {
                            let r = params.get("red").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let g = params.get("green").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let b = params.get("blue").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let mut nr = r;
                            let mut ng = g;
                            let mut nb = b;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("R:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut nr)
                                            .speed(0.01)
                                            .clamp_range(-1.0..=1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("G:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut ng)
                                            .speed(0.01)
                                            .clamp_range(-1.0..=1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("B:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut nb)
                                            .speed(0.01)
                                            .clamp_range(-1.0..=1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("red", serde_json::json!(nr));
                                        f.set_param("green", serde_json::json!(ng));
                                        f.set_param("blue", serde_json::json!(nb));
                                    }
                                }
                            }
                        }
                        EffectKind::FilmGrain => {
                            let val = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.1);
                            let mut new_val = val;
                            ui.horizontal(|ui| {
                                ui.label("Amount:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_val)
                                            .speed(0.01)
                                            .clamp_range(0.0..=1.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param("amount", serde_json::json!(new_val));
                                        }
                                    }
                                }
                            });
                        }
                        EffectKind::Brightness | EffectKind::Contrast | EffectKind::Saturation => {
                            let val = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let mut new_val = val;
                            let range = match kind {
                                EffectKind::Brightness => -1.0..=1.0,
                                EffectKind::Contrast => -1.0..=1.0,
                                _ => -1.0..=1.0,
                            };
                            let label = match kind {
                                EffectKind::Brightness => "Brightness:",
                                EffectKind::Contrast => "Contrast:",
                                _ => "Amount:",
                            };
                            ui.horizontal(|ui| {
                                ui.label(label);
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_val)
                                            .speed(0.01)
                                            .clamp_range(range),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param("amount", serde_json::json!(new_val));
                                        }
                                    }
                                }
                            });
                        }
                        EffectKind::HueRotate => {
                            let val = params
                                .get("degrees")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            let mut new_val = val;
                            ui.horizontal(|ui| {
                                ui.label("Degrees:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_val)
                                            .speed(1.0)
                                            .clamp_range(0.0..=360.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param("degrees", serde_json::json!(new_val));
                                        }
                                    }
                                }
                            });
                        }
                        EffectKind::Sharpen => {
                            let val = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.5);
                            let mut new_val = val;
                            ui.horizontal(|ui| {
                                ui.label("Amount:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_val)
                                            .speed(0.01)
                                            .clamp_range(0.0..=1.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param("amount", serde_json::json!(new_val));
                                        }
                                    }
                                }
                            });
                        }
                        EffectKind::Vignette => {
                            let val = params
                                .get("strength")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.5);
                            let mut new_val = val;
                            ui.horizontal(|ui| {
                                ui.label("Strength:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_val)
                                            .speed(0.01)
                                            .clamp_range(0.0..=1.0),
                                    )
                                    .changed()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param("strength", serde_json::json!(new_val));
                                        }
                                    }
                                }
                            });
                        }
                        EffectKind::ChromaKey => {
                            let hue = params.get("hue").and_then(|v| v.as_f64()).unwrap_or(120.0);
                            let tol = params
                                .get("tolerance")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(30.0);
                            let mut new_hue = hue;
                            let mut new_tol = tol;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Key Hue:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_hue)
                                            .speed(1.0)
                                            .clamp_range(0.0..=360.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Tolerance:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut new_tol)
                                            .speed(1.0)
                                            .clamp_range(1.0..=180.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            // Simple hue color preview
                            let (r, g, b) =
                                hsl_to_rgb_for_preview((new_hue as f32 / 360.0).clamp(0.0, 1.0));
                            let color = egui::Color32::from_rgb(
                                (r * 255.0) as u8,
                                (g * 255.0) as u8,
                                (b * 255.0) as u8,
                            );
                            ui.horizontal(|ui| {
                                ui.label("Color:");
                                ui.add(
                                    egui::Button::new("")
                                        .fill(color)
                                        .min_size(egui::vec2(24.0, 16.0)),
                                );
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("hue", serde_json::json!(new_hue));
                                        f.set_param("tolerance", serde_json::json!(new_tol));
                                    }
                                }
                            }
                        }
                        // ── New effect parameter editors ──────────────
                        EffectKind::Glow => {
                            let radius = params
                                .get("radius")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(10.0);
                            let intensity = params
                                .get("intensity")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.5);
                            let mut r = radius;
                            let mut i = intensity;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Radius:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut r)
                                            .speed(0.5)
                                            .clamp_range(0.5..=200.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Intensity:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut i)
                                            .speed(0.01)
                                            .clamp_range(0.0..=5.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("radius", serde_json::json!(r));
                                        f.set_param("intensity", serde_json::json!(i));
                                    }
                                }
                            }
                        }
                        EffectKind::LumaKey => {
                            let thresh = params
                                .get("threshold")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.5);
                            let tol = params
                                .get("tolerance")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.1);
                            let mut t = thresh;
                            let mut l = tol;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Threshold:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut t)
                                            .speed(0.01)
                                            .clamp_range(0.0..=1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Tolerance:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut l)
                                            .speed(0.01)
                                            .clamp_range(0.0..=0.5),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("threshold", serde_json::json!(t));
                                        f.set_param("tolerance", serde_json::json!(l));
                                    }
                                }
                            }
                        }
                        EffectKind::Distort => {
                            let k1 = params.get("k1").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let k2 = params.get("k2").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let mut a = k1;
                            let mut b = k2;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Barrel:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut a)
                                            .speed(0.01)
                                            .clamp_range(-1.0..=1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Pincushion:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut b)
                                            .speed(0.01)
                                            .clamp_range(-1.0..=1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("k1", serde_json::json!(a));
                                        f.set_param("k2", serde_json::json!(b));
                                    }
                                }
                            }
                        }
                        EffectKind::Noise => {
                            let amt = params
                                .get("amount")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.05);
                            let mono = params
                                .get("monochrome")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            let mut a = amt;
                            let mut m = mono;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Amount:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut a)
                                            .speed(0.001)
                                            .clamp_range(0.0..=1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if ui.checkbox(&mut m, "Monochrome").changed() {
                                changed = true;
                            }
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("amount", serde_json::json!(a));
                                        f.set_param("monochrome", serde_json::json!(m));
                                    }
                                }
                            }
                        }
                        EffectKind::Timecode => {
                            let size = params
                                .get("font_size")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(24.0);
                            let pos = params
                                .get("position")
                                .and_then(|v| v.as_str())
                                .unwrap_or("bottom");
                            let mut s = size;
                            let mut p_idx = if pos == "top" {
                                0
                            } else if pos == "center" {
                                1
                            } else {
                                2
                            };
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Size:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut s)
                                            .speed(1.0)
                                            .clamp_range(8.0..=72.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Pos:");
                                if ui.selectable_label(p_idx == 0, "Top").clicked() {
                                    p_idx = 0;
                                    changed = true;
                                }
                                if ui.selectable_label(p_idx == 1, "Center").clicked() {
                                    p_idx = 1;
                                    changed = true;
                                }
                                if ui.selectable_label(p_idx == 2, "Bottom").clicked() {
                                    p_idx = 2;
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("font_size", serde_json::json!(s));
                                        f.set_param(
                                            "position",
                                            serde_json::json!(if p_idx == 0 {
                                                "top"
                                            } else if p_idx == 1 {
                                                "center"
                                            } else {
                                                "bottom"
                                            }),
                                        );
                                    }
                                }
                            }
                        }
                        EffectKind::Eq => {
                            let low = params
                                .get("low_gain")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            let mid = params
                                .get("mid_gain")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            let high = params
                                .get("high_gain")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            let mut lo = low;
                            let mut md = mid;
                            let mut hi = high;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Low:");
                                if ui
                                    .add(egui::Slider::new(&mut lo, -24.0..=12.0).text("dB"))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Mid:");
                                if ui
                                    .add(egui::Slider::new(&mut md, -24.0..=12.0).text("dB"))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("High:");
                                if ui
                                    .add(egui::Slider::new(&mut hi, -24.0..=12.0).text("dB"))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("low_gain", serde_json::json!(lo));
                                        f.set_param("mid_gain", serde_json::json!(md));
                                        f.set_param("high_gain", serde_json::json!(hi));
                                    }
                                }
                            }
                        }
                        EffectKind::Compressor => {
                            let thresh = params
                                .get("threshold")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(-24.0);
                            let ratio = params.get("ratio").and_then(|v| v.as_f64()).unwrap_or(4.0);
                            let gain = params
                                .get("makeup_gain")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            let mut t = thresh;
                            let mut r = ratio;
                            let mut g = gain;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Threshold:");
                                if ui
                                    .add(egui::Slider::new(&mut t, -60.0..=0.0).text("dB"))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Ratio:");
                                if ui
                                    .add(egui::Slider::new(&mut r, 1.0..=20.0).text(":1"))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Gain:");
                                if ui
                                    .add(egui::Slider::new(&mut g, 0.0..=24.0).text("dB"))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("threshold", serde_json::json!(t));
                                        f.set_param("ratio", serde_json::json!(r));
                                        f.set_param("makeup_gain", serde_json::json!(g));
                                    }
                                }
                            }
                        }
                        EffectKind::Limiter => {
                            let ceiling = params
                                .get("ceiling")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(-1.0);
                            let release = params
                                .get("release_ms")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(50.0);
                            let mut c = ceiling;
                            let mut r = release;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Ceiling:");
                                if ui
                                    .add(egui::Slider::new(&mut c, -12.0..=0.0).text("dB"))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Release:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut r)
                                            .speed(1.0)
                                            .clamp_range(1.0..=500.0)
                                            .suffix("ms"),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("ceiling", serde_json::json!(c));
                                        f.set_param("release_ms", serde_json::json!(r));
                                    }
                                }
                            }
                        }
                        EffectKind::Reverb => {
                            let wet = params
                                .get("wet_mix")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.3);
                            let decay = params.get("decay").and_then(|v| v.as_f64()).unwrap_or(1.5);
                            let mut w = wet;
                            let mut d = decay;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Mix:");
                                if ui.add(egui::Slider::new(&mut w, 0.0..=1.0)).changed() {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Decay:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut d)
                                            .speed(0.1)
                                            .clamp_range(0.1..=10.0)
                                            .suffix("s"),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("wet_mix", serde_json::json!(w));
                                        f.set_param("decay", serde_json::json!(d));
                                    }
                                }
                            }
                        }
                        EffectKind::Delay => {
                            let time = params
                                .get("delay_ms")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(250.0);
                            let fb = params
                                .get("feedback")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.3);
                            let mut t = time;
                            let mut f = fb;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Time:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut t)
                                            .speed(10.0)
                                            .clamp_range(10.0..=2000.0)
                                            .suffix("ms"),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Feedback:");
                                if ui.add(egui::Slider::new(&mut f, 0.0..=0.95)).changed() {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("delay_ms", serde_json::json!(t));
                                        f.set_param("feedback", serde_json::json!(f));
                                    }
                                }
                            }
                        }
                        EffectKind::PitchShift => {
                            let semis = params
                                .get("semitones")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            let mut s = semis;
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Semitones:");
                                if ui.add(egui::Slider::new(&mut s, -12.0..=12.0)).changed() {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("semitones", serde_json::json!(s));
                                    }
                                }
                            }
                        }
                        EffectKind::ColorCurves => {
                            ui.label("Curves: adjust RGB curves");
                            draw_curves_editor(ui, project, clip_id, eid);
                        }
                        EffectKind::ColorWheels => {
                            ui.label("Color Wheels: shadows / midtones / highlights");
                            draw_color_wheels(ui, project, clip_id, eid);
                        }
                        EffectKind::Lut3D => {
                            let path = params
                                .get("lut_path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            ui.label(format!(
                                "LUT: {}",
                                if path.is_empty() {
                                    "(none loaded)"
                                } else {
                                    path
                                }
                            ));
                            if ui.button("Load .cube LUT…").clicked() {
                                if let Some(lut_path) = rfd::FileDialog::new()
                                    .add_filter("LUT", &["cube", "3dl", "look"])
                                    .pick_file()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                        if let Some(f) =
                                            clip.filters.iter_mut().find(|f| f.id() == eid)
                                        {
                                            f.set_param(
                                                "lut_path",
                                                serde_json::json!(lut_path.display().to_string()),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        EffectKind::TextOverlay => {
                            let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            let size = params
                                .get("font_size")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(24.0);
                            let mut t = text.to_string();
                            let mut s = size;
                            let mut changed = false;
                            if ui.text_edit_singleline(&mut t).changed() {
                                changed = true;
                            }
                            ui.horizontal(|ui| {
                                ui.label("Size:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut s)
                                            .speed(1.0)
                                            .clamp_range(8.0..=120.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                            if changed {
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid)
                                    {
                                        f.set_param("text", serde_json::json!(t));
                                        f.set_param("font_size", serde_json::json!(s));
                                    }
                                }
                            }
                        }
                        _ => {
                            ui.label(
                                egui::RichText::new("(no configurable parameters)")
                                    .size(11.0)
                                    .color(egui::Color32::from_gray(140)),
                            );
                        }
                    }
                }
            });

            // ── Keyframes ───────────────────────────────────────────
            ui.collapsing("💎 Keyframes", |ui| {
                // "Add keyframe at playhead" button row
                let clip_timeline_in = project
                    .timeline
                    .clip(clip_id)
                    .map(|c| c.timeline_in)
                    .unwrap_or(0);
                let local_frame = *playhead - clip_timeline_in;
                let max_local = project
                    .timeline
                    .clip(clip_id)
                    .map(|c| c.duration())
                    .unwrap_or(1)
                    .max(1);

                ui.label(format!("Playhead at local frame: {}", local_frame));

                // ── Add keyframe for specific properties ──────────
                ui.horizontal(|ui| {
                    ui.label("Quick add:");
                    let props = [
                        ("PX", KeyframeProperty::PositionX),
                        ("PY", KeyframeProperty::PositionY),
                        ("SX", KeyframeProperty::ScaleX),
                        ("SY", KeyframeProperty::ScaleY),
                        ("Rot", KeyframeProperty::Rotation),
                        ("Opac", KeyframeProperty::Opacity),
                        ("Vol", KeyframeProperty::Volume),
                    ];
                    for (label, prop) in &props {
                        let has_kf = kf_at_playhead.contains(&format!("{:?}", prop));
                        let btn_text = if has_kf {
                            format!("◆{label}")
                        } else {
                            format!("◇{label}")
                        };
                        if ui.small_button(btn_text).clicked() {
                            if has_kf {
                                // Remove keyframe at this frame for this property
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    clip.keyframes.retain(|k| {
                                        !(k.at_frame == local_frame
                                            && format!("{:?}", k.property) == format!("{:?}", prop))
                                    });
                                }
                            } else {
                                // Get current value
                                let val = if let Some(clip) = project.timeline.clip(clip_id) {
                                    match prop {
                                        KeyframeProperty::PositionX => {
                                            clip.transform.position.x as f64
                                        }
                                        KeyframeProperty::PositionY => {
                                            clip.transform.position.y as f64
                                        }
                                        KeyframeProperty::ScaleX => clip.transform.scale.x as f64,
                                        KeyframeProperty::ScaleY => clip.transform.scale.y as f64,
                                        KeyframeProperty::Rotation => {
                                            clip.transform.rotation_deg as f64
                                        }
                                        KeyframeProperty::Opacity => clip.transform.opacity as f64,
                                        KeyframeProperty::Volume => {
                                            clip.gain_db.unwrap_or(0.0) as f64
                                        }
                                        _ => 0.0,
                                    }
                                } else {
                                    0.0
                                };
                                if let Some(clip) = project.timeline.clip_mut(clip_id) {
                                    clip.keyframes.push(Keyframe::new(
                                        local_frame,
                                        prop.clone(),
                                        val,
                                    ));
                                }
                            }
                        }
                    }
                });

                ui.separator();

                if kf_list.is_empty() {
                    ui.label(
                        egui::RichText::new("No keyframes on this clip")
                            .size(11.0)
                            .color(egui::Color32::from_gray(140)),
                    );
                } else {
                    // Editable keyframe list
                    let mut remove_ids: Vec<rook_core::ids::KeyframeId> = Vec::new();
                    let mut edits: Vec<(usize, i64, f64)> = Vec::new(); // (index, new_frame, new_value)

                    // Snapshot current keyframe state for editing
                    let kf_snapshot: Vec<_> = {
                        if let Some(clip) = project.timeline.clip(clip_id) {
                            clip.keyframes
                                .iter()
                                .map(|k| {
                                    (
                                        k.id,
                                        k.at_frame,
                                        format!("{:?}", k.property),
                                        k.value,
                                        format!("{:?}", k.easing),
                                    )
                                })
                                .collect()
                        } else {
                            vec![]
                        }
                    };

                    for (idx, (id, frame, prop, value, easing)) in kf_snapshot.iter().enumerate() {
                        ui.horizontal(|ui| {
                            // Frame editing
                            let mut f = *frame;
                            let f_resp = ui.add(
                                egui::DragValue::new(&mut f)
                                    .speed(1.0)
                                    .clamp_range(0..=max_local)
                                    .prefix("f"),
                            );
                            if f_resp.changed() {
                                edits.push((idx, f, *value));
                            }

                            ui.label(egui::RichText::new(prop).size(10.0));

                            // Value editing
                            let mut v = *value;
                            let v_resp = ui.add(egui::DragValue::new(&mut v).speed(0.01));
                            if v_resp.changed() {
                                edits.push((idx, *frame, v));
                            }

                            // Delete
                            if ui.button("🗑").clicked() {
                                remove_ids.push(*id);
                            }
                        });
                    }

                    // Apply edits
                    if !remove_ids.is_empty() || !edits.is_empty() {
                        if let Some(clip) = project.timeline.clip_mut(clip_id) {
                            for id in &remove_ids {
                                clip.keyframes.retain(|k| k.id != *id);
                            }
                            for (idx, new_frame, new_val) in &edits {
                                if let Some(kf) = clip.keyframes.get_mut(*idx) {
                                    kf.at_frame = *new_frame;
                                    kf.value = *new_val;
                                }
                            }
                            // Re-sort by frame
                            clip.keyframes.sort_by_key(|k| k.at_frame);
                        }
                    }
                }
            });

            // ── AI Metadata ─────────────────────────────────────────
            if ai_desc.is_some() || !ai_labels.is_empty() {
                ui.collapsing("🤖 AI Metadata", |ui| {
                    if let Some(ref desc) = ai_desc {
                        ui.label(desc);
                    }
                    if !ai_labels.is_empty() {
                        ui.label(format!("Labels: {}", ai_labels.join(", ")));
                    }
                });
            }
        });
    }

    /// Toggle a keyframe at the given local frame for a property.
    /// If a keyframe exists at this frame + property, remove it.
    /// Otherwise, add one with the given value.
    fn toggle_keyframe(
        clip: &mut rook_core::clip::Clip,
        local_frame: i64,
        property: KeyframeProperty,
        value: f64,
    ) {
        let prop_str = format!("{:?}", property);
        let existing = clip
            .keyframes
            .iter()
            .position(|k| k.at_frame == local_frame && format!("{:?}", k.property) == prop_str);
        if let Some(idx) = existing {
            clip.keyframes.remove(idx);
        } else {
            clip.keyframes
                .push(Keyframe::new(local_frame, property, value));
            clip.keyframes.sort_by_key(|k| k.at_frame);
        }
    }
}

// ── Color Curves Editor ────────────────────────────────────────────────────

fn draw_curves_editor(
    ui: &mut egui::Ui,
    project: &mut Project,
    clip_id: rook_core::ids::ClipId,
    eid: rook_core::ids::EffectId,
) {
    let size = 220.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());
    let painter = ui.painter();

    // Background: gradient from black to white
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(18));
    for y in 0..(size as usize) {
        for x in (0..(size as usize)).step_by(4) {
            let px = rect.left() + x as f32;
            let py = rect.top() + y as f32;
            let v = (x as f32 / size * 255.0) as u8;
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(px, py), egui::vec2(4.0, 1.0)),
                0.0,
                egui::Color32::from_gray(v),
            );
        }
    }

    // Grid lines
    let grid_c = egui::Color32::from_gray(40);
    for i in 1..4 {
        let frac = i as f32 / 4.0;
        let x = rect.left() + frac * size;
        let y = rect.top() + (1.0 - frac) * size;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(0.5, grid_c),
        );
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(0.5, grid_c),
        );
    }

    let channels = [
        ("curve_master", egui::Color32::WHITE),
        ("curve_r", egui::Color32::from_rgb(255, 80, 80)),
        ("curve_g", egui::Color32::from_rgb(80, 255, 80)),
        ("curve_b", egui::Color32::from_rgb(80, 140, 255)),
    ];

    // Draw curves (read-only)
    for (pname, color) in &channels {
        let pts = read_curve_points(project, clip_id, eid, pname);
        if pts.len() < 2 {
            continue;
        }
        for w in 0..pts.len() - 1 {
            let x0 = rect.left() + (pts[w][0] / 255.0) * size;
            let y0 = rect.bottom() - (pts[w][1] / 255.0) * size;
            let x1 = rect.left() + (pts[w + 1][0] / 255.0) * size;
            let y1 = rect.bottom() - (pts[w + 1][1] / 255.0) * size;
            painter.line_segment(
                [egui::pos2(x0, y0), egui::pos2(x1, y1)],
                egui::Stroke::new(1.5, *color),
            );
        }
        for &[px, py] in &pts {
            let sx = rect.left() + (px / 255.0) * size;
            let sy = rect.bottom() - (py / 255.0) * size;
            painter.circle_filled(egui::pos2(sx, sy), 3.5, *color);
            painter.circle_stroke(
                egui::pos2(sx, sy),
                3.5,
                egui::Stroke::new(1.0, egui::Color32::BLACK),
            );
        }
    }

    // Interaction
    if let Some(pos) = response.interact_pointer_pos() {
        if rect.contains(pos) {
            let nx = ((pos.x - rect.left()) / size * 255.0).clamp(0.0, 255.0);
            let ny = ((rect.bottom() - pos.y) / size * 255.0).clamp(0.0, 255.0);
            if response.clicked() {
                let mut pts = read_curve_points(project, clip_id, eid, "curve_master");
                pts.push([nx, ny]);
                pts.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
                write_curve_points(project, clip_id, eid, "curve_master", &pts);
            }
            if response.dragged_by(egui::PointerButton::Primary) {
                let mut pts = read_curve_points(project, clip_id, eid, "curve_master");
                if let Some(nearest) = pts.iter_mut().min_by_key(|p| {
                    ((p[0] - nx) * (p[0] - nx) + (p[1] - ny) * (p[1] - ny) * 1000.0) as i32
                }) {
                    *nearest = [nx, ny];
                }
                pts.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
                write_curve_points(project, clip_id, eid, "curve_master", &pts);
            }
            if response.secondary_clicked() {
                let mut pts = read_curve_points(project, clip_id, eid, "curve_master");
                if pts.len() > 2 {
                    pts.retain(|p| (p[0] - nx) * (p[0] - nx) + (p[1] - ny) * (p[1] - ny) > 36.0);
                    write_curve_points(project, clip_id, eid, "curve_master", &pts);
                }
            }
        }
    }

    // Axis labels
    let font = egui::FontId::proportional(8.0);
    let gc = egui::Color32::from_gray(120);
    painter.text(
        egui::pos2(rect.left(), rect.bottom() + 2.0),
        egui::Align2::LEFT_TOP,
        "0",
        font.clone(),
        gc,
    );
    painter.text(
        egui::pos2(rect.right(), rect.bottom() + 2.0),
        egui::Align2::RIGHT_TOP,
        "255",
        font.clone(),
        gc,
    );
    painter.text(
        egui::pos2(rect.left() - 2.0, rect.bottom()),
        egui::Align2::RIGHT_BOTTOM,
        "0",
        font.clone(),
        gc,
    );
    painter.text(
        egui::pos2(rect.left() - 2.0, rect.top()),
        egui::Align2::RIGHT_TOP,
        "255",
        font,
        gc,
    );

    // Reset
    if ui.small_button("↩ Reset Curves").clicked() {
        let default = serde_json::json!([[0.0, 0.0], [255.0, 255.0]]);
        if let Some(clip) = project.timeline.clip_mut(clip_id) {
            if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid) {
                for (pname, _) in &channels {
                    f.set_param(pname, default.clone());
                }
            }
        }
    }
}

fn read_curve_points(
    project: &Project,
    clip_id: rook_core::ids::ClipId,
    eid: rook_core::ids::EffectId,
    channel: &str,
) -> Vec<[f32; 2]> {
    let default = serde_json::json!([[0.0, 0.0], [255.0, 255.0]]);
    let raw = project
        .timeline
        .clip(clip_id)
        .and_then(|c| c.filters.iter().find(|f| f.id() == eid))
        .and_then(|f| f.params.get(channel).cloned())
        .unwrap_or(default);
    serde_json::from_value::<Vec<[f32; 2]>>(raw)
        .unwrap_or_else(|_| vec![[0.0, 0.0], [255.0, 255.0]])
}

fn write_curve_points(
    project: &mut Project,
    clip_id: rook_core::ids::ClipId,
    eid: rook_core::ids::EffectId,
    channel: &str,
    pts: &[[f32; 2]],
) {
    if let Some(clip) = project.timeline.clip_mut(clip_id) {
        if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid) {
            f.set_param(channel, serde_json::to_value(pts).unwrap());
        }
    }
}

// ── Color Wheels Editor ─────────────────────────────────────────────────────

fn draw_color_wheels(
    ui: &mut egui::Ui,
    project: &mut Project,
    clip_id: rook_core::ids::ClipId,
    eid: rook_core::ids::EffectId,
) {
    let wheel_sz = 130.0;
    for (label, hp, sp, bp) in &[
        (
            "Shadows (Lift)",
            "shadow_hue",
            "shadow_sat",
            "shadow_bright",
        ),
        ("Midtones (Gamma)", "mid_hue", "mid_sat", "mid_bright"),
        ("Highlights (Gain)", "high_hue", "high_sat", "high_bright"),
    ] {
        ui.label(egui::RichText::new(*label).strong().size(11.0));
        let hue = read_wheel_param(project, clip_id, eid, hp, 0.0) as f32;
        let sat = read_wheel_param(project, clip_id, eid, sp, 0.0) as f32;
        let bright = read_wheel_param(project, clip_id, eid, bp, 0.0) as f32;

        let (wrect, wresp) = ui.allocate_exact_size(
            egui::vec2(wheel_sz, wheel_sz),
            egui::Sense::click_and_drag(),
        );
        let p = ui.painter();
        let cx = wrect.center().x;
        let cy = wrect.center().y;
        let r = wheel_sz / 2.0 - 4.0;

        // Paint HSL wheel
        for ri in (1..=r as usize).rev() {
            let rad = ri as f32;
            let steps = (2.0 * std::f32::consts::PI * rad).max(8.0) as usize;
            for i in 0..steps {
                let a = i as f32 / steps as f32 * 2.0 * std::f32::consts::PI;
                let (rc, gc, bc) = hsl2rgb(a / (2.0 * std::f32::consts::PI), rad / r, 0.5);
                p.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(cx + a.cos() * rad, cy + a.sin() * rad),
                        egui::vec2(2.5, 2.5),
                    ),
                    0.0,
                    egui::Color32::from_rgb(
                        (rc * 255.0) as u8,
                        (gc * 255.0) as u8,
                        (bc * 255.0) as u8,
                    ),
                );
            }
        }
        p.circle_stroke(
            egui::pos2(cx, cy),
            r,
            egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
        );

        // Crosshair
        let hx = cx + (hue * 2.0 * std::f32::consts::PI).cos() * sat * r;
        let hy = cy + (hue * 2.0 * std::f32::consts::PI).sin() * sat * r;
        p.line_segment(
            [egui::pos2(hx - 5.0, hy), egui::pos2(hx + 5.0, hy)],
            egui::Stroke::new(2.0, egui::Color32::WHITE),
        );
        p.line_segment(
            [egui::pos2(hx, hy - 5.0), egui::pos2(hx, hy + 5.0)],
            egui::Stroke::new(2.0, egui::Color32::WHITE),
        );
        p.circle_stroke(
            egui::pos2(hx, hy),
            4.0,
            egui::Stroke::new(1.5, egui::Color32::BLACK),
        );

        // Drag
        if let Some(pos) = wresp.interact_pointer_pos() {
            if wresp.dragged_by(egui::PointerButton::Primary) || wresp.clicked() {
                let dx = pos.x - cx;
                let dy = pos.y - cy;
                let dist = (dx * dx + dy * dy).sqrt().min(r);
                let new_hue = ((dy.atan2(dx) / (2.0 * std::f32::consts::PI)) % 1.0 + 1.0) % 1.0;
                let new_sat = (dist / r).clamp(0.0, 1.0);
                write_wheel_param(project, clip_id, eid, hp, new_hue as f64);
                write_wheel_param(project, clip_id, eid, sp, new_sat as f64);
            }
        }

        // Brightness slider
        let mut b = bright;
        ui.add(egui::Slider::new(&mut b, -1.0..=1.0).text("Bright"));
        if (b - bright).abs() > 0.001 {
            write_wheel_param(project, clip_id, eid, bp, b as f64);
        }
        ui.add_space(3.0);
    }
    if ui.small_button("↩ Reset Wheels").clicked() {
        for &(_, hp, sp, bp) in &[
            ("", "shadow_hue", "shadow_sat", "shadow_bright"),
            ("", "mid_hue", "mid_sat", "mid_bright"),
            ("", "high_hue", "high_sat", "high_bright"),
        ] {
            write_wheel_param(project, clip_id, eid, hp, 0.0);
            write_wheel_param(project, clip_id, eid, sp, 0.0);
            write_wheel_param(project, clip_id, eid, bp, 0.0);
        }
    }
}

fn read_wheel_param(
    project: &Project,
    clip_id: rook_core::ids::ClipId,
    eid: rook_core::ids::EffectId,
    name: &str,
    def: f64,
) -> f64 {
    project
        .timeline
        .clip(clip_id)
        .and_then(|c| c.filters.iter().find(|f| f.id() == eid))
        .and_then(|f| f.params.get(name).and_then(|v| v.as_f64()))
        .unwrap_or(def)
}

fn write_wheel_param(
    project: &mut Project,
    clip_id: rook_core::ids::ClipId,
    eid: rook_core::ids::EffectId,
    name: &str,
    v: f64,
) {
    if let Some(clip) = project.timeline.clip_mut(clip_id) {
        if let Some(f) = clip.filters.iter_mut().find(|f| f.id() == eid) {
            f.set_param(name, serde_json::json!(v));
        }
    }
}

// ── HSL helpers ─────────────────────────────────────────────────────────────

fn hsl2rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let f = |t: f32| -> f32 {
        let t = if t < 0.0 {
            t + 1.0
        } else if t > 1.0 {
            t - 1.0
        } else {
            t
        };
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0))
}

// ── HSL-to-RGB helper for chroma key color preview ────────────────────────

fn hsl_to_rgb_for_preview(h: f32) -> (f32, f32, f32) {
    let s = 0.8;
    let l = 0.5;
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue_to_rgb = |t: f32| -> f32 {
        let t = if t < 0.0 {
            t + 1.0
        } else if t > 1.0 {
            t - 1.0
        } else {
            t
        };
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (
        hue_to_rgb(h + 1.0 / 3.0),
        hue_to_rgb(h),
        hue_to_rgb(h - 1.0 / 3.0),
    )
}
