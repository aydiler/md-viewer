use crate::alerts::AlertBundle;
use crate::typography::TypographyConfig;
use egui::{RichText, TextStyle, Ui, text::LayoutJob};
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "mermaid")]
use std::collections::hash_map::DefaultHasher;
#[cfg(feature = "mermaid")]
use std::hash::{Hash, Hasher};
#[cfg(feature = "mermaid")]
use std::sync::mpsc;

use crate::pulldown::ScrollableCache;

#[cfg(feature = "better_syntax_highlighting")]
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::{SyntaxDefinition, SyntaxSet},
    util::LinesWithEndings,
};

#[cfg(any(feature = "better_syntax_highlighting", feature = "mermaid"))]
use std::sync::LazyLock;

#[cfg(feature = "better_syntax_highlighting")]
static GLOBAL_SYNTAX_SET: LazyLock<Arc<SyntaxSet>> =
    LazyLock::new(|| Arc::new(SyntaxSet::load_defaults_newlines()));


#[cfg(feature = "better_syntax_highlighting")]
const DEFAULT_THEME_LIGHT: &str = "base16-ocean.light";
#[cfg(feature = "better_syntax_highlighting")]
const DEFAULT_THEME_DARK: &str = "base16-ocean.dark";

pub struct CommonMarkOptions<'f> {
    pub indentation_spaces: usize,
    pub max_image_width: Option<usize>,
    pub show_alt_text_on_hover: bool,
    pub default_width: Option<usize>,
    #[cfg(feature = "better_syntax_highlighting")]
    pub theme_light: String,
    #[cfg(feature = "better_syntax_highlighting")]
    pub theme_dark: String,
    pub use_explicit_uri_scheme: bool,
    pub default_implicit_uri_scheme: String,
    pub alerts: AlertBundle,
    /// Whether to present a mutable ui for things like checkboxes
    pub mutable: bool,
    pub math_fn: Option<&'f crate::RenderMathFn>,
    pub html_fn: Option<&'f crate::RenderHtmlFn>,
    /// Typography configuration for line height and spacing
    pub typography: TypographyConfig,
}

impl std::fmt::Debug for CommonMarkOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("CommonMarkOptions");

        s.field("indentation_spaces", &self.indentation_spaces)
            .field("max_image_width", &self.max_image_width)
            .field("show_alt_text_on_hover", &self.show_alt_text_on_hover)
            .field("default_width", &self.default_width);

        #[cfg(feature = "better_syntax_highlighting")]
        s.field("theme_light", &self.theme_light)
            .field("theme_dark", &self.theme_dark);

        s.field("use_explicit_uri_scheme", &self.use_explicit_uri_scheme)
            .field(
                "default_implicit_uri_scheme",
                &self.default_implicit_uri_scheme,
            )
            .field("alerts", &self.alerts)
            .field("mutable", &self.mutable)
            .field("typography", &self.typography)
            .finish()
    }
}

impl Default for CommonMarkOptions<'_> {
    fn default() -> Self {
        Self {
            indentation_spaces: 4,
            max_image_width: None,
            show_alt_text_on_hover: true,
            default_width: None,
            #[cfg(feature = "better_syntax_highlighting")]
            theme_light: DEFAULT_THEME_LIGHT.to_owned(),
            #[cfg(feature = "better_syntax_highlighting")]
            theme_dark: DEFAULT_THEME_DARK.to_owned(),
            use_explicit_uri_scheme: false,
            default_implicit_uri_scheme: "file://".to_owned(),
            alerts: AlertBundle::gfm(),
            mutable: false,
            math_fn: None,
            html_fn: None,
            typography: TypographyConfig::default(),
        }
    }
}

impl CommonMarkOptions<'_> {
    #[cfg(feature = "better_syntax_highlighting")]
    pub fn curr_theme(&self, ui: &Ui) -> &str {
        if ui.style().visuals.dark_mode {
            &self.theme_dark
        } else {
            &self.theme_light
        }
    }

    pub fn max_width(&self, ui: &Ui) -> f32 {
        let available_width = ui.available_width();

        // Use default_width as the preferred width, but never exceed available_width
        // This ensures text wraps properly when the window is narrower than default_width
        if let Some(default_width) = self.default_width {
            (default_width as f32).min(available_width)
        } else {
            available_width
        }
    }
}

#[derive(Default, Clone)]
pub struct Style {
    pub heading: Option<u8>,
    pub strong: bool,
    pub emphasis: bool,
    pub strikethrough: bool,
    pub quote: bool,
    pub code: bool,
}

impl Style {
    pub fn to_richtext(&self, ui: &Ui, text: &str) -> RichText {
        self.to_richtext_with_typography(ui, text, None)
    }

