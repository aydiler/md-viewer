use crate::typography::TypographyConfig;
use egui::{self, NumExt, RichText, Sense, TextStyle, Ui, Vec2, epaint};

/// Add extra vertical spacing based on typography configuration.
/// Returns the amount of space added.
#[inline]
pub fn typography_spacing(ui: &mut Ui, spacing: f32) {
    if spacing > 0.0 {
        ui.add_space(spacing);
    }
}

/// Add paragraph spacing based on typography configuration.
#[inline]
pub fn paragraph_end_spacing(ui: &mut Ui, typography: &TypographyConfig) {
    let font_size = ui.text_style_height(&TextStyle::Body);
    let spacing = typography.resolve_paragraph_spacing(font_size);
    typography_spacing(ui, spacing);
}

/// Add heading spacing above based on typography configuration.
#[inline]
pub fn heading_start_spacing(ui: &mut Ui, typography: &TypographyConfig) {
    let font_size = ui.text_style_height(&TextStyle::Body);
    let spacing = typography.resolve_heading_above(font_size);
    typography_spacing(ui, spacing);
}

/// Add heading spacing below based on typography configuration.
#[inline]
pub fn heading_end_spacing(ui: &mut Ui, typography: &TypographyConfig) {
    let font_size = ui.text_style_height(&TextStyle::Body);
    let spacing = typography.resolve_heading_below(font_size);
    typography_spacing(ui, spacing);
}

#[inline]
pub fn rule(ui: &mut Ui, end_line: bool) {
    ui.add(egui::Separator::default().horizontal());
    // This does not add a new line, but instead ends the separator
    if end_line {
        newline(ui);
    }
}

#[inline]
pub fn soft_break(ui: &mut Ui) {
    ui.label(" ");
}

#[inline]
pub fn newline(ui: &mut Ui) {
    ui.label("\n");
}

pub fn bullet_point(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width_body_space(ui) * 4.0, height_body(ui)),
        Sense::hover(),
    );
    ui.painter().circle_filled(
        rect.center(),
        rect.height() / 6.0,
        ui.visuals().strong_text_color(),
    );
}

pub fn bullet_point_hollow(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width_body_space(ui) * 4.0, height_body(ui)),
        Sense::hover(),
    );
    ui.painter().circle(
        rect.center(),
        rect.height() / 6.0,
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(0.6, ui.visuals().strong_text_color()),
    );
}

pub fn number_point(ui: &mut Ui, number: &str) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width_body_space(ui) * 4.0, height_body(ui)),
        Sense::hover(),
    );
    ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        format!("{number}."),
        TextStyle::Body.resolve(ui.style()),
        ui.visuals().strong_text_color(),
    );
}

#[inline]
pub fn footnote_start(ui: &mut Ui, note: &str) {
    ui.label(RichText::new(note).raised().strong().small());
}

pub fn footnote(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width_body_space(ui) * 4.0, height_body(ui)),
        Sense::hover(),
    );
    ui.painter().text(
        rect.right_top(),
        egui::Align2::RIGHT_TOP,
        format!("{text}."),
        TextStyle::Small.resolve(ui.style()),
        ui.visuals().strong_text_color(),
    );
}

fn height_body(ui: &Ui) -> f32 {
    ui.text_style_height(&TextStyle::Body)
}

fn width_body_space(ui: &Ui) -> f32 {
    let id = TextStyle::Body.resolve(ui.style());
    ui.fonts_mut(|f| f.glyph_width(&id, ' '))
}

