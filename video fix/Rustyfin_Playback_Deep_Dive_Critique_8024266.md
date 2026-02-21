# Critique & Verification: `Rustyfin_Playback_Deep_Dive_8024266.md`

**Scope.** This review validates the claims in the playback deep-dive report against the **`Rustyfin-8024266.zip`** codebase by directly inspecting the source files in the archive. Where possible, I reference *file paths* and *line numbers* for verification.

**High-level verdict.** The report correctly identifies the *primary functional failure* (Player never gets a `file_id`, so clicks become no-ops). However, it misses one *major security gap* (HLS endpoints are unauthenticated), includes at least one *factual inaccuracy* (MIME mapping already includes `m4v`), and under-specifies the “secure modern” approach to streaming auth (JWT-in-query and localStorage are risky defaults).

---

## 0) Quick verification matrix (what the report got right / wrong / incomplete)

### ✅ Correct and well-supported
- Root cause: UI expects `file_id`, API `/api/v1/items/:id` doesn’t include it → buttons do nothing.
  - Verified in:
    - `ui/src/app/player/[id]/page.tsx` around line 19
    - `crates/server/src/routes.rs` `ItemResponse` around line 882 and `get_item` around line 928
- Direct-play streaming supports Range and allows auth via header or query token.
  - Verified in: `crates/server/src/streaming.rs` around line 104.
- Transcode uses ffmpeg to generate HLS `.ts` segments and `master.m3u8`.
  - Verified in: `crates/transcoder/src/session.rs` `spawn_ffmpeg()`.

### ❌ Incorrect
- Claim: “`content_type_for_path()` misses `m4v`”
  - Actual code already maps `m4v` to `video/mp4` in `crates/server/src/streaming.rs` (see the `Some("mp4" | "m4v")` arm).

### ⚠️ Incomplete / missing
- HLS serving endpoints (`/stream/hls/...`) **do not require authentication** and are effectively “bearer-by-URL” (session id = access).
- JWTs are placed in URL query strings for direct play (`?token=...`) which is a security smell unless handled as a short-lived, scoped streaming token.
- No recommendation for cache/referrer policy hardening on stream endpoints.
- No mention that the transcoder “max transcodes” semaphore permit is dropped immediately, weakening concurrency enforcement.

---

## 1) Report section 1: “Playback pipeline (what should happen)”

### Where is the problem?
In the report’s omission of **auth boundaries** for stream endpoints.

### What is the problem?
The pipeline description is broadly correct, but it omits the crucial statement:  
**every `/stream/*` request must be authenticated + authorized**, not only the session-creation request.

### How is it a problem?
Without explicit auth gates, the system drifts toward “bearer URLs”:
- session IDs become access keys
- leaked URLs become leaked content

### How to fix the problem
Update the report to include explicit “Auth gates”:
- **Gate A:** `POST /api/v1/playback/sessions` requires auth + library access (already true)
- **Gate B:** `GET /stream/file/*` and `GET /stream/hls/*` require auth + authorization (currently only true for direct-file)

### What “fixed” looks like
- The report’s pipeline section includes an “Auth at every stream hop” box.
- The implementation enforces 401/403 on unauthenticated HLS master/segments.

---

## 2) Report section 2: “Clicking does nothing” (primary root cause)

### Where is the problem?
- UI: `ui/src/app/player/[id]/page.tsx`
- API: `crates/server/src/routes.rs` (`ItemResponse`, `get_item`)

### What is the problem?
Correctly identified in the report:  
Player requests `/api/v1/items/{id}` and assumes the JSON includes `file_id`. If missing, Direct Play never sets `video.src` and HLS returns immediately.

### How is it a problem?
It matches the observed behavior: “both buttons do nothing”.

### How to fix the problem
Preferred approach: **Dedicated Playback Descriptor endpoint** (report’s Option B), hardened:
- `GET /api/v1/items/{id}/playback` → returns `file_id` and *safe streaming URLs* (not raw JWTs; see security sections below).
- Player disables buttons and shows a clear error if `file_id` is absent.

### What “fixed” looks like
- Clicking Direct Play triggers `GET /stream/file/{file_id}` and playback starts.
- Clicking HLS triggers session creation and then playlist/segment requests.

---

## 3) Report section 3: “Secondary blockers” — corrections and additions

### 3.1 Capability detection: modernize beyond `canPlayType()`

#### Where is the problem?
Report recommends mostly `canPlayType()` and `video.onerror` fallback.

#### What is the problem?
`canPlayType()` is coarse (“maybe”), and doesn’t account for performance.

#### How to fix the problem
Update the report: prefer **Media Capabilities API** where available, fallback to `canPlayType()`.

#### What “fixed” looks like
Fewer “direct-play then immediate failure” cases, and better default decisions.

---

### 3.2 MIME mapping: correct the report and extend where truly missing

#### Where is the problem?
`crates/server/src/streaming.rs::content_type_for_path()`

#### What is the problem?
The report claims `m4v` is missing; it isn’t.  
The mapping *is* missing common TS variants (e.g. `m2ts`, `mts`) and others.

