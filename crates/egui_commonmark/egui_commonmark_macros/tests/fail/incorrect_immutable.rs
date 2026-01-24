use egui_commonmark_macros_extended::commonmark;

// Ensure that the error message is sane
fn main() {
    let mut cache = egui_commonmark_backend_extended::CommonMarkCache::default();
    egui::__run_test_ui(|ui| {
        commonmark!(ui, &cache, "# Hello");
    });
}
