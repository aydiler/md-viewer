use std::collections::HashSet;

use egui::{FontData, FontDefinitions, FontFamily};
use egui_commonmark_extended::STRONG_FONT_FAMILY;
use fontique::{
    Blob, Collection, CollectionOptions, FamilyId, FontStyle, FontWeight, GenericFamily, Script,
    SourceCache, SourceKind,
};
use serde::{Deserialize, Serialize};

/// Markdown font presets that emulate how popular markdown viewers pick fonts.
///
/// A preset reorders the app's font chains so the first installed face matches
/// what the emulated viewer would show on Linux. CSS-generic keywords
/// (`-apple-system`, `BlinkMacSystemFont`, `system-ui`, `ui-monospace`,
/// `sans-serif`, `monospace`) have no literal family behind them, so they are
/// transcribed to the concrete faces a Linux browser resolves them to:
/// `system-ui`/UI aliases become GNOME's Adwaita Sans / Cantarell, and generic
/// monospace becomes Noto Sans Mono / DejaVu Sans Mono. Anything still missing
/// falls through to the app's own fallback chain, so Unicode/CJK coverage and
/// bold rendering never regress when switching presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum FontPreset {
    /// The app's own look: best installed general-purpose sans for body text,
    /// egui's bundled monospace face for code.
    #[default]
    Current,
    /// github.com rendered markdown (Primer `.markdown-body`): body
    /// `"Mona Sans VF", -apple-system, BlinkMacSystemFont, "Segoe UI",
    /// "Noto Sans", Helvetica, Arial, sans-serif`; code `ui-monospace,
    /// SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono",
    /// monospace`.
    Github,
    /// VS Code markdown preview (`markdown.preview.fontFamily` default plus
    /// the Linux `editor.fontFamily` default for code): body `-apple-system,
    /// BlinkMacSystemFont, "Segoe WPC", "Segoe UI", system-ui, "Ubuntu",
    /// "Droid Sans", sans-serif`; code `'Droid Sans Mono', monospace`.
    Vscode,
}

impl FontPreset {
    pub(crate) const ALL: [FontPreset; 3] =
        [FontPreset::Current, FontPreset::Github, FontPreset::Vscode];

    /// Menu label for this preset.
    pub(crate) fn label(self) -> &'static str {
        match self {
            FontPreset::Current => "Default",
            FontPreset::Github => "GitHub",
            FontPreset::Vscode => "VS Code",
        }
    }

    /// One-line explanation of what the preset emulates, shown as menu hover text.
    pub(crate) fn description(self) -> &'static str {
        match self {
            FontPreset::Current => {
                "This app's default: best installed sans + bundled mono code font."
            }
            FontPreset::Github => {
                "github.com style: Mona Sans / Segoe UI / Noto Sans body, SF Mono / Liberation Mono code."
            }
            FontPreset::Vscode => {
                "VS Code preview style: Segoe UI / system UI body, Droid Sans Mono code."
            }
        }
    }

    /// `(body line-height, code line-height)` multipliers fed to the renderer.
    /// GitHub: `.markdown-body{line-height:1.5}`, `pre{line-height:1.45}`.
    /// VS Code preview default: `--markdown-line-height: 1.6`, code 1.357em.
    pub(crate) fn line_heights(self) -> (f32, f32) {
        match self {
            FontPreset::Current => (1.5, 1.3),
            FontPreset::Github => (1.5, 1.45),
            FontPreset::Vscode => (1.6, 1.36),
        }
    }

    /// Body families from the emulated viewer's CSS stack, in resolution
    /// order. `None` keeps the app's own primary sans selection.
    ///
    /// Pure macOS keywords (`-apple-system`, `BlinkMacSystemFont`) are skipped.
    /// `"Adwaita Sans"` directly follows `"Segoe UI"` because modern fontconfig
    /// setups substitute Segoe UI with GNOME's Adwaita Sans, which is what
    /// Linux browsers actually render for stacks asking for Segoe UI first.
    fn body_families(self) -> Option<&'static [&'static str]> {
        match self {
            FontPreset::Current => None,
            FontPreset::Github => Some(GITHUB_BODY_FAMILIES),
            FontPreset::Vscode => Some(VSCODE_BODY_FAMILIES),
        }
    }

    /// Code families from the emulated viewer's monospace stack, in resolution
    /// order. Empty keeps egui's bundled monospace first.
    fn mono_families(self) -> &'static [&'static str] {
        match self {
            // GitHub puts ui-monospace first; on Linux browsers that resolves
            // through fontconfig's generic monospace alias before any of the
            // named proprietary faces are reached, so the Linux defaults lead.
            FontPreset::Github => GITHUB_MONO_FAMILIES,
            // VS Code webviews always inject editor.fontFamily; its Linux
            // default is 'Droid Sans Mono', monospace.
            FontPreset::Vscode => VSCODE_MONO_FAMILIES,
            FontPreset::Current => &[],
        }
    }
}