/// Enhanced/specialized version of egui's code blocks. This one features copy button and borders.
/// Uses selectable Label instead of TextEdit to allow text selection across code block boundaries.
pub fn code_block(ui: &mut Ui, text: &str, layout_job: egui::text::LayoutJob, max_width: f32, id: egui::Id) {
    let text = text.strip_suffix('\n').unwrap_or(text);

    // Reserve space for background drawing
    let where_to_put_background = ui.painter().add(egui::Shape::Noop);

    // Use a Frame with horizontal scroll for wide code blocks.
    // All code blocks use the same width with horizontal scrolling
    // for content that exceeds it.
    let frame_response = egui::Frame::new()
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            // Force all code blocks to fill the available width
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::horizontal()
                .id_salt(id)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(layout_job)
                            .selectable(true)
                            .wrap_mode(egui::TextWrapMode::Extend),
                    )
                })
                .inner
        });

    let frame_rect = frame_response.response.rect;

    // Draw background color + frame
    ui.painter().set(
        where_to_put_background,
        epaint::RectShape::new(
            frame_rect,
            ui.style().noninteractive().corner_radius,
            ui.visuals().extreme_bg_color,
            ui.visuals().widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Outside,
        ),
    );

    // Copy icon
    let spacing = &ui.style().spacing;
    let position = egui::pos2(
        frame_rect.right_top().x - spacing.icon_width * 0.5 - spacing.button_padding.x,
        frame_rect.right_top().y + spacing.button_padding.y * 2.0,
    );

    // Check if we should show ✔ instead of 🗐 if the text was copied and the mouse is hovered
    let persistent_id = ui.make_persistent_id(frame_response.response.id);
    let copied_icon = ui.memory_mut(|m| *m.data.get_temp_mut_or_default::<bool>(persistent_id));

    let copy_button = ui
        .put(
            egui::Rect {
                min: position,
                max: position,
            },
            egui::Button::new(if copied_icon { "✔" } else { "🗐" })
                .small()
                .frame(false)
                .fill(egui::Color32::TRANSPARENT),
        )
        .on_hover_cursor(
            ui.visuals()
                .interact_cursor
                .unwrap_or(egui::CursorIcon::Default),
        );

    // Update icon state in persistent memory
    if copied_icon && !copy_button.hovered() {
        ui.memory_mut(|m| *m.data.get_temp_mut_or_default(persistent_id) = false);
    }
    if !copied_icon && copy_button.clicked() {
        ui.memory_mut(|m| *m.data.get_temp_mut_or_default(persistent_id) = true);
    }

    // Copy full code block text when button clicked
    if copy_button.clicked() {
        ui.ctx().copy_text(text.to_owned());
    }
}

// Stripped down version of egui's Checkbox. The only difference is that this
// creates a noninteractive checkbox. ui.add_enabled could have been used instead,
// but it makes the checkbox too grey.
pub struct ImmutableCheckbox<'a> {
    checked: &'a mut bool,
}

impl<'a> ImmutableCheckbox<'a> {
    pub fn without_text(checked: &'a mut bool) -> Self {
        ImmutableCheckbox { checked }
    }
}

impl egui::Widget for ImmutableCheckbox<'_> {
    fn ui(self, ui: &mut Ui) -> egui::Response {
        let ImmutableCheckbox { checked } = self;

        let spacing = &ui.spacing();
        let icon_width = spacing.icon_width;

        let mut desired_size = egui::vec2(icon_width, 0.0);
        desired_size = desired_size.at_least(Vec2::splat(spacing.interact_size.y));
        desired_size.y = desired_size.y.max(icon_width);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

        if ui.is_rect_visible(rect) {
            let visuals = ui.style().visuals.noninteractive();
            let (small_icon_rect, big_icon_rect) = ui.spacing().icon_rectangles(rect);
            ui.painter().add(epaint::RectShape::new(
                big_icon_rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            ));

            if *checked {
                // Check mark:
                ui.painter().add(egui::Shape::line(
                    vec![
                        egui::pos2(small_icon_rect.left(), small_icon_rect.center().y),
                        egui::pos2(small_icon_rect.center().x, small_icon_rect.bottom()),
                        egui::pos2(small_icon_rect.right(), small_icon_rect.top()),
                    ],
                    visuals.fg_stroke,
                ));
            }
        }

        response
    }
}

pub fn blockquote(ui: &mut Ui, accent: egui::Color32, add_contents: impl FnOnce(&mut Ui)) {
    let start = ui.painter().add(egui::Shape::Noop);
    let response = egui::Frame::new()
        // offset the frame so that we can use the space for the horizontal line and other stuff
        // By not using a separator we have better control
        .outer_margin(egui::Margin {
            left: 10,
            ..Default::default()
        })
        .show(ui, add_contents)
        .response;

    // FIXME: Add some rounding

    ui.painter().set(
        start,
        egui::epaint::Shape::line_segment(
            [
                egui::pos2(response.rect.left_top().x, response.rect.left_top().y + 5.0),
                egui::pos2(
                    response.rect.left_bottom().x,
                    response.rect.left_bottom().y - 5.0,
                ),
            ],
            egui::Stroke::new(3.0, accent),
        ),
    );
}
