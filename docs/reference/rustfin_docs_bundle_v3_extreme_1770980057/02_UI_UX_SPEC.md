# UI/UX Specification (Extreme Expansion, Mobile-First)

## 1. UX definition of done
User can:
- add libraries and see scan progress,
- browse shows as **Show → Seasons → Episodes** by default,
- see season posters + episode thumbs,
- optionally show missing episodes (expected vs present),
- play with quality, speed, audio/subtitle selection,
- admin: users, providers, transcoding, diagnostics.

Jellyfin TV/specials behavior is the grounding target:
https://jellyfin.org/docs/general/server/media/shows/

---

## 2. Primary nav
Mobile tabs:
- Home
- Libraries
- Search
- Player (only when active)
- Profile

Admin pages:
- Dashboard
- Users
- Libraries
- Providers
- Transcoding
- Diagnostics

---

## 3. Screens

### 3.1 Home
- Continue Watching
- Next Up (TV)
- Recently Added
- New Seasons

Quick actions on tiles:
- Play/Resume
- Mark played/unplayed
- Favorite

### 3.2 Movies
Poster grid, filters, sorting, view modes.

### 3.3 Shows (core requirement)
Default: show tiles → season grid → episode list.

Specials:
- show Season 00 “Specials” if present
- optional inject specials into season order if configured (airsbefore/airsafter)

### 3.4 Season page
Episode list:
- thumb, title, runtime, watched state
- badges: Missing / Special / Multi-part / Future

### 3.5 Missing episodes
Per-user toggle:
- OFF: only present files
- ON: placeholders from expected list + counts “8/10 present”

### 3.6 Player
- speed control
- quality selector (Auto + fixed)
- audio track picker
- subtitles picker (forced/SDH markers)
- stats overlay (decision + bitrate + codecs)

---

## 4. Accessibility
- contrast meets WCAG minimum (see theme doc)
- reduced motion support
- tap targets ≥ 44px