/// Discrete base text sizes shared by every font preset. Unlike the presets
/// themselves — which only choose typefaces and line heights — the size class
/// is one app-wide setting with a single shared default, so switching presets
/// never changes the text size on its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TextSizeClass {
    Small,
    #[default]
    Normal,
    Large,
    ExtraLarge,
    Huge,
}

impl TextSizeClass {
    pub(crate) const ALL: [TextSizeClass; 5] = [
        TextSizeClass::Small,
        TextSizeClass::Normal,
        TextSizeClass::Large,
        TextSizeClass::ExtraLarge,
        TextSizeClass::Huge,
    ];

    /// Menu label including the concrete pixel size.
    pub(crate) fn label(self) -> &'static str {
        match self {
            TextSizeClass::Small => "Small (14px)",
            TextSizeClass::Normal => "Normal (16px)",
            TextSizeClass::Large => "Large (18px)",
            TextSizeClass::ExtraLarge => "Extra large (20px)",
            TextSizeClass::Huge => "Huge (24px)",
        }
    }

    /// Base body size in px. Headings and code cascade from this because the
    /// renderer derives every document size from `TextStyle::Body`.
    pub(crate) fn px(self) -> f32 {
        match self {
            TextSizeClass::Small => 14.0,
            TextSizeClass::Normal => 16.0,
            TextSizeClass::Large => 18.0,
            TextSizeClass::ExtraLarge => 20.0,
            TextSizeClass::Huge => 24.0,
        }
    }
}

/// Primer `.markdown-body` body stack without macOS-only aliases; the generic
/// `sans-serif` tail is covered by the app's existing fallback chain.
///
/// GitHub prepends its open-source Mona Sans (primer/primitives#1332); it
/// renders only where installed locally because GitHub ships no markdown
/// webfont. `"Noto Sans Backtick Fix"` is intentionally skipped: it is a
/// `local()`-only `@font-face` shim covering just U+60, not a real family.
const GITHUB_BODY_FAMILIES: &[&str] = &[
    "Mona Sans VF",
    "Mona Sans",
    "Segoe UI",
    "Adwaita Sans", // fontconfig substitutes Segoe UI with Adwaita Sans on modern GNOME
    "Noto Sans",
    "Helvetica",
    "Arial",
];

/// Primer `.markdown-body` code stack. Generic monospace leads because
/// `ui-monospace` precedes every named face in the CSS and resolves on Linux.
const GITHUB_MONO_FAMILIES: &[&str] = &[
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "SFMono-Regular",
    "SF Mono",
    "Menlo",
    "Consolas",
    "Liberation Mono",
];

/// VS Code preview body stack without macOS-only aliases; `system-ui` is
/// transcribed to its common desktop resolutions and `sans-serif` remains
/// covered by the app's existing fallback chain.
const VSCODE_BODY_FAMILIES: &[&str] = &[
    "Segoe WPC",
    "Segoe UI",
    "Adwaita Sans", // system-ui on GNOME 47+
    "Cantarell",    // system-ui on older GNOME releases
    "Ubuntu",
    "Droid Sans",
];

