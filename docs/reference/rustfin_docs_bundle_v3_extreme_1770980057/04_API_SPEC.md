# API Specification (Extreme Expansion)

Prefix: `/api/v1`

## 1. Errors
```json
{ "error": { "code": "bad_request", "message": "…", "details": {} } }
```

## 2. Auth
- POST /auth/login
- POST /auth/logout
- POST /auth/refresh

## 3. Users
- GET /users/me
- PATCH /users/me/preferences
- admin: GET/POST/PATCH/DELETE /users

## 4. Libraries
- POST /libraries
- GET /libraries
- GET /libraries/{id}
- PATCH /libraries/{id}
- POST /libraries/{id}/scan
- GET /libraries/{id}/items

## 5. Items
- GET /items/{id}
- PATCH /items/{id}
- POST /items/{id}/refresh
- POST /items/{id}/identify
- GET /items/{id}/children
- GET /series/{id}/seasons
- GET /seasons/{id}/episodes?include_missing=true

## 6. Images
- GET /items/{id}/images/{type}?w=&h=&format=

## 7. Playback sessions
- POST /playback/sessions
- POST /playback/sessions/{sid}/progress
- POST /playback/sessions/{sid}/stop

## 8. Streaming
Range: GET /stream/file/{file_id}
- RFC 7233: https://www.rfc-editor.org/rfc/rfc7233
- Accept-Ranges (MDN): https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Accept-Ranges

HLS:
- RFC 8216: https://www.rfc-editor.org/rfc/rfc8216
- GET /stream/hls/{sid}/master.m3u8
- GET /stream/hls/{sid}/variant_{n}.m3u8
- GET /stream/hls/{sid}/seg_{n}.m4s (or .ts)

## 9. Jobs/events
- GET /events (SSE)
- GET /jobs
- GET /jobs/{id}
- POST /jobs/{id}/cancel
