# Channel Audio/Text Audit

Date: 2026-03-09

## Scope

This pass audited the Rustyfin channel stack with emphasis on:

- long-lived voice sessions
- bursty text traffic
- attachment upload/download behavior under concurrency
- WebSocket backpressure and failure modes
- PostgreSQL query/write patterns on the hot channel paths

Files reviewed included:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/ws.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/manager.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/channels.rs`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/channelsContext.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/channels/components/VoiceEngine.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/channels/components/TextChannelView.tsx`

## Summary

The channel stack was already in a usable state, but it had several concrete reliability risks that would show up under sustained use:

- voice membership trusted the client too much
- per-user WS delivery used unbounded queues
- attachment uploads buffered the full file in memory before writing
- attachment downloads also loaded the full file into memory
- HTTP message/upload paths had no dedicated per-route rate limiting

Those are the wrong failure modes for a home-server product expected to sit open for hours.

This pass fixes the highest-value issues immediately.

## Implemented In This Pass

### 1. Voice joins now validate real channel access

Before:

- a client could send `join_voice` for any arbitrary channel id
- server-side join logic did not verify that the channel existed
- it also did not verify that the channel was actually a voice channel
- private voice-channel access relied too much on the client behaving properly

Now:

- `join_voice` is validated against the database
- only accessible voice channels can be joined
- invalid joins are rejected server-side

Relevant file:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/ws.rs`

### 2. Single-voice-membership is enforced server-side

Before:

- a buggy or malicious client could join multiple voice channels without properly leaving the old one
- that could create inconsistent in-memory presence and confusing RTC routing

Now:

- the server removes the user from all other voice channels before accepting a new voice join
- leave presence is broadcast for the channels the user was removed from

Relevant file:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/manager.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/ws.rs`

### 3. RTC forwarding is now scoped to shared voice membership

Before:

- `rtc_offer`, `rtc_answer`, and `rtc_ice` forwarding only trusted the `to_user_id` and `channel_id` supplied by the client

Now:

- the server verifies that both users are currently members of the same voice channel before forwarding RTC signaling

Relevant file:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/ws.rs`

### 4. Personal WS queues are now bounded

Before:

- `ChannelManager.user_senders` used `mpsc::UnboundedSender`
- a slow or stalled websocket consumer could accumulate arbitrary memory over time

Now:

- per-user personal queues are bounded
- if a client cannot keep up, its personal queue is evicted instead of growing without bound
- that forces disconnect/recovery rather than silent memory growth

Relevant file:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/manager.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/ws.rs`

### 5. Attachment uploads now stream to disk

Before:

- upload handler used `field.bytes()`
- each attachment was fully buffered in memory first
- multiple concurrent 25 MB uploads could create unnecessary heap pressure and contention with live chat/call activity

Now:

- attachment uploads are streamed chunk-by-chunk to disk
- max size is enforced during the stream
- partial files are cleaned up on failure

Relevant file:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`

### 6. Attachment downloads now stream from disk

Before:

- download handler used `fs::read`
- large downloads loaded the full file into memory before being sent

Now:

- attachment downloads use streamed file responses
- large downloads no longer compete with the rest of the channel traffic for the same memory spike

Relevant file:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`

### 7. HTTP-side message and upload rate limits were added

Before:

- websocket messages had a budget
- the plain HTTP routes for text send/upload did not have comparable protections

Now:

- message send route has a dedicated rate limiter
- attachment upload route has a stricter dedicated rate limiter
- this closes the obvious bypass path where a client could avoid websocket limits by hammering HTTP endpoints directly

Relevant file:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`

## What Looks Solid

### Voice session lifecycle

The client voice engine already had decent cleanup behavior:

- peer connections are closed on teardown
- audio elements are removed
- speaking monitors are stopped
- local audio/transcription pipelines are torn down on leave/unmount
- websocket reconnect logic is present

For a small home-server style deployment, this is acceptable.

### Text message retrieval

The text message read path is already reasonable:

- stable pagination on `(created_ts DESC, id DESC)`
- reversed back to ascending order for UI display
- existing PostgreSQL indexes support the hot path
- batched attachment fetch for message pages avoids obvious N+1 behavior

### Client-side message state

`channelsContext` already caps websocket-fed `newMessages` to the most recent 200 items, so it is not unbounded client memory growth.

## Remaining Risks / Recommended Follow-Up

### P1. Improve true message ordering under burst traffic

Current message timestamps are second-resolution integers. Under heavy message bursts inside the same second, ordering falls back to UUID ordering rather than true arrival order.

Recommendation:

- move `channel_message.created_ts` to millisecond precision, or
- add a per-channel monotonic sequence for perfect ordering

This is more about correctness under burst load than raw performance.

### P1. Use DB transactions for attachment-message creation

The upload flow still creates the text message and then creates the attachment row, with cleanup logic on failure.

That works, but a transaction is cleaner and makes the write path stricter under failure.

Recommendation:

- wrap message row creation + attachment row creation in a single transaction

### P1. Replace `ScriptProcessorNode` with `AudioWorklet` for transcription capture

`VoiceEngine` currently uses `createScriptProcessor`, which is legacy. It still works, but `AudioWorklet` is the more robust long-session browser path and gives better scheduling characteristics.

Recommendation:

- move local transcription capture from `ScriptProcessorNode` to `AudioWorklet`

### P2. Add channel metrics/observability

Right now there is limited operational visibility into:

- websocket disconnect causes
- slow-consumer evictions
- attachment upload throughput
- attachment download throughput
- average message/query latency

Recommendation:

- add structured counters/logs for these hot paths

### P2. Consider stronger media-specific attachment handling

Image previews in the text channel currently fetch the whole image blob client-side for preview.

That is acceptable for a home deployment, but if channel attachments become more common:

- server-generated thumbnails
- stricter content-type policy
- separate image size caps

would improve responsiveness.

## PostgreSQL Assessment

The current Postgres usage on channels is not obviously wasteful or naive. The main hot reads already have the right shape and supporting indexes.

The improvements that matter most here were not exotic SQL tricks. They were:

- controlling memory and queue growth
- protecting the non-WS ingress paths
- reducing trust in the client for voice routing
- avoiding full-file buffering for uploads/downloads

That is the correct order of operations. More niche SQL changes would not have produced the same reliability gain as these fixes.

## Validation

The following checks were run after the changes:

- `cargo fmt --all`
- `cargo check -p rustfin-server`
- `cargo test -p rustfin-server channels:: --lib`

All passed.
