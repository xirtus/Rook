//! Rook — Proxy-based NLE. Apple Glass aesthetic, VideoToolbox decode.

mod app;
mod audio;
mod panels;
mod theme;
mod widgets;

use app::RookApp;
use rook_core::canvas::Canvas;
use rook_core::time::Rational;
use rook_engine::Engine;
use std::sync::{Arc, Mutex};

fn main() -> Result<(), eframe::Error> {
    unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    std::panic::set_hook(Box::new(|info| {
        let loc = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let msg = format!("PANIC at {loc}\n{info}");
        let _ = std::fs::write("/private/tmp/rook_panic.txt", &msg);
        if let Ok(home) = std::env::var("HOME") {
            let _ = std::fs::write(format!("{home}/Desktop/rook_panic.txt"), &msg);
        }
        eprintln!("ROOK PANIC: {msg}");
    }));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("Rook"),
        ..Default::default()
    };
    eframe::run_native(
        "Rook",
        options,
        Box::new(|cc| {
            setup_theme(&cc.egui_ctx);
            let engine = Engine::new("Untitled", Canvas::HD_1080P, Rational::FPS_24);
            Ok(Box::new(RookApp::new(Arc::new(Mutex::new(engine)), None)))
        }),
    )
}

fn setup_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = egui::Color32::from_rgba_premultiplied(40, 40, 50, 255);
    visuals.panel_fill = egui::Color32::from_rgba_premultiplied(32, 32, 42, 255);
    visuals.faint_bg_color = egui::Color32::from_rgba_premultiplied(45, 45, 55, 255);
    visuals.extreme_bg_color = egui::Color32::from_rgba_premultiplied(25, 25, 35, 255);
    visuals.widgets.noninteractive.bg_fill =
        egui::Color32::from_rgba_premultiplied(55, 55, 70, 220);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgba_premultiplied(65, 65, 85, 240);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgba_premultiplied(85, 85, 110, 250);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgba_premultiplied(100, 100, 130, 255);
    visuals.selection.bg_fill = egui::Color32::from_rgb(80, 140, 220);
    ctx.set_visuals(visuals);

    // ── Bold typography ──
    let mut fonts = egui::FontDefinitions::default();
    // Embed Verdana Bold — clean thick sans-serif
    fonts.font_data.insert(
        "Bold".into(),
        std::sync::Arc::new(
            egui::FontData::from_static(include_bytes!("../assets/fonts/Verdana-Bold.ttf")).tweak(
                egui::FontTweak {
                    scale: 1.15,
                    ..Default::default()
                },
            ),
        ),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "Bold".into());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .insert(0, "Bold".into());
    ctx.set_fonts(fonts);

    // ── Boost text sizes globally ──
    let mut style = (*ctx.style()).clone();
    let heading = egui::FontId::proportional(18.0);
    let body = egui::FontId::proportional(15.0);
    let button = egui::FontId::proportional(15.0);
    let small = egui::FontId::proportional(13.0);
    style.text_styles = [
        (egui::TextStyle::Heading, heading),
        (egui::TextStyle::Body, body),
        (egui::TextStyle::Button, button),
        (egui::TextStyle::Small, small),
        (egui::TextStyle::Monospace, egui::FontId::monospace(14.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    ctx.set_style(style);
}
