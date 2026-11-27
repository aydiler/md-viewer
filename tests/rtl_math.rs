//! Inline formulas inside right-to-left paragraphs (#108) sit in the
//! paragraph's galley, in right-to-left order, on the same baseline as in a
//! left-to-right paragraph.
//!
//! Lives in the root workspace because only the vendored epaint lays
//! right-to-left text out in visual order. The formula is rendered for real, so
//! the test drives frames until its image appears.

use egui::{vec2, CentralPanel, Context, Pos2, RawInput, Rect, Shape};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};
use std::sync::Arc;

const PLACEHOLDER: char = '\u{FFFC}';

fn input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
        ..Default::default()
    }
}

struct Frame {
    texts: Vec<(Pos2, Arc<egui::Galley>)>,
    images: Vec<Rect>,
}

fn frame(ctx: &Context, cache: &mut CommonMarkCache, doc: &str) -> Frame {
    fn walk(shape: &Shape, out: &mut Frame) {
        match shape {
            Shape::Text(t) => out.texts.push((t.pos, t.galley.clone())),
            Shape::Rect(r) if r.fill_texture_id() != egui::TextureId::default() => {
                out.images.push(r.rect)
            }
            Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut pass = ctx.run_pass(input(), |ctx| {
        ctx.root_ui(|root| {
            CentralPanel::default().show(root, |ui| {
                CommonMarkViewer::new()
                    .default_width(Some(600))
                    .show_scrollable("rtl-math", ui, cache, doc);
            });
        });
    });
    pass.textures_delta.clear();
    let mut out = Frame {
        texts: Vec::new(),
        images: Vec::new(),
    };
    for clipped in pass.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// Frames until `count` formula images are painted. The body font is 32 px:
/// at egui's default 12.5 px the formula image's bottom padding already
/// reaches below the text's descent, every formula's lift is 0, and a test
/// could not tell a lifted formula from one that is not.
fn rendered(doc: &str, count: usize) -> Frame {
    let ctx = Context::default();
    ctx.global_style_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(32.0));
    });
    let mut cache = CommonMarkCache::default();
    for _ in 0..400 {
        let f = frame(&ctx, &mut cache, doc);
        if f.images.len() >= count {
            // One more frame so the layout reflects the finished render.
            return frame(&ctx, &mut cache, doc);
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("the formulas did not render within 10 s");
}

/// Absolute boxes of a galley's glyphs: (char, left, right, baseline, row top, row bottom).
fn glyphs(pos: Pos2, galley: &egui::Galley) -> Vec<(char, f32, f32, f32, f32, f32)> {
    let mut out = Vec::new();
    for row in &galley.rows {
        let x = pos.x + row.pos.x;
        let top = pos.y + row.pos.y;
        for g in &row.glyphs {
            out.push((
                g.chr,
                x + g.pos.x,
                x + g.pos.x + g.advance_width,
                top + g.pos.y,
                top,
                top + row.size.y,
            ));
        }
    }
    out
}

fn galley_with<'a>(f: &'a Frame, marker: &str) -> &'a (Pos2, Arc<egui::Galley>) {
    f.texts
        .iter()
        .find(|(_, g)| g.job.text.contains(marker))
        .unwrap_or_else(|| panic!("{marker:?} not painted"))
}

#[test]
fn rtl_formula_sits_between_its_words_on_the_ltr_baseline() {
    let doc = "abcd $x^2$ efgh\n\nשלום $x^2$ עולם\n";
    let f = rendered(doc, 2);
    let mut images = f.images.clone();
    images.sort_by(|a, b| a.top().total_cmp(&b.top()));
    let (ltr_image, rtl_image) = (images[0], images[1]);

    // The reference is the text after the formula. In a left-to-right
    // paragraph a label laid out before a taller widget keeps the row height
    // it saw (egui's wrapping layout), so `abcd` sits higher than `efgh`.
    let (pos, galley) = galley_with(&f, "efgh");
    let ltr_base = glyphs(*pos, galley).iter().find(|g| g.0 == 'e').unwrap().3;
    let (pos, galley) = galley_with(&f, "שלום");
    let rtl = glyphs(*pos, galley);
    let rtl_base = rtl.iter().find(|g| g.0 == 'ש').unwrap().3;

    // The placeholder's line height is the formula's height plus its lift.
    let box_height = rtl
        .iter()
        .find(|g| g.0 == PLACEHOLDER)
        .map(|g| g.5 - g.4)
        .unwrap();
    assert!(
        box_height > rtl_image.height() + 1.0,
        "the formula has no lift ({box_height:.1} px box, {:.1} px image): the baseline check would prove nothing",
        rtl_image.height()
    );

    let ltr_drop = ltr_image.bottom() - ltr_base;
    let rtl_drop = rtl_image.bottom() - rtl_base;
    assert!(
        (ltr_drop - rtl_drop).abs() <= 1.0,
        "formula bottom sits {rtl_drop:.1} px below the RTL baseline, {ltr_drop:.1} px below the LTR one"
    );

    // שלום comes first, so it is right of the formula; עולם is left of it.
    let at = rtl.iter().position(|g| g.0 == PLACEHOLDER).unwrap();
    let first_word_left = rtl[..at]
        .iter()
        .filter(|g| !g.0.is_whitespace())
        .map(|g| g.1)
        .fold(f32::MAX, f32::min);
    let last_word_right = rtl[at..]
        .iter()
        .filter(|g| !g.0.is_whitespace() && g.0 != PLACEHOLDER)
        .map(|g| g.2)
        .fold(f32::MIN, f32::max);
    assert!(
        last_word_right <= rtl_image.left() + 0.5 && rtl_image.right() <= first_word_left + 0.5,
        "expected עולם ..{last_word_right:.1}, formula {:.1}..{:.1}, שלום {first_word_left:.1}..",
        rtl_image.left(),
        rtl_image.right()
    );
}

#[test]
fn rtl_formula_in_a_wrapped_paragraph_overlaps_no_text() {
    let words = "מילה ".repeat(40);
    let doc = format!("{words}$\\sum_{{i=1}}^{{n}} x_i$ {words}\n");
    let f = rendered(&doc, 1);
    let image = f.images[0];
    let (pos, galley) = galley_with(&f, "מילה");
    assert!(galley.rows.len() > 2, "the paragraph should wrap");
    assert!(
        galley.job.text.contains(PLACEHOLDER),
        "the formula is not inside the paragraph's galley"
    );
    for g in glyphs(*pos, galley) {
        if g.0 == PLACEHOLDER || g.0.is_whitespace() {
            continue;
        }
        let glyph = Rect::from_min_max(egui::pos2(g.1, g.4), egui::pos2(g.2, g.5));
        assert!(
            !glyph.shrink(0.5).intersects(image),
            "glyph {:?} at {glyph:?} overlaps the formula {image:?}",
            g.0
        );
    }
}