    pub fn to_richtext_with_typography(
        &self,
        ui: &Ui,
        text: &str,
        typography: Option<&TypographyConfig>,
    ) -> RichText {
        let mut rich_text = RichText::new(text);

        // Get the base font size for resolving typography measurements
        let base_font_size = ui
            .style()
            .text_styles
            .get(&TextStyle::Body)
            .map_or(14.0, |d| d.size);

        if let Some(level) = self.heading {
            // Evidence-based heading scale using Major Third ratio (1.25×)
            // Research: Nielsen Norman Group "layer-cake" scanning pattern requires distinct hierarchy
            // H1: 2× base (32px), H2: 1.6× (26px), H3: 1.25× (20px), H4: 1.125× (18px)
            let (size, is_bold) = match level {
                0 => (base_font_size * 2.0, true),      // H1: 32px (2×)
                1 => (base_font_size * 1.6, true),     // H2: 26px (1.6×)
                2 => (base_font_size * 1.25, true),    // H3: 20px (1.25×)
                3 => (base_font_size * 1.125, true),   // H4: 18px (1.125×)
                4 => (base_font_size, false),          // H5: 16px (base, no bold)
                _ => (base_font_size * 0.875, false),  // H6: 14px (0.875×, no bold)
            };

            rich_text = rich_text.size(size);
            if is_bold {
                rich_text = rich_text.strong();
            }
            if level == 0 {
                rich_text = rich_text.heading();
            }

            // Apply heading-specific line height (tighter than body per research)
            // Headings use 1.2-1.3× line height vs 1.5× for body text
            if let Some(typo) = typography {
                if let Some(line_height) = typo.line_height {
                    let heading_line_height = match line_height {
                        crate::typography::Measurement::Multiplier(m) => {
                            // Scale down: 1.5× body → 1.3× heading
                            let heading_multiplier = 1.0 + (m - 1.0) * 0.6;
                            crate::typography::Measurement::Multiplier(heading_multiplier)
                        }
                        crate::typography::Measurement::Pixels(p) => {
                            crate::typography::Measurement::Pixels(p * 0.8)
                        }
                    };
                    rich_text = rich_text.line_height(Some(heading_line_height.resolve(size)));
                }
            }
        } else {
            // Apply line height for body text
            if let Some(typo) = typography {
                if let Some(resolved) = typo.resolve_line_height(base_font_size) {
                    rich_text = rich_text.line_height(Some(resolved));
                }
            }
        }

        if self.quote {
            rich_text = rich_text.weak();
        }

        if self.strong {
            rich_text = rich_text.strong();
        }

        if self.emphasis {
            // FIXME: Might want to add some space between the next text
            rich_text = rich_text.italics();
        }

        if self.strikethrough {
            rich_text = rich_text.strikethrough();
        }

        if self.code {
            rich_text = rich_text.code();
        }

        rich_text
    }
}

#[derive(Default)]
pub struct Link {
    pub destination: String,
    pub text: Vec<RichText>,
}

impl Link {
    pub fn end(self, ui: &mut Ui, cache: &mut CommonMarkCache) {
        let Self { destination, text } = self;

        let mut layout_job = LayoutJob::default();
        for t in text {
            t.append_to(
                &mut layout_job,
                ui.style(),
                egui::FontSelection::Default,
                egui::Align::LEFT,
            );
        }

        // Apply underline and hyperlink color to all sections for better visibility
        let link_color = ui.visuals().hyperlink_color;
        for section in &mut layout_job.sections {
            section.format.underline = egui::Stroke::new(1.0, link_color);
            section.format.color = link_color;
            // Remove extra line height to bring underline closer to text
            section.format.line_height = None;
        }

        // Use clickable label to preserve our custom underline styling
        let response = ui.add(
            egui::Label::new(layout_job)
                .selectable(false)
                .sense(egui::Sense::click()),
        );

        let is_hook = cache.link_hooks().contains_key(&destination);

        if response.clicked() || response.middle_clicked() {
            if is_hook {
                cache.link_hooks_mut().insert(destination.clone(), true);
            } else {
                ui.ctx().open_url(egui::OpenUrl::new_tab(&destination));
            }
        }

        // Show pointer cursor and URL on hover
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if !is_hook {
                response.on_hover_text(&destination);
            }
        }
    }
}

pub struct Image {
    pub uri: String,
    pub alt_text: Vec<RichText>,
}

impl Image {
    // FIXME: string conversion
    pub fn new(uri: &str, options: &CommonMarkOptions) -> Self {
        let has_scheme = uri.contains("://") || uri.starts_with("data:");
        let uri = if options.use_explicit_uri_scheme || has_scheme {
            uri.to_string()
        } else if uri.starts_with('/') {
            // Absolute path — use file:// directly
            format!("file://{uri}")
        } else {
            // Relative path — prepend configured base URI
            format!("{}{uri}", options.default_implicit_uri_scheme)
        };

        Self {
            uri,
            alt_text: Vec::new(),
        }
    }

    pub fn end(self, ui: &mut Ui, options: &CommonMarkOptions) {
        let response = ui.add(
            egui::Image::from_uri(&self.uri)
                .fit_to_original_size(1.0)
                .max_width(options.max_width(ui)),
        );

        if !self.alt_text.is_empty() && options.show_alt_text_on_hover {
            response.on_hover_ui_at_pointer(|ui| {
                for alt in self.alt_text {
                    ui.label(alt);
                }
            });
        }
    }
}

pub struct CodeBlock {
    pub lang: Option<String>,
    pub content: String,
}

