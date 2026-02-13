//! Sidecar subtitle file discovery.
//!
//! Naming conventions:
//! - `Movie.en.srt`          → language "en"
//! - `Movie.en.forced.srt`   → language "en", forced
//! - `Movie.srt`             → unknown language
//! - `Movie.en.hi.srt`       → language "en", hearing impaired
//!
//! Supported extensions: .srt, .sub, .ass, .ssa, .vtt, .sup, .idx

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A discovered sidecar subtitle file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarSubtitle {
    pub path: PathBuf,
    pub format: SubtitleFormat,
    pub language: Option<String>,
    pub forced: bool,
    pub sdh: bool, // hearing impaired / SDH
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtitleFormat {
    Srt,
    Sub,
    Ass,
    Ssa,
    Vtt,
    Sup, // PGS bitmap
    Idx, // VobSub index
}

impl SubtitleFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "srt" => Some(Self::Srt),
            "sub" => Some(Self::Sub),
            "ass" => Some(Self::Ass),
            "ssa" => Some(Self::Ssa),
            "vtt" => Some(Self::Vtt),
            "sup" => Some(Self::Sup),
            "idx" => Some(Self::Idx),
            _ => None,
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Srt => "application/x-subrip",
            Self::Sub => "text/plain",
            Self::Ass | Self::Ssa => "text/x-ssa",
            Self::Vtt => "text/vtt",
            Self::Sup => "application/octet-stream",
            Self::Idx => "text/plain",
        }
    }
}

/// ISO 639-1 two-letter language codes (common subset for validation).
const LANG_CODES: &[&str] = &[
    "aa", "ab", "af", "ak", "am", "an", "ar", "as", "av", "ay", "az", "ba", "be", "bg", "bh",
    "bi", "bm", "bn", "bo", "br", "bs", "ca", "ce", "ch", "co", "cr", "cs", "cu", "cv", "cy",
    "da", "de", "dv", "dz", "ee", "el", "en", "eo", "es", "et", "eu", "fa", "ff", "fi", "fj",
    "fo", "fr", "fy", "ga", "gd", "gl", "gn", "gu", "gv", "ha", "he", "hi", "ho", "hr", "ht",
    "hu", "hy", "hz", "ia", "id", "ie", "ig", "ii", "ik", "in", "io", "is", "it", "iu", "ja",
    "jv", "ka", "kg", "ki", "kj", "kk", "kl", "km", "kn", "ko", "kr", "ks", "ku", "kv", "kw",
    "ky", "la", "lb", "lg", "li", "ln", "lo", "lt", "lu", "lv", "mg", "mh", "mi", "mk", "ml",
    "mn", "mr", "ms", "mt", "my", "na", "nb", "nd", "ne", "ng", "nl", "nn", "no", "nr", "nv",
    "ny", "oc", "oj", "om", "or", "os", "pa", "pi", "pl", "ps", "pt", "qu", "rm", "rn", "ro",
    "ru", "rw", "sa", "sc", "sd", "se", "sg", "si", "sk", "sl", "sm", "sn", "so", "sq", "sr",
    "ss", "st", "su", "sv", "sw", "ta", "te", "tg", "th", "ti", "tk", "tl", "tn", "to", "tr",
    "ts", "tt", "tw", "ty", "ug", "uk", "ur", "uz", "ve", "vi", "vo", "wa", "wo", "xh", "yi",
    "yo", "za", "zh", "zu",
];

fn is_lang_code(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    LANG_CODES.contains(&lower.as_str())
        || lower.len() == 3 // also accept ISO 639-2 (3-letter)
}

/// Parse subtitle metadata from filename parts.
///
/// Given a media file "Movie.Title.2020.mkv", subtitle files like
/// "Movie.Title.2020.en.forced.srt" are parsed by examining the
/// extra segments between the media stem and the subtitle extension.
fn parse_sub_markers(media_stem: &str, sub_stem: &str) -> (Option<String>, bool, bool) {
    // Remove the media stem prefix to get the extra parts
    let extra = if sub_stem.len() > media_stem.len() {
        &sub_stem[media_stem.len()..]
    } else {
        return (None, false, false);
    };

    // Split on dots, ignoring empty
    let parts: Vec<&str> = extra.split('.').filter(|s| !s.is_empty()).collect();

    let mut language = None;
    let mut forced = false;
    let mut sdh = false;

    for part in &parts {
        let lower = part.to_ascii_lowercase();
        if lower == "forced" {
            forced = true;
        } else if lower == "sdh" || lower == "hi" || lower == "cc" {
            sdh = true;
        } else if is_lang_code(part) && language.is_none() {
            language = Some(lower);
        }
    }

    (language, forced, sdh)
}