#### How to fix the problem
- Update the report to remove the incorrect `m4v` claim.
- Expand the mapping to align with the scanner allowlist.

#### What “fixed” looks like
Direct play responses use a sensible `Content-Type` for more extensions, reducing client-side misbehavior.

---

### 3.3 Transcoder robustness: surface two issues the report doesn’t mention

#### Where is the problem?
1) `get_media_info()` uses `ffprobe` via `Path::new("ffprobe")` (hardcoded binary name).  
2) `crates/transcoder/src/session.rs` drops the semaphore permit immediately.

#### What is the problem?
1) Hardcoding `ffprobe` makes deployments fragile (PATH differences).  
2) Dropping the permit weakens the “max concurrent transcodes” guarantee.

#### How to fix the problem
- Make ffprobe path configurable + validated at startup.
- Hold the semaphore permit **inside the TranscodeSession** (e.g., `OwnedSemaphorePermit`) so it lives for the session’s lifetime.

#### What “fixed” looks like
- `/health` reports ffmpeg/ffprobe readiness.
- concurrency limits are deterministic under load.

---

### 3.4 HLS MIME casing: clarify what matters

#### Where is the problem?
`crates/transcoder/src/hls.rs` uses `video/MP2T`.

#### What is the problem?
The report implies lowercase is “canonical”. MIME type tokens are case-insensitive; what matters is **consistency and correctness**.

#### How to fix the problem
Update the report wording: “Case isn’t the bug; correctness is.”

#### What “fixed” looks like
No one chases casing ghosts during debugging; tests assert the correct MIME class is delivered.

---

## 4) Major missing security issue: HLS streaming is unauthenticated

### Where is the problem?
- `crates/server/src/routes.rs`
  - `hls_master(State(state), Path(sid))` has no `AuthUser`
  - `hls_segment(State(state), Path((sid, filename)))` has no `AuthUser`

### What is the problem?
Anyone with a `session_id` can fetch playlists and segments. Session id becomes a bearer credential.

### How is it a problem?
- Authorization bypass (session id = access)
- Session hijacking if IDs leak via logs/history/referrers
- DoS via keeping sessions alive

### How to fix the problem
Use one of these modern patterns:

**Fix A (preferred): same-origin HttpOnly cookie auth for `/stream/*`**
- Native `<video>` and Safari HLS can authenticate without URL tokens.

**Fix B: short-lived, scoped “stream token” (NOT the main JWT)**
- TTL 30–120s, scope bound to `session_id` or `file_id`.
- Include as query `st=` in playlist + segment URLs.
- Validate on every request.

Also:
- bind the transcoder session to `(user_id, file_id)` and enforce ownership on every HLS request.

### What “fixed” looks like
- `/stream/hls/...` without auth returns 401/403.
- With valid auth (cookie or stream token), playback works.

---

## 5) Security: primary JWT in query parameters (`?token=`) is risky

### Where is the problem?
- UI: `ui/src/app/player/[id]/page.tsx` builds `/stream/file/{file_id}?token=...`
- Server: `crates/server/src/streaming.rs` accepts query `token`.

### What is the problem?
Primary bearer tokens in URLs leak via logs, browser history, and referrers.

### How to fix the problem
- Prefer HttpOnly cookie auth (same-origin).
- If you need URL tokens: mint a **separate stream token** (short TTL + scope-limited).
- Harden with:
  - `Referrer-Policy` (reduce leakage)
  - `Cache-Control: no-store` (avoid caching private media)

### What “fixed” looks like
- Streaming never requires long-lived auth JWT in the URL.
- Any URL token is short-lived and scoped to a single stream.

---

## Appendix A: Evidence snippets (from the codebase)

### Player expects `file_id` from `/items/:id`
```text
  13:   const [sessionId, setSessionId] = useState<string | null>(null);
  14:   const [error, setError] = useState('');
  15: 
  16:   // Get the file ID for this item
  17:   useEffect(() => {
  18:     // Items have a file_id field from the episode_file_map
  19:     apiJson<{ file_id?: string }>(`/items/${id}`)
  20:       .then((item: any) => {
  21:         if (item.file_id) {
  22:           setFileId(item.file_id);
  23:         }
  24:       })
  25:       .catch((e) => setError(e.message));
```

### Items API response has no `file_id`
```text
 868:         .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
 869: 
 870:     if !cancelled {
 871:         return Err(ApiError::BadRequest("job not found or not cancellable".into()).into());
 872:     }
 873: 
 874:     Ok(Json(serde_json::json!({ "ok": true })))
 875: }
 876: 
 877: // ---------------------------------------------------------------------------
 878: // Items
 879: // ---------------------------------------------------------------------------
 880: 
 881: #[derive(Serialize)]
 882: struct ItemResponse {
 883:     id: String,
 884:     library_id: String,
 885:     kind: String,
 886:     parent_id: Option<String>,
 887:     title: String,
 888:     sort_title: Option<String>,
 889:     year: Option<i64>,
 890:     overview: Option<String>,
 891:     created_ts: i64,
 892:     updated_ts: i64,
 893: }
 894: 
 895: fn item_to_response(item: rustfin_db::repo::items::ItemRow) -> ItemResponse {
 896:     ItemResponse {
```