impl CodeBlock {
    pub fn end(
        &self,
        ui: &mut Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
        id: egui::Id,
    ) {
        #[cfg(feature = "mermaid")]
        if self.lang.as_deref() == Some("mermaid") {
            self.render_mermaid(ui, cache, options, max_width);
            return;
        }

        ui.scope(|ui| {
            Self::pre_syntax_highlighting(cache, options, ui);

            // Calculate code line height from typography config
            let mono_font_size = ui.text_style_height(&TextStyle::Monospace);
            let code_line_height = options.typography.resolve_code_line_height(mono_font_size);

            // Build the LayoutJob for syntax highlighting
            let mut job = if let Some(lang) = &self.lang {
                self.syntax_highlighting(cache, options, lang, ui, &self.content, code_line_height)
            } else {
                plain_highlighting(ui, &self.content, code_line_height)
            };

            // Don't wrap code block text - use horizontal scroll instead
            job.wrap.max_width = f32::INFINITY;

            crate::elements::code_block(ui, &self.content, job, max_width, id);
        });
    }
}

#[cfg(feature = "mermaid")]
enum MermaidState {
    /// Background thread is rendering this diagram
    Rendering,
    /// Rendered and ready to display (texture is 2x for crisp lightbox zoom)
    Ready {
        texture: egui::TextureHandle,
        size: egui::Vec2,
    },
    /// Rendering failed
    Error(String),
}

#[cfg(feature = "mermaid")]
struct MermaidRenderResult {
    hash: u64,
    result: Result<MermaidRendered, String>,
}

#[cfg(feature = "mermaid")]
struct MermaidRendered {
    image: egui::ColorImage,
    size: egui::Vec2,
}

#[cfg(feature = "mermaid")]
static MERMAID_FONTDB: LazyLock<Arc<resvg::usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut db = resvg::usvg::fontdb::Database::new();
    db.load_system_fonts();
    Arc::new(db)
});

