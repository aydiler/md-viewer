//! Typography configuration for egui_commonmark
//!
//! Provides line height and spacing controls for improved readability.

/// Specifies a measurement that can be either a multiplier of font size or absolute pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Measurement {
    /// Multiplier relative to font size (e.g., 1.5 means 150% of font size)
    Multiplier(f32),
    /// Absolute pixel value
    Pixels(f32),
}

impl Measurement {
    /// Resolve the measurement to pixels given a font size
    pub fn resolve(&self, font_size: f32) -> f32 {
        match self {
            Measurement::Multiplier(m) => font_size * m,
            Measurement::Pixels(p) => *p,
        }
    }
}

impl Default for Measurement {
    fn default() -> Self {
        Measurement::Multiplier(1.0)
    }
}

/// Typography configuration for markdown rendering.
///
/// Controls line height, paragraph spacing, and heading spacing for improved readability.
/// Based on WCAG 2.1 SC 1.4.12 guidelines (1.5x line height recommended).
#[derive(Debug, Clone, Default)]
pub struct TypographyConfig {
    /// Line height for body text. Applied via egui's TextFormat.line_height.
    /// Default: None (uses font's built-in line height)
    pub line_height: Option<Measurement>,

    /// Extra spacing between paragraphs.
    /// Default: None (uses default egui spacing)
    pub paragraph_spacing: Option<Measurement>,

    /// Extra spacing before headings.
    /// Default: None
    pub heading_spacing_above: Option<Measurement>,

    /// Extra spacing after headings.
    /// Default: None
    pub heading_spacing_below: Option<Measurement>,
}

impl TypographyConfig {
    /// Create a new typography config with research-backed defaults.
    ///
    /// - Line height: 1.5x (WCAG 2.1 SC 1.4.12)
    /// - Paragraph spacing: 1.5x font size
    /// - Heading above: 2.0x font size
    /// - Heading below: 0.5x font size
    pub fn recommended() -> Self {
        Self {
            line_height: Some(Measurement::Multiplier(1.5)),
            paragraph_spacing: Some(Measurement::Multiplier(1.5)),
            heading_spacing_above: Some(Measurement::Multiplier(2.0)),
            heading_spacing_below: Some(Measurement::Multiplier(0.5)),
        }
    }

    /// Check if any typography settings are configured
    pub fn is_configured(&self) -> bool {
        self.line_height.is_some()
            || self.paragraph_spacing.is_some()
            || self.heading_spacing_above.is_some()
            || self.heading_spacing_below.is_some()
    }

    /// Resolve line height to pixels given a font size.
    /// Returns None if line_height is not configured.
    pub fn resolve_line_height(&self, font_size: f32) -> Option<f32> {
        self.line_height.map(|m| m.resolve(font_size))
    }

    /// Resolve paragraph spacing to pixels given a font size.
    /// Returns 0.0 if not configured.
    pub fn resolve_paragraph_spacing(&self, font_size: f32) -> f32 {
        self.paragraph_spacing
            .map(|m| m.resolve(font_size))
            .unwrap_or(0.0)
    }

    /// Resolve heading spacing above to pixels given a font size.
    /// Returns 0.0 if not configured.
    pub fn resolve_heading_above(&self, font_size: f32) -> f32 {
        self.heading_spacing_above
            .map(|m| m.resolve(font_size))
            .unwrap_or(0.0)
    }

    /// Resolve heading spacing below to pixels given a font size.
    /// Returns 0.0 if not configured.
    pub fn resolve_heading_below(&self, font_size: f32) -> f32 {
        self.heading_spacing_below
            .map(|m| m.resolve(font_size))
            .unwrap_or(0.0)
    }
}
