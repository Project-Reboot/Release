#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use eframe::egui;
use launcher::ui::{self, LauncherApp};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Project Reboot Launcher")
            .with_app_id("project-reboot-launcher")
            .with_inner_size([ui::WIDTH, ui::HEIGHT])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_decorations(false)
            .with_icon(Arc::new(ui::window_icon())),
        centered: true,
        ..Default::default()
    };
    eframe::run_native("Project Reboot Launcher", options, Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))))
}