/// VS Code preview code stack: the Linux `editor.fontFamily` default followed
/// by the generic monospace resolutions.
const VSCODE_MONO_FAMILIES: &[&str] = &["Droid Sans Mono", "Noto Sans Mono", "DejaVu Sans Mono"];

/// Apply the shared base text size class to the context styles. Headings and
/// code cascade because the renderer derives document sizes from
/// `TextStyle::Body`.
fn apply_text_size_class(ctx: &egui::Context, size: TextSizeClass) {
    let base = size.px();
    ctx.style_mut(|style| {
        use egui::{FontId, TextStyle};
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(base));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(base * 2.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(base - 2.0));
    });
}

struct ScriptFallback {
    key: &'static str,
    script: [u8; 4],
    required_glyphs: &'static str,
}

// ISO 15924 identifiers and coverage samples describe scripts, not font
// families. The platform backend remains the sole source of family choices.
const SCRIPT_FALLBACKS: &[ScriptFallback] = &[
    ScriptFallback {
        key: "SystemHanFallback",
        script: *b"Hani",
        required_glyphs: "中文测试繁體",
    },
    ScriptFallback {
        key: "SystemHiraganaFallback",
        script: *b"Hira",
        required_glyphs: "かな",
    },
    ScriptFallback {
        key: "SystemKatakanaFallback",
        script: *b"Kana",
        required_glyphs: "カナ",
    },
    ScriptFallback {
        key: "SystemHangulFallback",
        script: *b"Hang",
        required_glyphs: "한글",
    },
    ScriptFallback {
        key: "SystemArabicFallback",
        script: *b"Arab",
        required_glyphs: "اب",
    },
    ScriptFallback {
        key: "SystemHebrewFallback",
        script: *b"Hebr",
        required_glyphs: "אב",
    },
    ScriptFallback {
        key: "SystemDevanagariFallback",
        script: *b"Deva",
        required_glyphs: "नमस्तेहिन्दी",
    },
    ScriptFallback {
        key: "SystemThaiFallback",
        script: *b"Thai",
        required_glyphs: "สวัสดีภาษาไทย",
    },
];

const GENERIC_FALLBACKS: &[(GenericFamily, &str, &str)] = &[
    (GenericFamily::Math, "SystemMathFallback", "→"),
    (GenericFamily::SansSerif, "SystemSymbolFallback", "⚠"),
];

#[derive(Clone)]
struct SelectedFont {
    blob: Blob<u8>,
    index: u32,
    family_id: FamilyId,
    family: String,
    source: String,
    weight: FontWeight,
}

impl SelectedFont {
    fn identity(&self) -> (u64, u32) {
        (self.blob.id(), self.index)
    }

    fn data(&self) -> FontData {
        let mut data = FontData::from_owned(self.blob.data().to_vec());
        data.index = self.index;
        data
    }
}

struct InstalledFont {
    key: String,
    selected: SelectedFont,
    required_glyphs: &'static str,
    primary: bool,
}

fn font_has_glyphs(blob: &Blob<u8>, index: u32, required_glyphs: &str) -> bool {
    ttf_parser::Face::parse(blob.data(), index)
        .map(|face| {
            required_glyphs
                .chars()
                .all(|c| face.glyph_index(c).is_some())
        })
        .unwrap_or(false)
}

fn source_description(source: &SourceKind) -> String {
    match source {
        SourceKind::Memory(_) => "<memory>".to_owned(),
        SourceKind::Path(path) => path.display().to_string(),
    }
}

