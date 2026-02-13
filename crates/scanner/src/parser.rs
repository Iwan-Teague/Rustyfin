use regex::Regex;
use std::sync::LazyLock;

/// Parsed movie info from a filename/folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieInfo {
    pub title: String,
    pub year: Option<u16>,
}

/// Parsed episode info from a filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeInfo {
    pub series_title: String,
    pub season: u32,
    pub episode: u32,
    pub episode_title: Option<String>,
}

/// Result of parsing a media filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedMedia {
    Movie(MovieInfo),
    Episode(EpisodeInfo),
    Unknown(String),
}

// Patterns to ignore
static IGNORE_NAMES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "@eaDir",
    ".nfo",
    ".txt",
    ".jpg",
    ".jpeg",
    ".png",
    ".srt",
    ".sub",
    ".idx",
    ".ass",
    ".ssa",
];

static VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "avi", "m4v", "mov", "wmv", "flv", "webm", "ts", "mpg", "mpeg", "3gp", "ogv",
];

// SxxExx pattern: S01E02, s1e3, etc.
static RE_SXXEXX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[Ss](\d{1,2})[Ee](\d{1,3})").unwrap()
});

// 1x02 pattern
static RE_XEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\d{1,2})[xX](\d{2,3})").unwrap()
});

// "Season X Episode Y" pattern
static RE_SEASON_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)Season\s+(\d+)\s+Episode\s+(\d+)").unwrap()
});

// Movie: "Title (Year)" or "Title.Year"
static RE_MOVIE_YEAR_PAREN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.+?)\s*\((\d{4})\)").unwrap()
});

static RE_MOVIE_YEAR_DOT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.+?)[\.\s](\d{4})(?:[\.\s]|$)").unwrap()
});

// Provider ID in folder name: [tmdb=12345], [tvdb=67890], [imdb=tt123]
static RE_PROVIDER_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(\w+)=([^\]]+)\]").unwrap()
});

/// Check if a filename should be ignored.
pub fn should_ignore(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    IGNORE_NAMES
        .iter()
        .any(|pat| lower == pat.to_lowercase() || lower.ends_with(pat))
}

/// Check if a file has a video extension.
pub fn is_video_file(filename: &str) -> bool {
    if let Some(ext) = filename.rsplit('.').next() {
        VIDEO_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    } else {
        false
    }
}

/// Extract provider IDs from a folder/file name like `[tmdb=12345]`.
pub fn extract_provider_ids(name: &str) -> Vec<(String, String)> {
    RE_PROVIDER_ID
        .captures_iter(name)
        .map(|c| (c[1].to_lowercase(), c[2].to_string()))
        .collect()
}

/// Clean up a title: replace dots/underscores with spaces, trim.
fn clean_title(raw: &str) -> String {
    raw.replace('.', " ")
        .replace('_', " ")
        .trim()
        .to_string()
}

/// Parse a video filename into movie or episode info.
pub fn parse_filename(filename: &str) -> ParsedMedia {
    let stem = filename
        .rsplit('/')
        .next()
        .unwrap_or(filename)
        .rsplit('\\')
        .next()
        .unwrap_or(filename);

    // Strip extension
    let stem = if let Some(pos) = stem.rfind('.') {
        &stem[..pos]
    } else {
        stem
    };

    // Try episode patterns first (more specific)
    if let Some(ep) = try_parse_episode(stem) {
        return ParsedMedia::Episode(ep);
    }

    // Try movie patterns
    if let Some(movie) = try_parse_movie(stem) {
        return ParsedMedia::Movie(movie);
    }

    // Fallback: treat as movie with just a title
    ParsedMedia::Movie(MovieInfo {
        title: clean_title(stem),
        year: None,
    })
}

fn try_parse_episode(stem: &str) -> Option<EpisodeInfo> {
    // Try SxxExx
    if let Some(caps) = RE_SXXEXX.captures(stem) {
        let season: u32 = caps[1].parse().ok()?;
        let episode: u32 = caps[2].parse().ok()?;
        let match_start = caps.get(0)?.start();
        let series_raw = &stem[..match_start];
        let series_title = clean_title(series_raw);
        let after = &stem[caps.get(0)?.end()..];
        let episode_title = if after.len() > 1 {
            let t = clean_title(after.trim_start_matches(['-', '.', ' ', '_']));
            if t.is_empty() { None } else { Some(t) }
        } else {
            None
        };
        return Some(EpisodeInfo {
            series_title,
            season,
            episode,
            episode_title,
        });
    }

    // Try 1x02
    if let Some(caps) = RE_XEP.captures(stem) {
        let season: u32 = caps[1].parse().ok()?;
        let episode: u32 = caps[2].parse().ok()?;
        let match_start = caps.get(0)?.start();
        let series_raw = &stem[..match_start];
        let series_title = clean_title(series_raw);
        return Some(EpisodeInfo {
            series_title,
            season,
            episode,
            episode_title: None,
        });
    }

    // Try "Season X Episode Y"
    if let Some(caps) = RE_SEASON_EPISODE.captures(stem) {
        let season: u32 = caps[1].parse().ok()?;
        let episode: u32 = caps[2].parse().ok()?;
        let match_start = caps.get(0)?.start();
        let series_raw = &stem[..match_start];
        let series_title = clean_title(series_raw);
        return Some(EpisodeInfo {
            series_title,
            season,
            episode,
            episode_title: None,
        });
    }

    None
}

