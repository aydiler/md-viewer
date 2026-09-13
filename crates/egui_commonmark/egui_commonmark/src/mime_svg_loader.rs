//! SVG loader fallback for resources whose URL does not end in `.svg`.
//!
//! egui_extras 0.33 documents MIME-based SVG detection, but its built-in
//! loader currently accepts only URIs ending in `.svg`. Dynamic image URLs
//! commonly omit that suffix while returning `image/svg+xml` correctly.

use std::{
    collections::HashMap,
    mem::size_of,
    sync::{Arc, OnceLock},
};

use egui::{
    load::{BytesPoll, ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint},
    mutex::Mutex,
    ColorImage,
};
#[cfg(feature = "svg_text")]
use resvg::usvg::fontdb::{Database, Family};

struct MimeSvgLoader {
    cache: Mutex<SvgCache>,
    options: OnceLock<resvg::usvg::Options<'static>>,
}

struct SvgCacheEntry {
    result: Result<Arc<ColorImage>, String>,
    last_used: u64,
}

#[derive(Default)]
struct SvgCache {
    entries: HashMap<(String, SizeHint), SvgCacheEntry>,
    use_tick: u64,
}

const MAX_SVG_CACHE_BYTES: usize = 64 * 1024 * 1024;

fn svg_result_bytes(result: &Result<Arc<ColorImage>, String>) -> usize {
    match result {
        Ok(image) => image.pixels.len() * size_of::<egui::Color32>(),
        Err(error) => error.len(),
    }
}

fn trim_svg_cache(cache: &mut SvgCache, max_bytes: usize) {
    let mut total_bytes = cache
        .entries
        .values()
        .map(|entry| svg_result_bytes(&entry.result))
        .sum::<usize>();
    if total_bytes <= max_bytes {
        return;
    }

    // Build the eviction order once. Recomputing the total and rescanning the
    // full map for every victim made a large trim quadratic in the entry count.
    let mut victims: Vec<_> = cache
        .entries
        .iter()
        .map(|(key, entry)| {
            (
                key.clone(),
                entry.last_used,
                svg_result_bytes(&entry.result),
            )
        })
        .collect();
    victims.sort_unstable_by_key(|(_, last_used, _)| *last_used);

    for (key, _, bytes) in victims {
        if total_bytes <= max_bytes {
            break;
        }
        if cache.entries.remove(&key).is_some() {
            total_bytes = total_bytes.saturating_sub(bytes);
        }
    }
}

impl MimeSvgLoader {
    const ID: &'static str = egui::generate_loader_id!(MimeSvgLoader);
}

impl Default for MimeSvgLoader {
    fn default() -> Self {
        Self {
            cache: Mutex::default(),
            options: OnceLock::new(),
        }
    }
}

fn svg_options() -> resvg::usvg::Options<'static> {
    let options = resvg::usvg::Options::default();
    #[cfg(feature = "svg_text")]
    let options = {
        let mut options = options;
        options.fontdb_mut().load_system_fonts();
        repair_sans_serif_family(options.fontdb_mut());
        options
    };
    options
}

/// fontdb may map the generic sans-serif family to a font that is not installed.
/// Preserve valid mappings and otherwise ask Fontconfig for the concrete system
/// sans-serif family used by SVG badges.
#[cfg(feature = "svg_text")]
fn repair_sans_serif_family(database: &mut Database) {
    if family_is_loaded(database, database.family_name(&Family::SansSerif)) {
        return;
    }

    if let Some(family) =
        fontconfig_sans_serif_family().filter(|family| family_is_loaded(database, family))
    {
        database.set_sans_serif_family(family);
    }
}

#[cfg(feature = "svg_text")]
fn family_is_loaded(database: &Database, requested: &str) -> bool {
    database.faces().any(|face| {
        face.families
            .iter()
            // Match fontdb's family-name query semantics exactly. Accepting a
            // differently-cased spelling here would still leave it unresolved
            // when usvg later queries the database.
            .any(|(family, _)| family == requested)
    })
}

