use egui::__run_test_ui;
use egui_commonmark_macros_extended::commonmark;

// Check hygiene of the ui expression
fn main() {
    __run_test_ui(|ui| {
        let mut cache = egui_commonmark_backend_extended::CommonMarkCache::default();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Frame::new().show(ui, |not_named_ui| {
                commonmark!(not_named_ui, &mut cache, "# Hello, World");
            })
        });
    });
}