#[cfg(feature = "mermaid")]
fn rasterize_mermaid_svg(svg_bytes: &[u8]) -> Option<(egui::ColorImage, egui::Vec2)> {
    let opts = resvg::usvg::Options {
        fontdb: Arc::clone(&MERMAID_FONTDB),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(svg_bytes, &opts).ok()?;
    let svg_size = tree.size();

    // Rasterize at 2x for crisp lightbox zoom (matches LightboxState::new)
    let scale = 2.0_f32;
    let w = (svg_size.width() * scale) as u32;
    let h = (svg_size.height() * scale) as u32;
    if w == 0 || h == 0 {
        return None;
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // Convert premultiplied RGBA → straight RGBA for egui
    let pixels = pixmap.data();
    let mut rgba = Vec::with_capacity(pixels.len());
    for chunk in pixels.chunks_exact(4) {
        let a = chunk[3] as f32 / 255.0;
        if a > 0.0 {
            rgba.push((chunk[0] as f32 / a).min(255.0) as u8);
            rgba.push((chunk[1] as f32 / a).min(255.0) as u8);
            rgba.push((chunk[2] as f32 / a).min(255.0) as u8);
        } else {
            rgba.push(0);
            rgba.push(0);
            rgba.push(0);
        }
        rgba.push(chunk[3]);
    }

    let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
    let size = egui::vec2(svg_size.width(), svg_size.height());
    Some((image, size))
}

#[cfg(feature = "mermaid")]
impl CodeBlock {
    fn render_mermaid(
        &self,
        ui: &mut Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        let mut hasher = DefaultHasher::new();
        self.content.hash(&mut hasher);
        let hash = hasher.finish();

        // Poll for completed background renders
        while let Ok(result) = cache.mermaid_rx.try_recv() {
            // Clear the rendering slot if this result is from the active thread
            if cache.mermaid_rendering == Some(result.hash) {
                cache.mermaid_rendering = None;
            }
            match result.result {
                Ok(rendered) => {
                    let texture = ui.ctx().load_texture(
                        format!("mermaid_{}", result.hash),
                        rendered.image,
                        egui::TextureOptions::LINEAR,
                    );
                    cache.mermaid_states.insert(
                        result.hash,
                        MermaidState::Ready {
                            texture,
                            size: rendered.size,
                        },
                    );
                }
                Err(err) => {
                    cache.mermaid_states.insert(result.hash, MermaidState::Error(err));
                }
            }
        }

        // First encounter: insert as Rendering placeholder, spawn only if slot is free
        if !cache.mermaid_states.contains_key(&hash) {
            cache.mermaid_states.insert(hash, MermaidState::Rendering);

            if cache.mermaid_rendering.is_none() {
                Self::spawn_mermaid_render(hash, &self.content, cache);
            }
        }

        // Promote: if this diagram is waiting and no thread is active, spawn now.
        // Since egui processes blocks in document order, the first Rendering block
        // encountered after the slot clears is always the topmost waiting one.
        if matches!(cache.mermaid_states.get(&hash), Some(MermaidState::Rendering))
            && cache.mermaid_rendering.is_none()
        {
            Self::spawn_mermaid_render(hash, &self.content, cache);
        }

        // Display based on current state
        let mut clicked: Option<(egui::TextureHandle, egui::Vec2)> = None;

        match cache.mermaid_states.get(&hash) {
            Some(MermaidState::Rendering) => {
                // Placeholder with loading text
                let w = max_width.min(options.max_width(ui));
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(w, 200.0), egui::Sense::hover());
                if ui.is_rect_visible(rect) {
                    ui.painter()
                        .rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Rendering diagram\u{2026}",
                        egui::FontId::proportional(14.0),
                        ui.visuals().text_color().gamma_multiply(0.5),
                    );
                }
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            Some(MermaidState::Ready { texture, size }) => {
                let sized_texture = egui::load::SizedTexture::new(texture.id(), *size);
                let response = ui.add(
                    egui::Image::new(egui::ImageSource::Texture(sized_texture))
                        .fit_to_original_size(1.0)
                        .max_width(options.max_width(ui).min(max_width))
                        .sense(egui::Sense::click()),
                );
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    clicked = Some((texture.clone(), *size));
                }
            }
            Some(MermaidState::Error(err_msg)) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Mermaid render error: {err_msg}"),
                );
            }
            None => unreachable!(),
        }

        if let Some(data) = clicked {
            cache.clicked_mermaid = Some(data);
        }
    }

    /// Spawn a background thread to render a mermaid diagram and mark it as active.
    fn spawn_mermaid_render(hash: u64, content: &str, cache: &mut CommonMarkCache) {
        cache.mermaid_rendering = Some(hash);
        let content = content.to_owned();
        let tx = cache.mermaid_tx.clone();
        let renderer = cache.mermaid_renderer.clone();

        std::thread::spawn(move || {
            let result = match renderer.render_svg_readable_sync(&content) {
                Ok(Some(svg_string)) => {
                    let svg_string = CodeBlock::sanitize_svg_font_family(&svg_string);
                    let svg_string = CodeBlock::strip_stroke_text(&svg_string);
                    let svg_string = CodeBlock::wrap_fallback_text(&svg_string);
                    let svg_bytes = svg_string.into_bytes();

                    match rasterize_mermaid_svg(&svg_bytes) {
                        Some((image, size)) => Ok(MermaidRendered { image, size }),
                        None => Err("Failed to rasterize SVG".to_string()),
                    }
                }
                Ok(None) => Err("Unknown diagram type".to_string()),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(MermaidRenderResult { hash, result });
        });
    }

    /// Sanitize SVG font-family values for resvg compatibility.
    /// Handles both XML attributes (`font-family="..."`) and CSS properties
    /// (`font-family: ...;`) since merman outputs fonts in CSS format.
    /// Replaces web fonts (Arial, Trebuchet MS, etc.) that may not be installed
    /// with concrete system font names common on Linux. Also sets a default
    /// font-family on the root `<svg>` element for text that lacks one
    /// (e.g. sequence diagram labels).
    fn sanitize_svg_font_family(svg: &str) -> String {
        // DejaVu Sans first — it has the widest Unicode coverage (including →, ←, etc.)
        // which prevents resvg from falling back to a monospace/bold font for missing glyphs.
        let safe_attr = "DejaVu Sans, Noto Sans, Liberation Sans";
        // Use single quotes — double quotes would break style="..." XML attributes
        let safe_css = "'DejaVu Sans', 'Noto Sans', 'Liberation Sans', sans-serif";

        let mut result = String::with_capacity(svg.len());
        let mut remaining = svg;

        while let Some(pos) = remaining.find("font-family") {
            result.push_str(&remaining[..pos]);
            let after = &remaining[pos + "font-family".len()..];

            if let Some(after_eq) = after.strip_prefix("=\"") {
                // XML attribute: font-family="..."
                result.push_str("font-family=\"");
                result.push_str(safe_attr);
                result.push('"');
                // Skip old value — find closing `"` (followed by `>`, ` `, `/`, or EOF)
                let bytes = after_eq.as_bytes();
                let mut end = after_eq.len();
                for i in 0..bytes.len() {
                    if bytes[i] == b'"' {
                        match bytes.get(i + 1) {
                            Some(b'>') | Some(b' ') | Some(b'/') | None => {
                                end = i + 1;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                remaining = &after_eq[end..];
            } else if after.starts_with(':') {
                // CSS property: font-family: ...; or font-family:...;
                result.push_str("font-family: ");
                result.push_str(safe_css);
                // Skip colon, optional whitespace, and value until ; or }
                let after_colon = &after[1..];
                let trimmed = after_colon.trim_start();
                if let Some(end) = trimmed.find(|c: char| c == ';' || c == '}') {
                    remaining = &trimmed[end..];
                } else {
                    remaining = "";
                }
            } else {
                // Not a font-family declaration we recognize
                result.push_str("font-family");
                remaining = after;
            }
        }
        result.push_str(remaining);

        // Set default font-family on root <svg> so text without explicit
        // font-family (e.g. sequence diagram labels) inherits a consistent font.
        if let Some(svg_pos) = result.find("<svg ") {
            if !result[svg_pos..].starts_with(&format!("<svg font-family")) {
                let insert_pos = svg_pos + 5; // after "<svg "
                result.insert_str(insert_pos, &format!("font-family=\"{}\" ", safe_attr));
            }
        }

        result
    }

    /// Remove stroke-outline `<text>` elements from fallback groups.
    /// merman duplicates each label with `stroke="#fff" stroke-width="3"` for
    /// readability on colored backgrounds, but this makes text look bold/fat
    /// when rendered by resvg on white node backgrounds.
    fn strip_stroke_text(svg: &str) -> String {
        if !svg.contains("stroke=\"#fff\"") {
            return svg.to_owned();
        }
        let mut result = String::with_capacity(svg.len());
        let mut remaining = svg;

        while let Some(pos) = remaining.find("<text ") {
            let tag_end = remaining[pos..].find('>').map(|p| pos + p + 1);
            let Some(tag_end) = tag_end else {
                break;
            };
            let tag = &remaining[pos..tag_end];

            if tag.contains("stroke=\"#fff\"") {
                // Copy everything before this <text>, skip the element entirely
                result.push_str(&remaining[..pos]);
                if let Some(close) = remaining[pos..].find("</text>") {
                    remaining = &remaining[pos + close + 7..];
                } else {
                    remaining = &remaining[tag_end..];
                }
            } else {
                // Keep this text element — copy up to and including the tag
                result.push_str(&remaining[..tag_end]);
                remaining = &remaining[tag_end..];
            }
        }
        result.push_str(remaining);
        result
    }

    /// Word-wrap long fallback text in merman's readable SVG output.
    /// merman places all node label text on a single line in the fallback `<text>` elements,
    /// but the node rects are sized for wrapped text. This causes overflow when text is long.
    /// We split long labels into multiple `<tspan>` lines to fit within nodes.
    fn wrap_fallback_text(svg: &str) -> String {
        const FALLBACK_MARKER: &str = "data-merman-foreignobject=\"fallback\"";
        if !svg.contains(FALLBACK_MARKER) {
            return svg.to_owned();
        }

        // Average char width at 16px for Noto Sans ≈ 8.5px; node rects cap at ~260px
        // Use ~28 chars as the wrap threshold
        const MAX_CHARS: usize = 28;

        let mut result = String::with_capacity(svg.len() + 512);
        let mut remaining = svg;

        while let Some(marker_pos) = remaining.find(FALLBACK_MARKER) {
            // Find the <g that starts this fallback group
            let g_start = remaining[..marker_pos].rfind('<').unwrap_or(marker_pos);
            // Copy everything before this group
            result.push_str(&remaining[..g_start]);

            // Find </g> end
            let after_marker = &remaining[marker_pos..];
            if let Some(g_end_rel) = after_marker.find("</g>") {
                let g_end = marker_pos + g_end_rel + 4;
                let group = &remaining[g_start..g_end];

                // Process: find <tspan ...>TEXT</tspan> patterns and wrap long ones
                let processed = Self::wrap_tspans_in_group(group, MAX_CHARS);
                result.push_str(&processed);

                remaining = &remaining[g_end..];
            } else {
                result.push_str(&remaining[g_start..]);
                remaining = "";
            }
        }
        result.push_str(remaining);
        result
    }

    /// Process a single fallback `<g>` group: wrap long tspan text, then
    /// recalculate all dy values so visual lines are evenly spaced and centered.
    ///
    /// Each visual line is emitted as its own `<text>` element with a single
    /// `<tspan dy="...">` so that dy is always relative to the text element's
    /// base y (not to a previous tspan).
    fn wrap_tspans_in_group(group: &str, max_chars: usize) -> String {
        const LINE_SPACING: f32 = 16.0; // matches font-size 16px

        // --- Pass 1: collect <text> elements and wrap their tspan content ---

        // We need: the open tag template (for style/attrs), x value, and visual lines.
        let mut open_tag_template = String::new();
        let mut x_val = String::new();
        let mut all_visual_lines: Vec<String> = Vec::new();

        let mut remaining = group;

        // Prefix: everything before the first <text
        let prefix = if let Some(pos) = remaining.find("<text") {
            let p = remaining[..pos].to_string();
            remaining = &remaining[pos..];
            p
        } else {
            return group.to_owned();
        };

        while let Some(text_start) = remaining.find("<text") {
            let text_rest = &remaining[text_start..];
            let Some(open_end) = text_rest.find('>') else { break };

            // Capture the first <text ...> tag as template (all share same attrs)
            if open_tag_template.is_empty() {
                open_tag_template = text_rest[..=open_end].to_string();
            }

            let inner = &text_rest[open_end + 1..];
            let Some(close_pos) = inner.find("</text>") else { break };
            let inner_content = &inner[..close_pos];

            // Extract tspan content
            let mut tspan_remaining = inner_content;
            while let Some(ts) = tspan_remaining.find("<tspan") {
                let ts_rest = &tspan_remaining[ts..];
                let Some(ts_close) = ts_rest.find("</tspan>") else { break };
                let full_tspan = &ts_rest[..ts_close + 8];

                if x_val.is_empty() {
                    x_val = Self::extract_attr(full_tspan, "x").to_string();
                }

                let content_start = full_tspan.find('>').map(|p| p + 1).unwrap_or(0);
                let content = &full_tspan[content_start..ts_close];

                if content.trim().len() > max_chars {
                    all_visual_lines.extend(Self::word_wrap(content.trim(), max_chars));
                } else {
                    all_visual_lines.push(content.to_string());
                }

                tspan_remaining = &ts_rest[ts_close + 8..];
            }

            // Advance past </text>
            remaining = &remaining[text_start + open_end + 1 + close_pos + 7..];
        }

        let suffix = remaining;

        if all_visual_lines.is_empty() {
            return group.to_owned();
        }

        // --- Pass 2: emit one <text> per visual line with centered dy ---
        let total = all_visual_lines.len();
        let mut result = String::with_capacity(group.len() + 512);
        result.push_str(&prefix);

        for (i, line) in all_visual_lines.iter().enumerate() {
            let dy = if total <= 1 {
                0.0
            } else {
                -(total as f32 - 1.0) * LINE_SPACING * 0.5 + i as f32 * LINE_SPACING
            };
            result.push_str(&open_tag_template);
            result.push_str(&format!(
                "<tspan x=\"{}\" dy=\"{}\">{}</tspan></text>",
                x_val, dy, line
            ));
        }

        result.push_str(suffix);
        result
    }

    /// Extract an XML attribute value by name.
    fn extract_attr<'a>(tag: &'a str, name: &str) -> &'a str {
        let needle = format!("{}=\"", name);
        if let Some(start) = tag.find(&needle) {
            let val = &tag[start + needle.len()..];
            if let Some(end) = val.find('"') {
                return &val[..end];
            }
        }
        ""
    }

    /// Word-wrap text at word boundaries, keeping lines under max_chars.
    fn word_wrap(text: &str, max_chars: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_line = String::new();

        for word in text.split_whitespace() {
            if current_line.is_empty() {
                current_line = word.to_string();
            } else if current_line.len() + 1 + word.len() <= max_chars {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current_line));
                current_line = word.to_string();
            }
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }
        lines
    }
}

