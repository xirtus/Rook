//! Plugin browser sidebar panel.
//!
//! View → Plugin Browser
//!
//! Shows all plugins registered in the project, with search + category filter.
//! [+ Apply] adds the plugin as an effect on the selected clip.
//! [Install Plugin…] opens a file dialog for .wasm / .ofx files.
//! [🔄 Refresh] re-scans ~/.local/share/Rook/plugins/.

use rook_core::{
    commands::EditCommand,
    plugin::{PluginCategory, PluginManifest, PluginSource},
};
use rook_engine::Engine;

#[derive(Default)]
pub struct PluginBrowserPanel {
    search: String,
    filter_category: Option<PluginCategory>,
    filter_host: HostFilter,
    /// Which plugin card is expanded (by plugin id u64).
    expanded: Option<u64>,
}

#[derive(Default, PartialEq, Clone, Copy)]
enum HostFilter {
    #[default]
    All,
    Wasm,
    Ofx,
}

impl PluginBrowserPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, engine: &mut Engine) {
        ui.vertical(|ui| {
            // ── Toolbar ───────────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.text_edit_singleline(&mut self.search);
                ui.separator();
                if ui.button("🔄 Refresh").clicked() {
                    engine.apply(EditCommand::RefreshPluginCache).ok();
                }
                ui.separator();
                if ui.small_button("Install…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Plugin", &["wasm", "ofx", "bundle"])
                        .pick_file()
                    {
                        install_plugin(engine, &path);
                    }
                }
            });

            // ── Filters ───────────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Host:");
                ui.selectable_value(&mut self.filter_host, HostFilter::All,  "All");
                ui.selectable_value(&mut self.filter_host, HostFilter::Wasm, "🟢 WASM");
                ui.selectable_value(&mut self.filter_host, HostFilter::Ofx,  "🟠 OFX");
                ui.separator();
                ui.label("Category:");
                egui::ComboBox::from_id_salt("plugin_cat_filter")
                    .selected_text(category_label(self.filter_category.as_ref()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.filter_category, None, "All");
                        for cat in all_categories() {
                            let label = cat.label().to_string();
                            ui.selectable_value(&mut self.filter_category, Some(cat), label);
                        }
                    });
            });

            ui.separator();

            // ── Plugin list ───────────────────────────────────────────────
            let plugins: Vec<PluginManifest> = engine.project().plugins.clone();
            let q = self.search.to_lowercase();

            let filtered: Vec<&PluginManifest> = plugins.iter().filter(|p| {
                // search
                let matches_q = q.is_empty()
                    || p.name.to_lowercase().contains(&q)
                    || p.description.to_lowercase().contains(&q)
                    || p.author.to_lowercase().contains(&q);

                // host
                let matches_host = match self.filter_host {
                    HostFilter::All  => true,
                    HostFilter::Wasm => p.is_wasm(),
                    HostFilter::Ofx  => p.is_ofx(),
                };

                // category
                let matches_cat = self.filter_category.as_ref()
                    .map(|c| c == &p.category)
                    .unwrap_or(true);

                matches_q && matches_host && matches_cat
            }).collect();

            if filtered.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("No plugins found.\nDrop .wasm or .ofx files into\n~/.local/share/Rook/plugins/")
                        .weak().italics());
                });
                return;
            }

            let selected_clip_id = engine.project()
                .timeline.selected_clip_ids.first().copied();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for manifest in &filtered {
                    self.show_plugin_card(ui, engine, manifest, selected_clip_id);
                    ui.add_space(4.0);
                }
            });
        });
    }

    fn show_plugin_card(
        &mut self,
        ui: &mut egui::Ui,
        engine: &mut Engine,
        manifest: &PluginManifest,
        selected_clip: Option<rook_core::clip::ClipId>,
    ) {
        let id_u64 = manifest.id.0;
        let is_expanded = self.expanded == Some(id_u64);

        let frame = egui::Frame::default()
            .inner_margin(egui::Margin::same(8))
            .corner_radius(6.0)
            .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke);

        frame.show(ui, |ui| {
            // ── Header row ────────────────────────────────────────────────
            ui.horizontal(|ui| {
                // Host badge
                let (badge, badge_color) = if manifest.is_wasm() {
                    ("🟢 WASM", egui::Color32::from_rgb(60, 160, 80))
                } else {
                    ("🟠 OFX", egui::Color32::from_rgb(200, 130, 40))
                };
                ui.colored_label(badge_color, badge);

                // Disabled warning
                if manifest.disabled {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 60),
                        format!("⚠ Disabled ({} crashes)", manifest.crash_count),
                    );
                }

                // Name + expand toggle
                let label = egui::RichText::new(&manifest.name).strong();
                if ui.selectable_label(is_expanded, label).clicked() {
                    self.expanded = if is_expanded { None } else { Some(id_u64) };
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // [+ Apply]
                    let can_apply = selected_clip.is_some() && !manifest.disabled;
                    ui.add_enabled_ui(can_apply, |ui| {
                        if ui.small_button("+ Apply").clicked() {
                            if let Some(_clip_id) = selected_clip {
                                engine
                                    .apply(EditCommand::ApplyPlugin {
                                        clip_id: _clip_id,
                                        plugin_id: manifest.id,
                                    })
                                    .ok();
                            }
                        }
                    });
                    // Category chip
                    ui.weak(manifest.category.label());
                });
            });

            // ── Expanded detail ───────────────────────────────────────────
            if is_expanded {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&manifest.description).weak().small());
                ui.horizontal(|ui| {
                    ui.weak(format!("v{}", manifest.version));
                    ui.separator();
                    ui.weak(&manifest.author);
                });

                if !manifest.params.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("{} parameter(s)", manifest.params.len()))
                            .small(),
                    );
                    for param in &manifest.params {
                        ui.label(
                            egui::RichText::new(format!("  · {}", param.name))
                                .small()
                                .weak(),
                        );
                    }
                }

                ui.add_space(4.0);
                // Path display
                let path_str = manifest.source.path().display().to_string();
                ui.horizontal(|ui| {
                    ui.weak("Path:");
                    ui.label(egui::RichText::new(&path_str).small().monospace().weak());
                });

                // [Unload] button
                ui.add_space(2.0);
                if ui.small_button("🗑 Unload").clicked() {
                    engine
                        .apply(EditCommand::UnloadPlugin {
                            plugin_id: manifest.id,
                        })
                        .ok();
                    self.expanded = None;
                }
            }
        });
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn category_label(cat: Option<&PluginCategory>) -> &str {
    cat.map(|c| c.label()).unwrap_or("All")
}

