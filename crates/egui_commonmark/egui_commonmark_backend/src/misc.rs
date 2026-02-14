use crate::alerts::AlertBundle;
use crate::typography::TypographyConfig;
use egui::{RichText, TextStyle, Ui, text::LayoutJob};
use std::collections::HashMap;

use crate::pulldown::ScrollableCache;

#[cfg(feature = "better_syntax_highlighting")]
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::{SyntaxDefinition, SyntaxSet},
    util::LinesWithEndings,
};

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
    ) {
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

            job.wrap.max_width = max_width;

            crate::elements::code_block(ui, &self.content, job);
        });
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
#[derive(Debug)]
pub struct CommonMarkCache {
    // Everything stored in `CommonMarkCache` must take into account that
    // the cache is for multiple `CommonMarkviewer`s with different source_ids.
    #[cfg(feature = "better_syntax_highlighting")]
    ps: SyntaxSet,

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
}

#[allow(clippy::derivable_impls)]
impl Default for CommonMarkCache {
    fn default() -> Self {
        Self {
            #[cfg(feature = "better_syntax_highlighting")]
            ps: SyntaxSet::load_defaults_newlines(),
            #[cfg(feature = "better_syntax_highlighting")]
            ts: ThemeSet::load_defaults(),
            link_hooks: HashMap::new(),
            scroll: Default::default(),
            has_installed_loaders: false,
            header_positions: HashMap::new(),
            current_scroll_offset: 0.0,
        }
    }
}

impl CommonMarkCache {
    #[cfg(feature = "better_syntax_highlighting")]
    pub fn add_syntax_from_folder(&mut self, path: &str) {
        let mut builder = self.ps.clone().into_builder();
        let _ = builder.add_from_folder(path, true);
        self.ps = builder.build();
    }

    #[cfg(feature = "better_syntax_highlighting")]
    pub fn add_syntax_from_str(&mut self, s: &str, fallback_name: Option<&str>) {
        let mut builder = self.ps.clone().into_builder();
        let _ = SyntaxDefinition::load_from_str(s, true, fallback_name).map(|d| builder.add(d));
        self.ps = builder.build();
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
