# Testing, Observability, and Security (Extreme Expansion)

## 1. Tests
- unit: parsers, range handling, merge rules
- integration: scan fixture tree, browse APIs
- streaming: validate 206 responses and Content-Range correctness
- HLS: playlist validity and segment serving

## 2. Observability
- tracing logs with request IDs and session IDs
- optional metrics: transcodes active, queue depth

## 3. Security (LAN-safe)
- argon2/bcrypt password hashing
- strict path canonicalization (prevent traversal)
- provider allowlist by default (reduce SSRF)
- avoid shell invocation of ffmpeg (argv list)