#[cfg(feature = "svg_text")]
fn fontconfig_sans_serif_family() -> Option<String> {
    #[cfg(all(unix, not(any(target_vendor = "apple", target_os = "android"))))]
    {
        use std::process::{Command, Stdio};

        let output = Command::new("fc-match")
            .arg("--format=%{family[0]}\\n")
            .arg("sans-serif")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if output.status.success() {
            return parse_fontconfig_family(&output.stdout);
        }
    }

    None
}

#[cfg(all(
    feature = "svg_text",
    unix,
    not(any(target_vendor = "apple", target_os = "android"))
))]
fn parse_fontconfig_family(stdout: &[u8]) -> Option<String> {
    let family = String::from_utf8_lossy(stdout);
    let family = family.trim();
    (!family.is_empty()).then(|| family.to_owned())
}

impl ImageLoader for MimeSvgLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, uri: &str, size_hint: SizeHint) -> ImageLoadResult {
        // Let egui_extras' built-in loader handle ordinary `.svg` URIs. This
        // fallback is deliberately limited to the MIME-detection gap.
        if uri.ends_with(".svg") {
            return Err(LoadError::NotSupported);
        }

        let key = (uri.to_owned(), size_hint);
        let cached = {
            let mut cache = self.cache.lock();
            cache.use_tick = cache.use_tick.wrapping_add(1);
            let tick = cache.use_tick;
            cache.entries.get_mut(&key).map(|entry| {
                entry.last_used = tick;
                entry.result.clone()
            })
        };
        if let Some(result) = cached {
            return result
                .map(|image| ImagePoll::Ready { image })
                .map_err(LoadError::Loading);
        }

        match ctx.try_load_bytes(uri)? {
            BytesPoll::Pending { size } => Ok(ImagePoll::Pending { size }),
            BytesPoll::Ready { bytes, mime, .. } if mime.as_deref().is_some_and(is_svg_mime) => {
                let options = self.options.get_or_init(svg_options);
                let result =
                    egui_extras::image::load_svg_bytes_with_size(&bytes, size_hint, options)
                        .map(Arc::new);
                if svg_result_bytes(&result) <= MAX_SVG_CACHE_BYTES {
                    let mut cache = self.cache.lock();
                    cache.use_tick = cache.use_tick.wrapping_add(1);
                    let tick = cache.use_tick;
                    cache.entries.insert(
                        key,
                        SvgCacheEntry {
                            result: result.clone(),
                            last_used: tick,
                        },
                    );
                    trim_svg_cache(&mut cache, MAX_SVG_CACHE_BYTES);
                }
                result
                    .map(|image| ImagePoll::Ready { image })
                    .map_err(LoadError::Loading)
            }
            BytesPoll::Ready { .. } => Err(LoadError::NotSupported),
        }
    }

    fn forget(&self, uri: &str) {
        self.cache
            .lock()
            .entries
            .retain(|(cached_uri, _), _| cached_uri != uri);
    }

    fn forget_all(&self) {
        self.cache.lock().entries.clear();
    }

    fn byte_size(&self) -> usize {
        self.cache
            .lock()
            .entries
            .values()
            .map(|entry| svg_result_bytes(&entry.result))
            .sum()
    }
}

fn is_svg_mime(mime: &str) -> bool {
    mime.split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("image/svg+xml"))
}