#[cfg(not(feature = "better_syntax_highlighting"))]
impl CodeBlock {
    fn pre_syntax_highlighting(
        _cache: &mut CommonMarkCache,
        _options: &CommonMarkOptions,
        ui: &mut Ui,
    ) {
        ui.style_mut().visuals.extreme_bg_color = ui.visuals().extreme_bg_color;
    }

    fn syntax_highlighting(
        &self,
        _cache: &mut CommonMarkCache,
        _options: &CommonMarkOptions,
        extension: &str,
        ui: &Ui,
        text: &str,
        code_line_height: Option<f32>,
    ) -> egui::text::LayoutJob {
        simple_highlighting(ui, text, extension, code_line_height)
    }
}

#[cfg(feature = "better_syntax_highlighting")]
impl CodeBlock {
    fn pre_syntax_highlighting(
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
    ) {
        let curr_theme = cache.curr_theme(ui, options);
        let style = ui.style_mut();

        style.visuals.extreme_bg_color = curr_theme
            .settings
            .background
            .map(syntect_color_to_egui)
            .unwrap_or(style.visuals.extreme_bg_color);

        if let Some(color) = curr_theme.settings.selection_foreground {
            style.visuals.selection.bg_fill = syntect_color_to_egui(color);
        }
    }

    fn syntax_highlighting(
        &self,
        cache: &CommonMarkCache,
        options: &CommonMarkOptions,
        extension: &str,
        ui: &Ui,
        text: &str,
        code_line_height: Option<f32>,
    ) -> egui::text::LayoutJob {
        if let Some(syntax) = cache.ps.find_syntax_by_extension(extension) {
            let mut job = egui::text::LayoutJob::default();
            let mut h = HighlightLines::new(syntax, cache.curr_theme(ui, options));

            for line in LinesWithEndings::from(text) {
                let ranges = h.highlight_line(line, &cache.ps).unwrap();
                for v in ranges {
                    let front = v.0.foreground;
                    let mut format = egui::TextFormat::simple(
                        TextStyle::Monospace.resolve(ui.style()),
                        syntect_color_to_egui(front),
                    );
                    // Apply code line height if configured
                    if let Some(line_height) = code_line_height {
                        format.line_height = Some(line_height);
                    }
                    job.append(v.1, 0.0, format);
                }
            }

            job
        } else {
            simple_highlighting(ui, text, extension, code_line_height)
        }
    }
}