/// Discover sidecar subtitle files for a given media file.
pub fn discover_sidecars(media_path: &Path) -> Vec<SidecarSubtitle> {
    let parent = match media_path.parent() {
        Some(p) => p,
        None => return Vec::new(),
    };

    let media_stem = match media_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    let entries = match std::fs::read_dir(parent) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_string(),
            None => continue,
        };

        let format = match SubtitleFormat::from_extension(&ext) {
            Some(f) => f,
            None => continue,
        };

        let sub_stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Subtitle file must start with the media file stem
        if !sub_stem.starts_with(&media_stem) {
            continue;
        }

        let (language, forced, sdh) = parse_sub_markers(&media_stem, &sub_stem);

        let title = build_title(&language, forced, sdh);

        results.push(SidecarSubtitle {
            path,
            format,
            language,
            forced,
            sdh,
            title: Some(title),
        });
    }

    results.sort_by(|a, b| a.path.cmp(&b.path));
    results
}

fn build_title(language: &Option<String>, forced: bool, sdh: bool) -> String {
    let mut parts = Vec::new();
    if let Some(lang) = language {
        parts.push(lang.to_uppercase());
    } else {
        parts.push("Unknown".into());
    }
    if forced {
        parts.push("Forced".into());
    }
    if sdh {
        parts.push("SDH".into());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn subtitle_format_detection() {
        assert_eq!(SubtitleFormat::from_extension("srt"), Some(SubtitleFormat::Srt));
        assert_eq!(SubtitleFormat::from_extension("SRT"), Some(SubtitleFormat::Srt));
        assert_eq!(SubtitleFormat::from_extension("ass"), Some(SubtitleFormat::Ass));
        assert_eq!(SubtitleFormat::from_extension("vtt"), Some(SubtitleFormat::Vtt));
        assert_eq!(SubtitleFormat::from_extension("sup"), Some(SubtitleFormat::Sup));
        assert_eq!(SubtitleFormat::from_extension("mp4"), None);
    }

    #[test]
    fn parse_markers_english() {
        let (lang, forced, sdh) = parse_sub_markers("Movie.2020", "Movie.2020.en");
        assert_eq!(lang.as_deref(), Some("en"));
        assert!(!forced);
        assert!(!sdh);
    }

    #[test]
    fn parse_markers_forced() {
        let (lang, forced, sdh) = parse_sub_markers("Movie.2020", "Movie.2020.en.forced");
        assert_eq!(lang.as_deref(), Some("en"));
        assert!(forced);
        assert!(!sdh);
    }

    #[test]
    fn parse_markers_sdh() {
        let (lang, forced, sdh) = parse_sub_markers("Movie.2020", "Movie.2020.en.sdh");
        assert_eq!(lang.as_deref(), Some("en"));
        assert!(!forced);
        assert!(sdh);
    }

    #[test]
    fn parse_markers_no_lang() {
        let (lang, forced, sdh) = parse_sub_markers("Movie.2020", "Movie.2020");
        assert!(lang.is_none());
        assert!(!forced);
        assert!(!sdh);
    }

    #[test]
    fn discover_sidecars_finds_subtitles() {
        let tmp = std::env::temp_dir().join(format!("rf_sub_test_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();

        let media = tmp.join("Movie.Title.2020.mkv");
        fs::write(&media, "fake video").unwrap();

        // Create subtitle files
        fs::write(tmp.join("Movie.Title.2020.en.srt"), "1\n00:00:01,000 --> 00:00:02,000\nHello").unwrap();
        fs::write(tmp.join("Movie.Title.2020.fr.forced.srt"), "1\n00:00:01,000 --> 00:00:02,000\nBonjour").unwrap();
        fs::write(tmp.join("Movie.Title.2020.srt"), "no lang").unwrap();
        fs::write(tmp.join("Movie.Title.2020.en.sdh.ass"), "sdh subs").unwrap();
        // Unrelated file
        fs::write(tmp.join("OtherMovie.en.srt"), "not ours").unwrap();

        let subs = discover_sidecars(&media);
        assert_eq!(subs.len(), 4);

        // Check the English SRT
        let en_srt = subs.iter().find(|s| s.language.as_deref() == Some("en") && s.format == SubtitleFormat::Srt).unwrap();
        assert!(!en_srt.forced);
        assert!(!en_srt.sdh);

        // Check French forced
        let fr_forced = subs.iter().find(|s| s.language.as_deref() == Some("fr")).unwrap();
        assert!(fr_forced.forced);

        // Check no-language SRT
        let no_lang = subs.iter().find(|s| s.language.is_none() && s.format == SubtitleFormat::Srt).unwrap();
        assert!(!no_lang.forced);

        // Check SDH ASS
        let sdh_ass = subs.iter().find(|s| s.sdh && s.format == SubtitleFormat::Ass).unwrap();
        assert_eq!(sdh_ass.language.as_deref(), Some("en"));

        fs::remove_dir_all(&tmp).ok();
    }
}