fn select_from_families(
    collection: &mut Collection,
    source_cache: &mut SourceCache,
    family_ids: &[FamilyId],
    weight: FontWeight,
    required_glyphs: &str,
    require_true_bold: bool,
) -> Option<SelectedFont> {
    for family_id in family_ids {
        let Some(family) = collection.family(*family_id) else {
            continue;
        };
        let mut candidates: Vec<_> = family
            .fonts()
            .iter()
            .filter(|font| {
                font.style() == FontStyle::Normal
                    && (!require_true_bold || font.weight() >= FontWeight::BOLD)
            })
            .collect();
        candidates.sort_by(|left, right| {
            let left_distance = (left.weight().value() - weight.value()).abs();
            let right_distance = (right.weight().value() - weight.value()).abs();
            left_distance.total_cmp(&right_distance)
        });

        for font in candidates {
            let Some(blob) = font.load(Some(source_cache)) else {
                continue;
            };
            if !font_has_glyphs(&blob, font.index(), required_glyphs) {
                continue;
            }
            return Some(SelectedFont {
                blob,
                index: font.index(),
                family_id: *family_id,
                family: family.name().to_owned(),
                source: source_description(font.source().kind()),
                weight: font.weight(),
            });
        }
    }
    None
}

fn fallback_family_ids(
    collection: &mut Collection,
    script: [u8; 4],
    locale: Option<&str>,
) -> Vec<FamilyId> {
    let script = Script::from_bytes(script);
    match locale {
        Some(locale) => collection.fallback_families((script, locale)).collect(),
        None => collection.fallback_families(script).collect(),
    }
}

fn installed_fonts_cover(installed: &[InstalledFont], glyphs: &str) -> bool {
    installed
        .iter()
        .any(|font| font_has_glyphs(&font.selected.blob, font.selected.index, glyphs))
}

fn install_regular_font(
    definitions: &mut FontDefinitions,
    installed: &mut Vec<InstalledFont>,
    loaded_faces: &mut HashSet<(u64, u32)>,
    key: &'static str,
    selected: SelectedFont,
    required_glyphs: &'static str,
    primary: bool,
) {
    if !loaded_faces.insert(selected.identity()) {
        return;
    }
    log::info!(
        "Loaded system font {} ({}, weight {}) from {} (face index {})",
        key,
        selected.family,
        selected.weight.value(),
        selected.source,
        selected.index
    );
    definitions
        .font_data
        .insert(key.to_owned(), selected.data().into());
    if let Some(family) = definitions.families.get_mut(&FontFamily::Proportional) {
        if primary {
            family.insert(0, key.to_owned())
        } else {
            family.push(key.to_owned())
        }
    }
    if let Some(family) = definitions.families.get_mut(&FontFamily::Monospace) {
        family.push(key.to_owned());
    }
    installed.push(InstalledFont {
        key: key.to_owned(),
        selected,
        required_glyphs,
        primary,
    });
}