fn simple_highlighting(ui: &Ui, text: &str, extension: &str, code_line_height: Option<f32>) -> egui::text::LayoutJob {
    let mut job = egui_extras::syntax_highlighting::highlight(
        ui.ctx(),
        ui.style(),
        &egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style()),
        text,
        extension,
    );
    // Apply code line height to all sections if configured
    if let Some(line_height) = code_line_height {
        for section in &mut job.sections {
            section.format.line_height = Some(line_height);
        }
    }
    job
}

fn plain_highlighting(ui: &Ui, text: &str, code_line_height: Option<f32>) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut format = egui::TextFormat::simple(
        TextStyle::Monospace.resolve(ui.style()),
        ui.style().visuals.text_color(),
    );
    // Apply code line height if configured
    if let Some(line_height) = code_line_height {
        format.line_height = Some(line_height);
    }
    job.append(text, 0.0, format);
    job
}

#[cfg(feature = "better_syntax_highlighting")]
fn syntect_color_to_egui(color: syntect::highlighting::Color) -> egui::Color32 {
    egui::Color32::from_rgb(color.r, color.g, color.b)
}

#[cfg(feature = "better_syntax_highlighting")]
fn default_theme(ui: &Ui) -> &str {
    if ui.style().visuals.dark_mode {
        DEFAULT_THEME_DARK
    } else {
        DEFAULT_THEME_LIGHT
    }
}

/// A cache used for storing content such as images.
pub struct CommonMarkCache {
    // Everything stored in `CommonMarkCache` must take into account that
    // the cache is for multiple `CommonMarkviewer`s with different source_ids.
    #[cfg(feature = "better_syntax_highlighting")]
    ps: Arc<SyntaxSet>,

    #[cfg(feature = "better_syntax_highlighting")]
    ts: ThemeSet,

    link_hooks: HashMap<String, bool>,

    scroll: HashMap<egui::Id, ScrollableCache>,
    pub(self) has_installed_loaders: bool,

    /// Stores the y-position of each header (by normalized title) for scroll navigation.
    /// Populated during rendering, cleared on content change.
    header_positions: HashMap<String, f32>,
    /// Current scroll offset, set before rendering to calculate content-relative positions.
    current_scroll_offset: f32,

