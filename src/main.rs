#![cfg_attr(windows, windows_subsystem = "windows")]

mod gui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("hopilo Padupilo")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([980.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native("hopilo Padupilo", options, Box::new(|cc| Ok(Box::new(gui::App::new(cc)))))
}