fn install_regular_fonts(
    collection: &mut Collection,
    source_cache: &mut SourceCache,
    definitions: &mut FontDefinitions,
    locale: Option<&str>,
    preferred_family: Option<&str>,
    preset_body_families: Option<&'static [&'static str]>,
) -> Vec<InstalledFont> {
    let mut installed = Vec::new();
    let mut loaded_faces = HashSet::new();

    if let Some(name) = preferred_family {
        match collection.family_id(name) {
            Some(family_id) => {
                if let Some(selected) = select_from_families(
                    collection,
                    source_cache,
                    &[family_id],
                    FontWeight::NORMAL,
                    "Aa",
                    false,
                ) {
                    install_regular_font(
                        definitions,
                        &mut installed,
                        &mut loaded_faces,
                        "SystemSans",
                        selected,
                        "Aa",
                        true,
                    );
                } else {
                    log::warn!(
                        "Preferred font '{name}' has no usable regular face; falling back to system default."
                    );
                }
            }
            None => {
                log::warn!("Preferred font '{name}' not found; falling back to system default.");
            }
        }
    }

    // A preset's body families lead the primary sans chain when no user-picked
    // family was installed (the picker's choice outranks the preset's emulated
    // stack). Each stack name is tried in the emulated viewer's resolution
    // order; a total miss falls through to the app's own auto-detection.
    if installed.is_empty() {
        if let Some(preset_families) = preset_body_families {
            let family_ids: Vec<FamilyId> = preset_families
                .iter()
                .filter_map(|name| collection.family_id(name))
                .collect();
            if let Some(selected) = select_from_families(
                collection,
                source_cache,
                &family_ids,
                FontWeight::NORMAL,
                "Aa",
                false,
            ) {
                install_regular_font(
                    definitions,
                    &mut installed,
                    &mut loaded_faces,
                    "SystemSans",
                    selected,
                    "Aa",
                    true,
                );
            } else {
                log::warn!(
                    "Preset body families {preset_families:?} had no usable regular face; falling back to system default."
                );
            }
        }
    }

    // Auto-detect the system sans-serif only if no preferred family was
    // requested, or the preferred family couldn't be installed above.
    if installed.is_empty() {
        let families: Vec<_> = collection
            .generic_families(GenericFamily::SansSerif)
            .collect();
        if let Some(selected) = select_from_families(
            collection,
            source_cache,
            &families,
            FontWeight::NORMAL,
            "Aa",
            false,
        ) {
            install_regular_font(
                definitions,
                &mut installed,
                &mut loaded_faces,
                "SystemSans",
                selected,
                "Aa",
                true,
            );
        }
    }

    for spec in SCRIPT_FALLBACKS {
        if installed_fonts_cover(&installed, spec.required_glyphs) {
            continue;
        }
        let families = fallback_family_ids(collection, spec.script, locale);
        let Some(selected) = select_from_families(
            collection,
            source_cache,
            &families,
            FontWeight::NORMAL,
            spec.required_glyphs,
            false,
        ) else {
            continue;
        };
        install_regular_font(
            definitions,
            &mut installed,
            &mut loaded_faces,
            spec.key,
            selected,
            spec.required_glyphs,
            false,
        );
    }

    for &(generic, key, glyphs) in GENERIC_FALLBACKS {
        if installed_fonts_cover(&installed, glyphs) {
            continue;
        }
        let families: Vec<_> = collection.generic_families(generic).collect();
        let Some(selected) = select_from_families(
            collection,
            source_cache,
            &families,
            FontWeight::NORMAL,
            glyphs,
            false,
        ) else {
            continue;
        };
        install_regular_font(
            definitions,
            &mut installed,
            &mut loaded_faces,
            key,
            selected,
            glyphs,
            false,
        );
    }
    installed
}

fn push_unique(
    destination: &mut Vec<String>,
    seen: &mut HashSet<String>,
    names: impl IntoIterator<Item = String>,
) {
    for name in names {
        if seen.insert(name.clone()) {
            destination.push(name)
        }
    }
}

fn build_strong_family(
    primary_regular: Option<&str>,
    primary_bold: Option<&str>,
    default_proportional: &[String],
    fallback_bold: &[String],
    proportional: &[String],
) -> Vec<String> {
    let mut family = Vec::new();
    let mut seen = HashSet::new();
    push_unique(
        &mut family,
        &mut seen,
        primary_bold.into_iter().map(str::to_owned),
    );
    push_unique(
        &mut family,
        &mut seen,
        primary_regular.into_iter().map(str::to_owned),
    );
    push_unique(&mut family, &mut seen, default_proportional.iter().cloned());
    push_unique(&mut family, &mut seen, fallback_bold.iter().cloned());
    push_unique(&mut family, &mut seen, proportional.iter().cloned());
    family
}