    /// Mermaid diagram render states: content hash → rendering/ready/error
    #[cfg(feature = "mermaid")]
    mermaid_states: HashMap<u64, MermaidState>,

    /// Channel sender for background mermaid render results
    #[cfg(feature = "mermaid")]
    mermaid_tx: mpsc::Sender<MermaidRenderResult>,

    /// Channel receiver for background mermaid render results
    #[cfg(feature = "mermaid")]
    mermaid_rx: mpsc::Receiver<MermaidRenderResult>,

    /// Mermaid renderer instance (reused across renders)
    #[cfg(feature = "mermaid")]
    mermaid_renderer: merman::render::HeadlessRenderer,

    /// Set when a mermaid diagram is clicked (texture + logical size for lightbox)
    #[cfg(feature = "mermaid")]
    clicked_mermaid: Option<(egui::TextureHandle, egui::Vec2)>,

    /// Hash of the diagram that currently has an active background thread.
    /// Only one diagram renders at a time so they appear top-to-bottom.
    #[cfg(feature = "mermaid")]
    mermaid_rendering: Option<u64>,
}

impl std::fmt::Debug for CommonMarkCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("CommonMarkCache");
        s.field("link_hooks", &self.link_hooks)
            .field("scroll", &self.scroll)
            .field("has_installed_loaders", &self.has_installed_loaders)
            .field("header_positions", &self.header_positions)
            .field("current_scroll_offset", &self.current_scroll_offset);
        #[cfg(feature = "mermaid")]
        s.field("mermaid_states_count", &self.mermaid_states.len());
        #[cfg(feature = "mermaid")]
        s.field("clicked_mermaid", &self.clicked_mermaid.is_some());
        s.finish()
    }
}

#[allow(clippy::derivable_impls)]
impl Default for CommonMarkCache {
    fn default() -> Self {
        #[cfg(feature = "mermaid")]
        let (mermaid_tx, mermaid_rx) = mpsc::channel();

        Self {
            #[cfg(feature = "better_syntax_highlighting")]
            ps: Arc::clone(&GLOBAL_SYNTAX_SET),
            #[cfg(feature = "better_syntax_highlighting")]
            ts: ThemeSet::load_defaults(),
            link_hooks: HashMap::new(),
            scroll: Default::default(),
            has_installed_loaders: false,
            header_positions: HashMap::new(),
            current_scroll_offset: 0.0,
            #[cfg(feature = "mermaid")]
            mermaid_states: HashMap::new(),
            #[cfg(feature = "mermaid")]
            mermaid_tx,
            #[cfg(feature = "mermaid")]
            mermaid_rx,
            #[cfg(feature = "mermaid")]
            mermaid_renderer: merman::render::HeadlessRenderer::new()
                .with_text_measurer(Arc::new(merman::render::DeterministicTextMeasurer {
                    // Wider than default (0.55) to accommodate Noto Sans/DejaVu Sans
                    // which are wider than the vendored Trebuchet MS metrics.
                    // 0.65 prevents text overlap in complex flowcharts with long labels.
                    char_width_factor: 0.65,
                    line_height_factor: 0.0, // use internal default
                })),
            #[cfg(feature = "mermaid")]
            clicked_mermaid: None,
            #[cfg(feature = "mermaid")]
            mermaid_rendering: None,
        }
    }
}

impl CommonMarkCache {
    #[cfg(feature = "better_syntax_highlighting")]
    pub fn add_syntax_from_folder(&mut self, path: &str) {
        let mut builder = (*self.ps).clone().into_builder();
        let _ = builder.add_from_folder(path, true);
        self.ps = Arc::new(builder.build());
    }

    #[cfg(feature = "better_syntax_highlighting")]
    pub fn add_syntax_from_str(&mut self, s: &str, fallback_name: Option<&str>) {
        let mut builder = (*self.ps).clone().into_builder();
        let _ = SyntaxDefinition::load_from_str(s, true, fallback_name).map(|d| builder.add(d));
        self.ps = Arc::new(builder.build());
    }

    #[cfg(feature = "better_syntax_highlighting")]
    /// Add more color themes for code blocks(.tmTheme files). Set the color theme with
    /// [`syntax_theme_dark`](CommonMarkViewer::syntax_theme_dark) and
    /// [`syntax_theme_light`](CommonMarkViewer::syntax_theme_light)
    pub fn add_syntax_themes_from_folder(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), syntect::LoadingError> {
        self.ts.add_from_folder(path)
    }

    #[cfg(feature = "better_syntax_highlighting")]
    /// Add color theme for code blocks(.tmTheme files). Set the color theme with
    /// [`syntax_theme_dark`](CommonMarkViewer::syntax_theme_dark) and
    /// [`syntax_theme_light`](CommonMarkViewer::syntax_theme_light)
    pub fn add_syntax_theme_from_bytes(
        &mut self,
        name: impl Into<String>,
        bytes: &[u8],
    ) -> Result<(), syntect::LoadingError> {
        let mut cursor = std::io::Cursor::new(bytes);
        self.ts
            .themes
            .insert(name.into(), ThemeSet::load_from_reader(&mut cursor)?);
        Ok(())
    }