fn try_parse_movie(stem: &str) -> Option<MovieInfo> {
    // "Title (2024)"
    if let Some(caps) = RE_MOVIE_YEAR_PAREN.captures(stem) {
        let title = clean_title(&caps[1]);
        let year: u16 = caps[2].parse().ok()?;
        return Some(MovieInfo {
            title,
            year: Some(year),
        });
    }

    // "Title.2024.etc"
    if let Some(caps) = RE_MOVIE_YEAR_DOT.captures(stem) {
        let title = clean_title(&caps[1]);
        let year: u16 = caps[2].parse().ok()?;
        if year >= 1900 && year <= 2100 {
            return Some(MovieInfo {
                title,
                year: Some(year),
            });
        }
    }

    None
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sxxexx() {
        let r = parse_filename("Breaking.Bad.S02E05.Episode.Title.mkv");
        assert_eq!(
            r,
            ParsedMedia::Episode(EpisodeInfo {
                series_title: "Breaking Bad".into(),
                season: 2,
                episode: 5,
                episode_title: Some("Episode Title".into()),
            })
        );
    }

    #[test]
    fn parse_sxxexx_lowercase() {
        let r = parse_filename("the.office.s01e01.pilot.mp4");
        assert_eq!(
            r,
            ParsedMedia::Episode(EpisodeInfo {
                series_title: "the office".into(),
                season: 1,
                episode: 1,
                episode_title: Some("pilot".into()),
            })
        );
    }

    #[test]
    fn parse_xep_format() {
        let r = parse_filename("Seinfeld.3x12.avi");
        assert_eq!(
            r,
            ParsedMedia::Episode(EpisodeInfo {
                series_title: "Seinfeld".into(),
                season: 3,
                episode: 12,
                episode_title: None,
            })
        );
    }

    #[test]
    fn parse_season_episode_format() {
        let r = parse_filename("Friends Season 2 Episode 14.mkv");
        assert_eq!(
            r,
            ParsedMedia::Episode(EpisodeInfo {
                series_title: "Friends".into(),
                season: 2,
                episode: 14,
                episode_title: None,
            })
        );
    }

    #[test]
    fn parse_movie_with_year_paren() {
        let r = parse_filename("The Matrix (1999).mkv");
        assert_eq!(
            r,
            ParsedMedia::Movie(MovieInfo {
                title: "The Matrix".into(),
                year: Some(1999),
            })
        );
    }

    #[test]
    fn parse_movie_with_year_dot() {
        let r = parse_filename("Inception.2010.1080p.BluRay.mkv");
        assert_eq!(
            r,
            ParsedMedia::Movie(MovieInfo {
                title: "Inception".into(),
                year: Some(2010),
            })
        );
    }

    #[test]
    fn parse_movie_no_year() {
        let r = parse_filename("Some Random Movie.mp4");
        assert_eq!(
            r,
            ParsedMedia::Movie(MovieInfo {
                title: "Some Random Movie".into(),
                year: None,
            })
        );
    }

    #[test]
    fn ignore_patterns() {
        assert!(should_ignore(".DS_Store"));
        assert!(should_ignore("Thumbs.db"));
        assert!(should_ignore("movie.nfo"));
        assert!(should_ignore("poster.jpg"));
        assert!(!should_ignore("movie.mkv"));
    }

    #[test]
    fn video_extension_check() {
        assert!(is_video_file("movie.mkv"));
        assert!(is_video_file("Movie.MP4"));
        assert!(is_video_file("ep.avi"));
        assert!(!is_video_file("poster.jpg"));
        assert!(!is_video_file("subs.srt"));
    }

    #[test]
    fn provider_ids_extraction() {
        let ids = extract_provider_ids("Breaking Bad [tmdb=1396] [tvdb=81189]");
        assert_eq!(ids, vec![
            ("tmdb".to_string(), "1396".to_string()),
            ("tvdb".to_string(), "81189".to_string()),
        ]);
    }

    #[test]
    fn provider_ids_imdb() {
        let ids = extract_provider_ids("The Matrix (1999) [imdb=tt0133093]");
        assert_eq!(ids, vec![("imdb".to_string(), "tt0133093".to_string())]);
    }

    #[test]
    fn specials_season_zero() {
        let r = parse_filename("Show.Name.S00E01.Special.mkv");
        assert_eq!(
            r,
            ParsedMedia::Episode(EpisodeInfo {
                series_title: "Show Name".into(),
                season: 0,
                episode: 1,
                episode_title: Some("Special".into()),
            })
        );
    }
}
