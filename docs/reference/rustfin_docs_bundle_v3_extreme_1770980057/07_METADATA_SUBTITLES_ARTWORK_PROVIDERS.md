# Metadata, Subtitles, Artwork, and Providers (Extreme Expansion)

Grounding target for TV structure, specials, and image naming:
https://jellyfin.org/docs/general/server/media/shows/

## 1. Provider strategy (keep it small)
- Movies: TMDb
- TV: TVDb or TMDb (chosen per series, then locked)
- Optional: OMDb, fanart.tv, OpenSubtitles

## 2. Precedence (merge rules)
1) User edits (DB)
2) Local sidecars (NFO/images)
3) Provider metadata
4) Derived from filenames

## 3. IDs in folder names (deterministic matching)
Examples:
- `Show Name (2020) [tvdb=12345]`
- `Movie Name (2019) [tmdb=6789]`

## 4. Missing episodes
- canonical expected episode list stored after identification
- UI compares expected vs present and shows placeholders if enabled

## 5. Subtitles
- sidecar discovery: `Title.S01E01.en.srt`, `...en.forced.srt`
- embedded track listing: ffprobe cached JSON
- burn-in only when device requires

## 6. TMDb images
Image URL construction:
https://developer.themoviedb.org/docs/image-basics