    /// Take the clicked mermaid texture and size (if any). Returns `Some` once per click.
    /// The texture is pre-rasterized at 2x resolution for crisp lightbox zoom.
    #[cfg(feature = "mermaid")]
    pub fn take_clicked_mermaid(&mut self) -> Option<(egui::TextureHandle, egui::Vec2)> {
        self.clicked_mermaid.take()
    }

    /// Clear the cache for all scrollable elements
    pub fn clear_scrollable(&mut self) {
        self.scroll.clear();
    }

    /// Clear the cache for a specific scrollable viewer. Returns false if the
    /// id was not in the cache.
    pub fn clear_scrollable_with_id(&mut self, source_id: impl std::hash::Hash) -> bool {
        self.scroll.remove(&egui::Id::new(source_id)).is_some()
    }

    /// If the user clicks on a link in the markdown render that has `name` as a link. The hook
    /// specified with this method will be set to true. It's status can be acquired
    /// with [`get_link_hook`](Self::get_link_hook). Be aware that all hook state is reset once
    /// [`CommonMarkViewer::show`] gets called
    ///
    /// # Why use link hooks
    ///
    /// egui provides a method for checking links afterwards so why use this instead?
    ///
    /// ```rust
    /// # use egui::__run_test_ctx;
    /// # __run_test_ctx(|ctx| {
    /// ctx.output_mut(|o| for command in &o.commands {
    ///     matches!(command, egui::OutputCommand::OpenUrl(_));
    /// });
    /// # });
    /// ```
    ///
    /// The main difference is that link hooks allows egui_commonmark to check for link hooks
    /// while rendering. Normally when hovering over a link, egui_commonmark will display the full
    /// url. With link hooks this feature is disabled, but to do that all hooks must be known.
    // Works when displayed through egui_commonmark
    #[allow(rustdoc::broken_intra_doc_links)]
    pub fn add_link_hook<S: Into<String>>(&mut self, name: S) {
        self.link_hooks.insert(name.into(), false);
    }

    /// Returns None if the link hook could not be found. Returns the last known status of the
    /// hook otherwise.
    pub fn remove_link_hook(&mut self, name: &str) -> Option<bool> {
        self.link_hooks.remove(name)
    }

    /// Get status of link. Returns true if it was clicked
    pub fn get_link_hook(&self, name: &str) -> Option<bool> {
        self.link_hooks.get(name).copied()
    }

    /// Remove all link hooks
    pub fn link_hooks_clear(&mut self) {
        self.link_hooks.clear();
    }

    /// All link hooks
    pub fn link_hooks(&self) -> &HashMap<String, bool> {
        &self.link_hooks
    }

    /// Raw access to link hooks
    pub fn link_hooks_mut(&mut self) -> &mut HashMap<String, bool> {
        &mut self.link_hooks
    }

    /// Set all link hooks to false
    fn deactivate_link_hooks(&mut self) {
        for v in self.link_hooks.values_mut() {
            *v = false;
        }
    }

    #[cfg(feature = "better_syntax_highlighting")]
    fn curr_theme(&self, ui: &Ui, options: &CommonMarkOptions) -> &Theme {
        self.ts
            .themes
            .get(options.curr_theme(ui))
            // Since we have called load_defaults, the default theme *should* always be available..
            .unwrap_or_else(|| &self.ts.themes[default_theme(ui)])
    }

    /// Set the current scroll offset before rendering.
    /// This is used to calculate content-relative header positions.
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.current_scroll_offset = offset;
    }

    /// Record the y-position of a header for scroll navigation.
    /// Converts viewport-relative position to content-relative using scroll offset.
    /// Only records if not already recorded (first render captures correct position).
    pub fn record_header_position(&mut self, title: &str, viewport_y: f32) {
        let key = title.trim().to_lowercase();
        // Only record on first encounter to avoid position jumping
        if !self.header_positions.contains_key(&key) {
            let content_y = self.current_scroll_offset + viewport_y;
            self.header_positions.insert(key, content_y);
        }
    }

    /// Get the y-position of a header by its title (content-relative).
    /// Returns None if the header hasn't been rendered yet.
    pub fn get_header_position(&self, title: &str) -> Option<f32> {
        let key = title.trim().to_lowercase();
        self.header_positions.get(&key).copied()
    }

    /// Clear all recorded header positions.
    /// Should be called when content changes.
    pub fn clear_header_positions(&mut self) {
        self.header_positions.clear();
    }
}

pub fn scroll_cache<'a>(cache: &'a mut CommonMarkCache, id: &egui::Id) -> &'a mut ScrollableCache {
    if !cache.scroll.contains_key(id) {
        cache.scroll.insert(*id, Default::default());
    }
    cache.scroll.get_mut(id).unwrap()
}

/// Should be called before any rendering
pub fn prepare_show(cache: &mut CommonMarkCache, ctx: &egui::Context) {
    if !cache.has_installed_loaders {
        // Even though the install function can be called multiple times, its not the cheapest
        // so we ensure that we only call it once.
        // This could be done at the creation of the cache, however it is better to keep the
        // cache free from egui's Ui and Context types as this allows it to be created before
        // any egui instances. It also keeps the API similar to before the introduction of the
        // image loaders.
        #[cfg(feature = "embedded_image")]
        crate::data_url_loader::install_loader(ctx);

        egui_extras::install_image_loaders(ctx);
        cache.has_installed_loaders = true;
    }

    cache.deactivate_link_hooks();
}
