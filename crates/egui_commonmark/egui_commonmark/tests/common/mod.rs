//! Headless frame helpers shared by the integration tests (egui 0.37).
//!
//! egui 0.37 renamed `Context::run` to `run_pass`, made `CentralPanel::show`
//! take a `Ui`, and `debug_assert!`s that a `FullOutput`'s texture deltas were
//! handled before it is dropped. Tests only read shapes, so these helpers
//! clear the deltas and keep the old call shapes.
#![allow(dead_code)]

use egui::{CentralPanel, Context, FullOutput, Id, InnerResponse, RawInput, Ui, Vec2};
use egui::scroll_area::State;

pub trait FrameExt {
    /// `Context::run_pass`, with the texture deltas cleared.
    fn run_frame(&self, input: RawInput, run: impl FnMut(&Context)) -> FullOutput;
    /// `Context::end_pass`, with the texture deltas cleared.
    fn end_frame(&self) -> FullOutput;
}

impl FrameExt for Context {
    fn run_frame(&self, input: RawInput, run: impl FnMut(&Context)) -> FullOutput {
        let mut out = self.run_pass(input, run);
        out.textures_delta.clear();
        out
    }

    fn end_frame(&self) -> FullOutput {
        let mut out = self.end_pass();
        out.textures_delta.clear();
        out
    }
}

pub trait PanelExt {
    /// `CentralPanel::show` on the context's root `Ui` (the 0.33 call shape).
    fn show_ctx<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R>;
}

impl PanelExt for CentralPanel {
    fn show_ctx<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        ctx.root_ui(|root| self.show(root, add))
    }
}

/// Store `state` with its offset replaced by `offset`.
///
/// egui 0.37 keeps `State::offset` private and has no setter; the 0.33 tests
/// wrote it directly to inject a precise scroll position (including one past
/// the content end, #140). A serde round-trip is the only way to keep that
/// precondition exact. The assertion makes a silent failure impossible.
pub fn store_offset(ctx: &Context, id: Id, state: State, offset: Vec2) {
    let mut value = serde_json::to_value(state).expect("State serializes");
    value["offset"] = serde_json::to_value(offset).expect("Vec2 serializes");
    let patched: State = serde_json::from_value(value).expect("State deserializes");
    assert_eq!(patched.unclamped_offset(), offset, "offset injection failed");
    patched.store(ctx, id);
}
