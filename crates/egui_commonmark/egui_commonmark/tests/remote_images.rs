//! #289: with remote loading off, an `http(s)` image is never requested.
//!
//! A recording bytes loader sees every URI any image loader asks for, so the
//! test checks the request itself, not just what gets painted. It must be
//! the newest loader: egui asks bytes loaders newest first, and egui_extras'
//! HTTP and file loaders would otherwise answer before it sees anything.

mod common;

use std::sync::{Arc, Mutex};

use common::{FrameExt, PanelExt};
use egui::load::{BytesLoadResult, BytesLoader, LoadError};
use egui::{CentralPanel, Context, Shape};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

#[derive(Default)]
struct RecordingLoader {
    uris: Mutex<Vec<String>>,
}

impl BytesLoader for RecordingLoader {
    fn id(&self) -> &str {
        "remote_images::RecordingLoader"
    }

    fn load(&self, _ctx: &Context, uri: &str) -> BytesLoadResult {
        self.uris.lock().unwrap().push(uri.to_owned());
        if uri.to_ascii_lowercase().starts_with("http") {
            // Stop here: the test must not reach the network.
            Err(LoadError::Loading("recorded".to_owned()))
        } else {
            Err(LoadError::NotSupported)
        }
    }

    fn forget(&self, _uri: &str) {}

    fn forget_all(&self) {}

    fn byte_size(&self) -> usize {
        0
    }
}

const DOCUMENT: &str = "\
# Badges

![build status](https://img.example.com/badge.svg?branch=main)
![](HTTP://img.example.com/upper.png)

A local one: ![logo](assets/logo.png)
";

struct Rendered {
    requested: Vec<String>,
    texts: Vec<String>,
    blocked: usize,
    clicked_request: bool,
}

fn render(load_remote: bool) -> Rendered {
    let ctx = Context::default();
    // The renderer installs these on its first frame; installing them first
    // makes the recorder the newest loader.
    egui_extras::install_image_loaders(&ctx);
    let recorder = Arc::new(RecordingLoader::default());
    ctx.add_bytes_loader(recorder.clone());
    let mut cache = CommonMarkCache::default();
    let mut texts = Vec::new();
    for _ in 0..3 {
        let out = ctx.run_frame(Default::default(), |ctx| {
            CentralPanel::default().show_ctx(ctx, |ui| {
                CommonMarkViewer::new()
                    .default_implicit_uri_scheme("file:///docs/")
                    .load_remote_images(load_remote)
                    .show(ui, &mut cache, DOCUMENT);
            });
        });
        texts = out
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
    }
    let requested = recorder.uris.lock().unwrap().clone();
    Rendered {
        requested,
        texts,
        blocked: cache.blocked_remote_image_count(),
        clicked_request: cache.take_remote_images_request(),
    }
}

#[test]
fn blocked_remote_images_are_never_requested() {
    let blocked = render(false);
    let remote: Vec<_> = blocked
        .requested
        .iter()
        .filter(|uri| uri.to_ascii_lowercase().starts_with("http"))
        .collect();
    assert!(remote.is_empty(), "requested while blocked: {remote:?}");
    assert_eq!(blocked.blocked, 2);
    assert!(
        blocked
            .texts
            .iter()
            .any(|text| text == "Remote image: build status"),
        "alt text placeholder missing: {:?}",
        blocked.texts
    );
    assert!(
        blocked
            .texts
            .iter()
            .any(|text| text == "Remote image: img.example.com"),
        "host placeholder missing: {:?}",
        blocked.texts
    );
    assert!(!blocked.clicked_request);
    // Local images are still loaded.
    assert!(
        blocked
            .requested
            .iter()
            .any(|uri| uri == "file:///docs/assets/logo.png"),
        "local image not requested: {:?}",
        blocked.requested
    );
}

#[test]
fn allowed_remote_images_are_requested_as_before() {
    let allowed = render(true);
    assert!(
        allowed
            .requested
            .iter()
            .any(|uri| uri == "https://img.example.com/badge.svg?branch=main"),
        "the control must see the request: {:?}",
        allowed.requested
    );
    assert_eq!(allowed.blocked, 0);
    assert!(!allowed
        .texts
        .iter()
        .any(|text| text.starts_with("Remote image:")));
}