fn all_categories() -> [PluginCategory; 7] {
    [
        PluginCategory::ColorGrade,
        PluginCategory::Keying,
        PluginCategory::BlurSharpen,
        PluginCategory::Stylize,
        PluginCategory::Overlay,
        PluginCategory::Audio,
        PluginCategory::Other,
    ]
}

/// Register a .wasm or .ofx plugin file into the project.
fn install_plugin(engine: &mut Engine, path: &std::path::Path) {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let source = match ext {
        "wasm" => PluginSource::WasmFile(path.to_path_buf()),
        "ofx" | "bundle" => PluginSource::OfxBundle(path.to_path_buf()),
        _ => {
            tracing::warn!(?path, "install_plugin: unrecognised extension");
            return;
        }
    };

    // Try to load sidecar manifest; fall back to a minimal stub.
    let sidecar = path.with_extension("json");
    let manifest = if sidecar.exists() {
        std::fs::read_to_string(&sidecar)
            .ok()
            .and_then(|s| serde_json::from_str::<PluginManifest>(&s).ok())
            .map(|mut m| {
                m.source = source.clone();
                m
            })
    } else {
        None
    }
    .unwrap_or_else(|| {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Plugin")
            .to_string();
        PluginManifest::new(name, "Unknown", "", PluginCategory::Other, source)
    });

    engine.apply(EditCommand::LoadPlugin { manifest }).ok();
}