fn install_strong_font_family(
    collection: &mut Collection,
    source_cache: &mut SourceCache,
    definitions: &mut FontDefinitions,
    installed: &[InstalledFont],
    default_proportional: &[String],
) -> usize {
    let mut loaded_faces: HashSet<_> = installed
        .iter()
        .map(|font| font.selected.identity())
        .collect();
    let mut primary_regular = None;
    let mut primary_bold = None;
    let mut fallback_bold = Vec::new();
    let mut count = 0;

    for regular in installed {
        if regular.primary {
            primary_regular = Some(regular.key.clone())
        }
        let Some(selected) = select_from_families(
            collection,
            source_cache,
            &[regular.selected.family_id],
            FontWeight::BOLD,
            regular.required_glyphs,
            true,
        ) else {
            continue;
        };
        if !loaded_faces.insert(selected.identity()) {
            continue;
        }
        let key = format!("{}Bold", regular.key);
        log::info!(
            "Loaded strong font {} ({}, weight {}) from {} (face index {})",
            key,
            selected.family,
            selected.weight.value(),
            selected.source,
            selected.index
        );
        definitions
            .font_data
            .insert(key.clone(), selected.data().into());
        count += 1;
        if regular.primary {
            primary_bold = Some(key)
        } else {
            fallback_bold.push(key)
        }
    }

    let proportional = definitions
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let strong = build_strong_family(
        primary_regular.as_deref(),
        primary_bold.as_deref(),
        default_proportional,
        &fallback_bold,
        &proportional,
    );
    if primary_regular.is_some() && primary_bold.is_none() {
        log::warn!("No true bold face found for the primary system sans; using regular fallback.");
    }
    definitions
        .families
        .insert(FontFamily::Name(STRONG_FONT_FAMILY.into()), strong);
    count
}

/// Load fonts through platform generic-family and script/locale fallback rules.
///
/// Fontique delegates to fontconfig on Linux/FreeBSD, DirectWrite on Windows,
/// CoreText on Apple platforms, and the system configuration on Android.
///
/// `preferred_family` optionally names a specific installed family (matched
/// case-insensitively) to use as the primary proportional font instead of the
/// auto-detected system sans-serif; an unset or unresolvable preference falls
/// back to today's auto-detect behavior. Returns the sorted, deduplicated
/// list of installed family names so callers can build a font picker without
/// scanning the font collection a second time.
/// Lead the monospace chain with the preset's code face, keeping egui's
/// bundled monospace (and any installed fallbacks) as fallback. Returns
/// whether a preset face was installed.
fn install_preset_mono_font(
    collection: &mut Collection,
    source_cache: &mut SourceCache,
    definitions: &mut FontDefinitions,
    mono_families: &'static [&'static str],
) -> bool {
    let family_ids: Vec<FamilyId> = mono_families
        .iter()
        .filter_map(|name| collection.family_id(name))
        .collect();
    let Some(selected) = select_from_families(
        collection,
        source_cache,
        &family_ids,
        FontWeight::NORMAL,
        "{}",
        false,
    ) else {
        log::warn!(
            "Preset mono families {mono_families:?} had no usable face; keeping the bundled monospace."
        );
        return false;
    };
    definitions
        .font_data
        .insert("PresetMono".to_owned(), selected.data().into());
    if let Some(family) = definitions.families.get_mut(&FontFamily::Monospace) {
        family.insert(0, "PresetMono".to_owned());
    }
    true
}