### Streaming accepts query token
```text
  94:         Some("mov") => "video/quicktime",
  95:         Some("ts") => "video/mp2t",
  96:         Some("mpg" | "mpeg") => "video/mpeg",
  97:         _ => "application/octet-stream",
  98:     }
  99: }
 100: 
 101: /// Stream a file with HTTP Range support (Direct Play).
 102: /// GET /stream/file/{file_id}
 103: #[derive(Debug, Default, Deserialize)]
 104: pub struct StreamAuthQuery {
 105:     token: Option<String>,
 106: }
 107: 
 108: pub async fn stream_file_range(
 109:     State(state): State<AppState>,
 110:     Path(file_id): Path<String>,
 111:     Query(query): Query<StreamAuthQuery>,
 112:     headers: HeaderMap,
 113: ) -> Result<Response, AppError> {
 114:     // Require JWT either via Authorization header or query token.
```

### HLS endpoints lack authentication
```text
1146:         .ok_or(ApiError::NotFound("media file not found".into()))?;
1147: 
1148:     let info = rustfin_transcoder::ffprobe::probe(
1149:         std::path::Path::new("ffprobe"),
1150:         std::path::Path::new(&file.path),
1151:     )
1152:     .await
1153:     .map_err(|e| ApiError::Internal(format!("ffprobe error: {e}")))?;
1154: 
1155:     Ok(Json(serde_json::to_value(&info).unwrap()))
1156: }
1157: 
1158: // ---------------------------------------------------------------------------
1159: // HLS serving
1160: // ---------------------------------------------------------------------------
1161: 
1162: async fn hls_master(
1163:     State(state): State<AppState>,
1164:     Path(sid): Path<String>,
1165: ) -> Result<axum::response::Response, AppError> {
1166:     use axum::body::Body;
1167:     use axum::response::IntoResponse;
1168: 
1169:     // Ping the session
1170:     if !state.transcoder.ping(&sid).await {
1171:         return Err(ApiError::NotFound("HLS session not found".into()).into());
1172:     }
1173: 
1174:     let path = state
1175:         .transcoder
1176:         .get_file_path(&sid, "master.m3u8")
1177:         .await
1178:         .map_err(|e| ApiError::NotFound(format!("session error: {e}")))?;

1190:     }
1191: 
1192:     let content = tokio::fs::read_to_string(&path)
1193:         .await
1194:         .map_err(|e| ApiError::Internal(format!("read playlist: {e}")))?;
1195: 
1196:     Ok((
1197:         [(
1198:             axum::http::header::CONTENT_TYPE,
1199:             rustfin_transcoder::hls::PLAYLIST_CONTENT_TYPE,
1200:         )],
1201:         Body::from(content),
1202:     )
1203:         .into_response())
1204: }
1205: 
1206: async fn hls_segment(
1207:     State(state): State<AppState>,
1208:     Path((sid, filename)): Path<(String, String)>,
1209: ) -> Result<axum::response::Response, AppError> {
1210:     use axum::body::Body;
1211:     use axum::response::IntoResponse;
1212: 
1213:     // Ping the session
1214:     if !state.transcoder.ping(&sid).await {
1215:         return Err(ApiError::NotFound("HLS session not found".into()).into());
1216:     }
1217: 
1218:     // Validate filename (prevent traversal)
1219:     if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
1220:         return Err(ApiError::BadRequest("invalid filename".into()).into());
1221:     }
1222: 
```

### HLS MIME constants
```text
   1: //! HLS playlist and segment content-type helpers.
   2: 
   3: /// Content-Type for HLS master/variant playlists.
   4: pub const PLAYLIST_CONTENT_TYPE: &str = "application/vnd.apple.mpegurl";
   5: 
   6: /// Content-Type for MPEG-TS segments.
   7: pub const SEGMENT_CONTENT_TYPE_TS: &str = "video/MP2T";
   8: 
   9: /// Content-Type for fMP4 segments.
  10: pub const SEGMENT_CONTENT_TYPE_MP4: &str = "video/mp4";
  11: 
  12: /// Determine segment content type from filename extension.
  13: pub fn segment_content_type(filename: &str) -> &'static str {
  14:     if filename.ends_with(".m4s") || filename.ends_with(".mp4") {
```

### Transcoder drops concurrency permit
```text
 102:             started_at: Instant::now(),
 103:             last_ping: Instant::now(),
 104:             child: Some(child),
 105:         };
 106: 
 107:         self.sessions
 108:             .lock()
 109:             .await
 110:             .insert(session_id.clone(), session);
 111: 
 112:         // The semaphore permit is dropped here, but we track active sessions via the map.
 113:         // We re-check count in create_session. For true gating, we'd hold the permit
 114:         // in the session, but that complicates the borrow. The try_acquire + map size
 115:         // provides adequate protection.
 116:         // Actually, let's forget the permit — we'll just check map size.
 117:         drop(_permit);
 118: 
 119:         info!(session_id = %session_id, "HLS transcode session created");
 120:         Ok(session_id)
 121:     }
 122: 
```
