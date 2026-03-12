You are an autonomous senior security-focused full-stack engineer working inside this repository. Your job is to implement the password vault described in:

- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/password-vault-design-2026-03-12.md`

You must treat that document as the source of truth for product scope, threat model, crypto design, authorization model, autofill defaults, device-session requirements, and rollout order.

You are not writing a speculative prototype. You are building a production-grade first version of Rustyfin Vault that is aligned with the repo's current Rust backend, PostgreSQL storage model, Next.js UI, and Rustyfin design language.

Read these first and follow them:

- `/Users/iwanteague/Desktop/Rustyfin/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
- `/Users/iwanteague/Desktop/Rustyfin/CLAUDE.md` if it exists
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/password-vault-design-2026-03-12.md`

Your operating mode:

- implement, do not just plan
- keep going until the highest safe level of completion is reached
- do not stop at analysis
- do not leave TODOs or placeholders
- do not claim work is done if it is only scaffolded
- if a later feature depends on missing security foundations, implement the foundation first
- if the entire document cannot be completed in one run, still complete the largest coherent, secure slice end-to-end, then clearly report what remains and why

Non-negotiable constraints:

- Keep backend logic in Rust.
- Use PostgreSQL through the existing repo patterns in `crates/db`.
- Keep the Rustyfin web UI in the existing theme and component style.
- Do not weaken existing auth, setup, libraries, rooms, channels, servers, or runtime behavior.
- Do not add a separate vault admin panel.
- Do not add organization sharing, family sharing, or collection-sharing features in v1.
- Do not add server-side plaintext vault decryption in normal operation.
- Do not add server-side parsing of plaintext import files in v1.
- Do not add remote website metadata scraping, favicon fetching, or screenshot fetching.
- Do not add HTML or Markdown rendering for vault notes in v1.
- Do not store plaintext vault contents, vault master passwords, or unwrapped vault keys on the server.
- Do not implement page-load autofill by default.
- Do not autofill on HTTP by default.
- Do not autofill in untrusted iframes by default.
- Do not silently save credentials without explicit user confirmation.

Implementation objective:

Build Rustyfin Vault in the correct phased order:

1. security prerequisites
2. encrypted vault backend and database model
3. Rustyfin `/vault` web experience
4. browser extension MVP for site detection, save prompts, and manual autofill
5. verification, hardening, and documentation updates

What “done” means:

- the repository contains a working first-class vault feature aligned with the design document
- the data model, API, UI, crypto helpers, and extension are implemented rather than merely described
- the implementation follows the design document's security posture and does not regress into weaker shortcuts
- tests and builds are updated and passing where relevant

Implementation order and required work:

## Phase 0: Security prerequisites

Before building vault CRUD or autofill, implement the security foundations described in the design doc.

Required outcomes:

- Add vault-specific device sessions with:
  - opaque refresh tokens
  - rotation
  - revocation
  - replay-aware token family behavior
  - `client_kind` support for at least:
    - `web_vault`
    - `browser_extension`
- Add protected-action support for sensitive operations with:
  - challenge/initiation endpoint
  - one-time action token or equivalent
  - binding to:
    - user
    - device session
    - action kind
    - target item when applicable
  - short TTL
  - single-use consumption
- Keep existing app auth working, but do not let the extension just reuse the current browser token model.
- If you improve general auth/session handling as part of vault work, do so carefully and keep the app working.

At minimum, protected actions must cover:

- vault rekey
- vault export
- destructive import overwrite
- vault destruction
- new device approval
- revoke-all-other-sessions

If you include reveal/copy gating for especially sensitive fields, that is acceptable, but do not make the UX unusable.

## Phase 1: Database and shared types

Implement the vault schema and shared type system described in the design doc.

Required database work:

- add new migrations under `crates/db/migrations_pg/`
- add a new repo module in `crates/db/src/repo/vault.rs`
- wire it into existing db module exports

Required tables or equivalent structures:

- `vault_account`
- `vault_wrapped_key`
- `vault_item`
- `vault_item_uri_index`
- `vault_device_session`
- `vault_pending_device_approval`
- `vault_protected_action_token` if persisted
- `vault_audit_event`

Required schema properties:

- every vault row is scoped by `user_id`
- no ownership inference from client input
- separate encrypted summary and encrypted payload storage
- soft-delete support for vault items
- revision/version field for item replacement safety
- explicit algorithm and key-version metadata

Required shared types:

- add `crates/core/src/vault.rs`
- define versioned DTOs for:
  - vault config
  - encrypted item summary
  - encrypted item payload
  - URI match modes
  - device sessions
  - protected-action requests/responses
  - audit event responses

## Phase 2: Rust server implementation

Add a dedicated vault module in `crates/server/src/vault/`.

Required files or equivalent structure:

- `crates/server/src/vault/mod.rs`
- `crates/server/src/vault/router.rs`
- `crates/server/src/vault/handlers.rs`
- `crates/server/src/vault/service.rs`
- `crates/server/src/vault/device_sessions.rs`
- `crates/server/src/vault/audit.rs`

Wire the router into the main API router in `crates/server/src/routes.rs` or the appropriate composition point.

Required API behavior:

- `GET /api/v1/vault/config`
- `POST /api/v1/vault/bootstrap`
- `POST /api/v1/vault/rekey`
- `GET /api/v1/vault/items`
- `GET /api/v1/vault/items/{id}`
- `POST /api/v1/vault/items`
- `PUT /api/v1/vault/items/{id}`
- `DELETE /api/v1/vault/items/{id}`
- `POST /api/v1/vault/lookup`
- `GET /api/v1/vault/sync` if useful for extension sync
- pairing/device-session endpoints
- protected-action endpoints
- audit endpoints
- import/export endpoints consistent with the design doc

Critical server rules:

- never decrypt vault content as part of normal operation
- mutation endpoints must not leak expanded item details
- prefer full `PUT` replacement over weak partial merge semantics in v1
- every query that touches an item must scope by both `id` and `user_id`
- every alternate endpoint must share the same ownership rules as the canonical item read path
- never trust client-supplied ownership
- do not return another user's ciphertext or summary through lookup, sync, mutation, or device-session paths

Explicitly implement the authorization matrix from the design document.

## Phase 3: Cryptography implementation

Implement the client-side crypto model from the design doc.

Required v1 crypto choices:

- `Argon2id`
- `HKDF-SHA-256`
- `AES-256-GCM`
- `HMAC-SHA-256` for blinded lookup indexes

Required v1 KDF profile:

- Argon2id memory: `65536 KiB`
- Argon2id iterations: `3`
- Argon2id parallelism: `4`

Do not ship user-editable KDF tuning in v1.

Required crypto behavior:

- separate Rustyfin account auth from vault master-password unlock
- derive a master unlock key from the vault master password
- derive subkeys with HKDF
- generate and wrap a random vault data key
- encrypt summary and payload separately
- bind AAD to:
  - user ID
  - item ID
  - blob kind
  - schema or payload version
- compute blinded URI lookup hashes client-side

Client-side crypto helpers should live in the UI layer and shared code used by the extension where appropriate.

Recommended files:

- `ui/src/lib/vaultCrypto.ts`
- `ui/src/lib/vaultGenerator.ts`
- `ui/src/lib/vaultApi.ts`
- `ui/src/lib/vaultSession.ts`

Do not:

- store unwrapped vault keys in localStorage
- store plaintext decrypted vault state in localStorage
- store plaintext master-password-derived material in persistent browser storage

If you need browser persistence, keep it limited to ciphertext or non-sensitive session state, consistent with the design document.

## Phase 4: Rustyfin web UI

Build a first-class `/vault` experience in the existing Rustyfin visual language.

Required route and components:

- `ui/src/app/vault/page.tsx`
- supporting components for:
  - unlock panel
  - vault list
  - item editor
  - password generator
  - device sessions
  - audit view
  - security settings

Required UX behavior:

- vault list only after unlock
- encrypted summaries decrypted client-side
- local search over decrypted summaries
- add/edit/delete entries
- reveal and copy password
- generator presets
- pair extension flow
- device-session management
- protected-action gating for sensitive flows

Required UI safety rules:

- no third-party analytics or embeds on `/vault`
- no HTML or Markdown rendering for notes
- no remote metadata or favicon fetching
- no storing decrypted data in URL params
- no `dangerouslySetInnerHTML` on vault surfaces

Visual requirements:

- reuse existing Rustyfin theme variables and shared button patterns
- use `.btn-primary` for primary actions
- keep the orange to pink to purple styling
- do not introduce an unrelated design system

## Phase 5: Browser extension MVP

Create a browser extension MVP in:

- `extensions/rustfin-vault-webext/`

Use a modern WebExtensions-compatible structure suitable for Chromium and Firefox where feasible.

Required extension pieces:

- background/service worker
- content script
- popup UI
- options page if needed

Required extension capabilities:

- detect candidate login, signup, and password-change forms
- normalize current page URL/origin/host/base-domain
- honor excluded domains
- request vault lookup matches from Rustyfin
- show matching credentials in the popup
- allow manual user-triggered autofill
- prompt to save new credentials
- prompt to update changed credentials
- insert generated passwords into signup or password-change forms

Required extension defaults:

- page-load autofill off
- HTTP autofill off
- hidden-field fill off
- untrusted-iframe fill off
- explicit save confirmation on

Required extension security behavior:

- content script must not expose secrets through `window`
- do not keep captured credentials after dismissal
- do not design around persistent MV3 service-worker memory
- treat service-worker restart as a lock boundary unless a safe ephemeral mechanism exists

## Phase 6: Import, export, and notes handling

Required import/export behavior:

- import parsing should happen client-side where feasible
- import plaintext must not be sent to the server for parsing in v1
- export should be assembled client-side after local decryption
- export flows must be protected actions
- add file-size and item-count limits
- fail closed on malformed data

Required notes behavior:

- plain text only
- no HTML
- no Markdown rendering in v1

## Phase 7: Testing and verification

You must add or update tests so the implementation is defensible.

Required backend tests:

- repo tests for vault tables
- handler tests for vault endpoints
- protected-action token tests
- device-session rotation and revocation tests
- audit tests

Required adversarial authorization tests:

- user A cannot read user B's vault item by ID
- user A cannot mutate user B's vault item by ID
- user A cannot access user B's lookup results
- user A cannot access user B's sync results
- user A cannot revoke user B's device session
- alternate endpoints behave the same as canonical endpoints for denial

Required crypto tests:

- wrap and unwrap round trips
- wrong key failure
- tampered ciphertext rejection
- AAD mismatch rejection
- version compatibility tests

Required UI tests where practical:

- unlock and lock behavior
- search after unlock
- no plaintext persistence in browser storage
- copy and reveal behavior
- destructive confirmations

Required extension tests where practical:

- login form detection
- SPA rerender handling
- save prompt heuristics
- no fill into hidden fields
- no fill on mismatched origin
- no fill in untrusted iframes
- excluded-domain suppression

## Build and verification commands

Run the relevant verification commands before finalizing substantial work.

At minimum, run:

- `cargo fmt --all`
- `cargo check`
- `cargo test`
- `npm --prefix ui run build`

Also run any additional extension build or test commands you add.

If a command fails, fix the issue rather than just reporting the failure unless you are truly blocked.

## Documentation updates

Update documentation as needed so the repo stays coherent.

If architecture, API, or developer workflow changed materially, update:

- `/Users/iwanteague/Desktop/Rustyfin/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`

At minimum, add documentation for:

- vault feature overview
- trust model caveats
- extension requirement for browser-wide automation
- new environment variables or runtime expectations if introduced

## Deliverable quality bar

Your output is unacceptable if any of the following are true:

- the server can decrypt vault entries during ordinary operation
- the vault master password is sent to the server
- autofill is enabled aggressively by default
- there is no explicit protected-action model
- cross-user authorization tests are missing
- the extension simply reuses the existing browser token without proper device-session treatment
- the implementation adds remote metadata fetching for saved websites
- the implementation adds organization sharing in v1

## Final response requirements

In your final response, provide:

- a concise summary of what was implemented
- exact commands run and their results
- a file-by-file change list
- any remaining gaps relative to the design doc
- any important security notes or tradeoffs

Now do the work. Start by reading the report and auditing the current auth, db, and UI structure, then implement the vault in the correct order. Do not stop at planning.