pub(crate) fn setup_fonts(
    ctx: &egui::Context,
    preferred_family: Option<&str>,
    preset: FontPreset,
    size: TextSizeClass,
) -> Vec<String> {
    let started = std::time::Instant::now();
    let mut collection = Collection::new(CollectionOptions::default());
    let mut family_names: Vec<String> = collection.family_names().map(str::to_owned).collect();
    family_names.sort_by_key(|name| name.to_lowercase());
    family_names.dedup();
    let family_count = family_names.len();
    let mut source_cache = SourceCache::default();
    let locale = sys_locale::get_locale();
    let mut definitions = FontDefinitions::default();
    let defaults = definitions
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let installed = install_regular_fonts(
        &mut collection,
        &mut source_cache,
        &mut definitions,
        locale.as_deref(),
        preferred_family,
        preset.body_families(),
    );
    let bold_count = install_strong_font_family(
        &mut collection,
        &mut source_cache,
        &mut definitions,
        &installed,
        &defaults,
    );
    let preset_mono_families = preset.mono_families();
    let preset_mono = if preset_mono_families.is_empty() {
        false
    } else {
        install_preset_mono_font(
            &mut collection,
            &mut source_cache,
            &mut definitions,
            preset_mono_families,
        )
    };
    if installed.is_empty() {
        log::warn!("No suitable system font fallbacks found; using egui defaults.");
    } else {
        log::info!(
            "Selected {} font faces from {} families for locale {:?} in {:.1} ms",
            installed.len() + bold_count + usize::from(preset_mono),
            family_count,
            locale,
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    ctx.set_fonts(definitions);
    apply_text_size_class(ctx, size);
    family_names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_scripts_use_full_coverage_samples() {
        let sample = |script| {
            SCRIPT_FALLBACKS
                .iter()
                .find(|fallback| fallback.script == script)
                .unwrap()
                .required_glyphs
        };
        assert_eq!(sample(*b"Deva"), "नमस्तेहिन्दी");
        assert_eq!(sample(*b"Thai"), "สวัสดีภาษาไทย");
        assert!(sample(*b"Hani").contains('测'));
        assert!(sample(*b"Hani").contains('體'));
    }

    #[test]
    fn default_latin_precedes_script_bold_without_primary_system_sans() {
        let strong = build_strong_family(
            None,
            None,
            &["DefaultLatin".into()],
            &["HanBold".into()],
            &["DefaultLatin".into(), "Han".into()],
        );
        assert_eq!(strong, ["DefaultLatin", "HanBold", "Han"]);
    }

    #[test]
    fn primary_regular_and_bold_stay_together() {
        let strong = build_strong_family(
            Some("SystemSans"),
            Some("SystemSansBold"),
            &["DefaultLatin".into()],
            &["HanBold".into()],
            &["SystemSans".into(), "Han".into()],
        );
        assert_eq!(
            strong,
            [
                "SystemSansBold",
                "SystemSans",
                "DefaultLatin",
                "HanBold",
                "Han"
            ]
        );
    }

    #[test]
    #[ignore = "requires installed system fonts"]
    fn unknown_preferred_family_falls_back_to_auto_detect() {
        let mut collection = Collection::new(CollectionOptions::default());
        let mut source_cache = SourceCache::default();
        let mut definitions = FontDefinitions::default();
        let installed = install_regular_fonts(
            &mut collection,
            &mut source_cache,
            &mut definitions,
            None,
            Some("Definitely Not An Installed Font Name 12345"),
            None,
        );
        assert!(
            installed.iter().any(|f| f.primary),
            "auto-detect fallback should still install a primary font"
        );
    }

    #[test]
    #[ignore = "requires installed system fonts"]
    fn known_preferred_family_becomes_primary() {
        let mut collection = Collection::new(CollectionOptions::default());
        let mut source_cache = SourceCache::default();

        // Discover whatever the system's default sans-serif family is, then
        // request it explicitly by name and confirm it round-trips as
        // primary. Avoids hardcoding a font name that may not exist on every
        // machine/CI image.
        let mut baseline_definitions = FontDefinitions::default();
        let baseline = install_regular_fonts(
            &mut collection,
            &mut source_cache,
            &mut baseline_definitions,
            None,
            None,
            None,
        );
        let Some(baseline_primary) = baseline.iter().find(|f| f.primary) else {
            return; // no system fonts available in this environment
        };
        let family_name = baseline_primary.selected.family.clone();

        let mut definitions = FontDefinitions::default();
        let installed = install_regular_fonts(
            &mut collection,
            &mut source_cache,
            &mut definitions,
            None,
            Some(&family_name),
            None,
        );
        let primary = installed
            .iter()
            .find(|f| f.primary)
            .expect("primary font installed");
        assert_eq!(primary.selected.family, family_name);
    }

    #[test]
    #[ignore = "requires installed multilingual regular and bold fonts"]
    fn installed_fonts_cover_reported_scripts() {
        let context = egui::Context::default();
        setup_fonts(
            &context,
            None,
            FontPreset::default(),
            TextSizeClass::default(),
        );
        context.begin_pass(Default::default());
        let regular = egui::FontId::proportional(16.0);
        let strong = egui::FontId::new(16.0, FontFamily::Name(STRONG_FONT_FAMILY.into()));
        let samples = [
            "中文测试繁體",
            "かなカナ",
            "한글",
            "नमस्तेहिन्दी",
            "สวัสดีภาษาไทย",
        ];
        let missing_regular: Vec<_> = samples
            .iter()
            .filter(|sample| !context.fonts_mut(|fonts| fonts.has_glyphs(&regular, sample)))
            .collect();
        let missing_strong: Vec<_> = samples
            .iter()
            .filter(|sample| !context.fonts_mut(|fonts| fonts.has_glyphs(&strong, sample)))
            .collect();
        let _ = context.end_pass();
        assert!(
            missing_regular.is_empty(),
            "regular chain lacks {missing_regular:?}"
        );
        assert!(
            missing_strong.is_empty(),
            "strong chain lacks {missing_strong:?}"
        );
    }

    #[test]
    fn preset_family_lists_follow_the_emulated_css_stacks() {
        // GitHub (Primer .markdown-body) leads with its locally-installed Mona
        // Sans, asks for Segoe UI before Noto Sans, and ends the named part of
        // its body stack at Arial.
        let github_body = FontPreset::Github
            .body_families()
            .expect("github body list");
        assert_eq!(github_body[..3], ["Mona Sans VF", "Mona Sans", "Segoe UI"]);
        assert!(github_body.contains(&"Noto Sans"));
        assert_eq!(*github_body.last().expect("non-empty"), "Arial");

        // VS Code preview lists Ubuntu and Droid Sans behind the system-ui
        // resolutions.
        let vscode_body = FontPreset::Vscode
            .body_families()
            .expect("vscode body list");
        let ubuntu_pos = vscode_body
            .iter()
            .position(|f| *f == "Ubuntu")
            .expect("ubuntu in vscode stack");
        let adwaita_pos = vscode_body
            .iter()
            .position(|f| *f == "Adwaita Sans")
            .expect("adwaita sans in vscode stack");
        assert!(adwaita_pos < ubuntu_pos, "system-ui precedes Ubuntu");

        // The default preset keeps the app's own sans selection.
        assert_eq!(FontPreset::Current.body_families(), None);
        assert!(FontPreset::Current.mono_families().is_empty());
    }

    #[test]
    fn preset_mono_lists_lead_with_linux_generic_monospace_resolutions() {
        assert_eq!(
            FontPreset::Github.mono_families()[..2],
            ["Noto Sans Mono", "DejaVu Sans Mono"]
        );
        assert_eq!(
            FontPreset::Vscode.mono_families(),
            ["Droid Sans Mono", "Noto Sans Mono", "DejaVu Sans Mono"]
        );
    }

    #[test]
    #[ignore = "requires installed system fonts"]
    fn github_preset_resolves_its_body_stack_when_fonts_exist() {
        let mut collection = Collection::new(CollectionOptions::default());
        let mut source_cache = SourceCache::default();
        let mut definitions = FontDefinitions::default();
        let installed = install_regular_fonts(
            &mut collection,
            &mut source_cache,
            &mut definitions,
            None,
            None,
            FontPreset::Github.body_families(),
        );
        let Some(primary) = installed.iter().find(|f| f.primary) else {
            return; // no system fonts available in this environment
        };
        let stack = FontPreset::Github
            .body_families()
            .expect("github body list");
        assert!(
            stack.contains(&primary.selected.family.as_str()),
            "preset lead {:?} must come from the emulated stack",
            primary.selected.family
        );
    }
}
