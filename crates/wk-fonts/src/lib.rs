//! Font discovery for the engine. Stands in for fontconfig as used by Qt 4.8
//! (`QFontDatabase`) in upstream wkhtmltopdf: there is no fontconfig here, fonts
//! are found by scanning the usual directories, and a fallback set is embedded
//! so the binary renders on a bare container.
//!
//! Family resolution mirrors the reference environment the fixtures were
//! captured in (`wkref:local`: Ubuntu 20.04 with `fonts-dejavu-core` only):
//! - a family that exists is used as is (`DejaVu Serif`, `DejaVu Sans Mono`,
//!   or any scanned system family);
//! - every generic family (`serif`, `sans-serif`, `monospace`, …) and every
//!   unknown family resolves to **DejaVu Sans**, as QtWebKit + fontconfig do
//!   in that image (verified: `monospace`, `serif`, `Arial`, `Courier` and
//!   `<pre>` all print with DejaVuSans);
//! - italics are synthesised (the image has no oblique faces), so the
//!   embedded set is exactly the six faces of `fonts-dejavu-core`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parley::FontContext;
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};

/// The embedded fallback faces (`fonts-dejavu-core` 2.37, Bitstream Vera
/// licence, see `fonts/LICENSE`).
pub const EMBEDDED_FONTS: &[(&str, &[u8])] = &[
    ("DejaVuSans.ttf", include_bytes!("../fonts/DejaVuSans.ttf")),
    ("DejaVuSans-Bold.ttf", include_bytes!("../fonts/DejaVuSans-Bold.ttf")),
    ("DejaVuSerif.ttf", include_bytes!("../fonts/DejaVuSerif.ttf")),
    ("DejaVuSerif-Bold.ttf", include_bytes!("../fonts/DejaVuSerif-Bold.ttf")),
    ("DejaVuSansMono.ttf", include_bytes!("../fonts/DejaVuSansMono.ttf")),
    ("DejaVuSansMono-Bold.ttf", include_bytes!("../fonts/DejaVuSansMono-Bold.ttf")),
];

/// Family every generic and unknown family resolves to.
pub const DEFAULT_FAMILY: &str = "DejaVu Sans";

/// Environment variable disabling the system font scan when set to `0`
/// (tests use it so output does not depend on the host's fonts).
pub const SYSTEM_FONTS_ENV: &str = "WKHTMLTOPDF_SYSTEM_FONTS";

/// Where to look for fonts.
#[derive(Debug, Clone)]
pub struct FontOptions {
    /// Scan the platform font directories.
    pub system_fonts: bool,
    /// Additional directories, scanned recursively.
    pub extra_dirs: Vec<PathBuf>,
}

impl Default for FontOptions {
    fn default() -> Self {
        let system_fonts = std::env::var(SYSTEM_FONTS_ENV).map(|v| v != "0").unwrap_or(true);
        FontOptions { system_fonts, extra_dirs: Vec::new() }
    }
}

/// The platform font directories, like fontconfig's default `<dir>` list.
pub fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        dirs.push("/System/Library/Fonts".into());
        dirs.push("/Library/Fonts".into());
        if let Some(h) = &home {
            dirs.push(h.join("Library/Fonts"));
        }
    } else {
        dirs.push("/usr/share/fonts".into());
        dirs.push("/usr/local/share/fonts".into());
        if let Some(h) = &home {
            dirs.push(h.join(".fonts"));
            dirs.push(h.join(".local/share/fonts"));
        }
    }
    dirs
}

fn is_font_file(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
        Some("ttf" | "otf" | "ttc" | "otc")
    )
}

fn scan_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            scan_dir(&p, depth + 1, out);
        } else if is_font_file(&p) {
            out.push(p);
        }
    }
}

fn map_file(p: &Path) -> Option<Blob<u8>> {
    let f = std::fs::File::open(p).ok()?;
    // SAFETY: font files are treated as read-only for the lifetime of the
    // process, like fontconfig/FreeType do; a concurrent truncation would be
    // a misconfiguration of the host.
    let map = unsafe { memmap2::Mmap::map(&f) }.ok()?;
    Some(Blob::new(Arc::new(map)))
}

/// Build the font collection: embedded set, then the scanned directories,
/// then the generic-family mapping.
pub fn collection(opts: &FontOptions) -> Collection {
    let mut collection = Collection::new(CollectionOptions { system_fonts: false, shared: false });
    for (_, data) in EMBEDDED_FONTS {
        collection.register_fonts(Blob::new(Arc::new(*data)), None);
    }
    let mut files = Vec::new();
    if opts.system_fonts {
        for d in system_font_dirs() {
            scan_dir(&d, 0, &mut files);
        }
    }
    for d in &opts.extra_dirs {
        scan_dir(d, 0, &mut files);
    }
    for f in files {
        if let Some(blob) = map_file(&f) {
            collection.register_fonts(blob, None);
        }
    }
    if let Some(sans) = collection.family_id(DEFAULT_FAMILY) {
        for g in [
            GenericFamily::Serif,
            GenericFamily::SansSerif,
            GenericFamily::Monospace,
            GenericFamily::Cursive,
            GenericFamily::Fantasy,
            GenericFamily::SystemUi,
            GenericFamily::UiSerif,
            GenericFamily::UiSansSerif,
            GenericFamily::UiMonospace,
            GenericFamily::UiRounded,
            GenericFamily::Emoji,
            GenericFamily::Math,
            GenericFamily::FangSong,
        ] {
            collection.set_generic_families(g, [sans].into_iter());
        }
    }
    collection
}

/// A parley font context over [`collection`].
pub fn font_context(opts: &FontOptions) -> FontContext {
    FontContext { collection: collection(opts), source_cache: SourceCache::new(Default::default()) }
}

/// The regular face `QFont(family)` resolves to, as font bytes and face
/// index: the family when it exists, otherwise [`DEFAULT_FAMILY`] (the
/// reference image's fontconfig substitution, e.g. for `Arial`).
pub fn regular_face(ctx: &mut FontContext, family: &str) -> Option<(Arc<Vec<u8>>, u32)> {
    use parley::fontique::{FontStyle, FontWeight, FontWidth};
    let id = ctx.collection.family_id(family).or_else(|| ctx.collection.family_id(DEFAULT_FAMILY))?;
    let fam = ctx.collection.family(id)?;
    let font = fam.match_font(FontWidth::NORMAL, FontStyle::Normal, FontWeight::NORMAL, true)?;
    let blob = font.load(Some(&mut ctx.source_cache))?;
    Some((Arc::new(blob.data().to_vec()), font.index()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_families_and_generics() {
        let mut c = collection(&FontOptions { system_fonts: false, extra_dirs: vec![] });
        for f in ["DejaVu Sans", "DejaVu Serif", "DejaVu Sans Mono"] {
            assert!(c.family_id(f).is_some(), "{f}");
        }
        let sans = c.family_id(DEFAULT_FAMILY).unwrap();
        let mono: Vec<_> = c.generic_families(GenericFamily::Monospace).collect();
        assert_eq!(mono, vec![sans], "monospace maps to DejaVu Sans like the reference image");
    }
}