pub(crate) fn install(ctx: &egui::Context) {
    if !ctx.is_loader_installed(MimeSvgLoader::ID) {
        // Image loaders are tried newest-first, so install this after the
        // standard egui_extras loaders initialized by prepare_show().
        ctx.add_image_loader(Arc::new(MimeSvgLoader::default()));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{
        load::{ImageLoader, SizeHint},
        Color32, ColorImage,
    };

    #[cfg(all(
        feature = "svg_text",
        unix,
        not(any(target_vendor = "apple", target_os = "android"))
    ))]
    use super::parse_fontconfig_family;
    use super::{is_svg_mime, trim_svg_cache, MimeSvgLoader, SvgCacheEntry};

    #[test]
    fn recognizes_svg_mime_with_optional_parameters() {
        assert!(is_svg_mime("image/svg+xml"));
        assert!(is_svg_mime("image/svg+xml;charset=utf-8"));
        assert!(is_svg_mime("IMAGE/SVG+XML; charset=UTF-8"));
        assert!(!is_svg_mime("image/png"));
    }

    #[cfg(all(
        feature = "svg_text",
        unix,
        not(any(target_vendor = "apple", target_os = "android"))
    ))]
    #[test]
    fn parses_concrete_fontconfig_family() {
        assert_eq!(
            parse_fontconfig_family(b"DejaVu Sans\n"),
            Some("DejaVu Sans".to_string())
        );
        assert_eq!(parse_fontconfig_family(b"\n"), None);
        assert_eq!(parse_fontconfig_family(b""), None);
    }

    #[test]
    fn forget_removes_every_cached_size_for_uri() {
        let loader = MimeSvgLoader {
            cache: Default::default(),
            options: resvg::usvg::Options::default().into(),
        };
        let image = Arc::new(ColorImage::filled([1, 1], Color32::WHITE));

        loader.cache.lock().entries.insert(
            ("https://example.com/badge".to_owned(), SizeHint::Width(10)),
            SvgCacheEntry {
                result: Ok(image.clone()),
                last_used: 1,
            },
        );
        loader.cache.lock().entries.insert(
            ("https://example.com/badge".to_owned(), SizeHint::Width(20)),
            SvgCacheEntry {
                result: Ok(image.clone()),
                last_used: 2,
            },
        );
        loader.cache.lock().entries.insert(
            ("https://example.com/other".to_owned(), SizeHint::Width(10)),
            SvgCacheEntry {
                result: Ok(image),
                last_used: 3,
            },
        );

        loader.forget("https://example.com/badge");

        let cache = loader.cache.lock();
        assert_eq!(cache.entries.len(), 1);
        assert!(cache
            .entries
            .keys()
            .all(|(uri, _)| uri == "https://example.com/other"));
    }

    #[test]
    fn svg_options_are_initialized_lazily() {
        let loader = MimeSvgLoader::default();

        assert!(loader.options.get().is_none());
    }

    #[test]
    fn cache_limit_evicts_least_recently_used_entries() {
        let loader = MimeSvgLoader::default();
        let image = Arc::new(ColorImage::filled([2, 1], Color32::WHITE));
        let mut cache = loader.cache.lock();
        for (uri, last_used) in [("old", 1), ("recent", 3), ("middle", 2)] {
            cache.entries.insert(
                (uri.to_owned(), SizeHint::Width(10)),
                SvgCacheEntry {
                    result: Ok(image.clone()),
                    last_used,
                },
            );
        }

        trim_svg_cache(&mut cache, 16);

        assert_eq!(cache.entries.len(), 2);
        assert!(!cache
            .entries
            .contains_key(&("old".to_owned(), SizeHint::Width(10))));
    }

    #[test]
    fn cache_limit_evicts_multiple_entries_in_lru_order() {
        let loader = MimeSvgLoader::default();
        let image = Arc::new(ColorImage::filled([2, 1], Color32::WHITE));
        let mut cache = loader.cache.lock();
        for (uri, last_used) in [("oldest", 1), ("old", 2), ("recent", 3), ("newest", 4)] {
            cache.entries.insert(
                (uri.to_owned(), SizeHint::Width(10)),
                SvgCacheEntry {
                    result: Ok(image.clone()),
                    last_used,
                },
            );
        }

        trim_svg_cache(&mut cache, 16);

        assert_eq!(cache.entries.len(), 2);
        assert!(cache
            .entries
            .contains_key(&("recent".to_owned(), SizeHint::Width(10))));
        assert!(cache
            .entries
            .contains_key(&("newest".to_owned(), SizeHint::Width(10))));
    }
}
