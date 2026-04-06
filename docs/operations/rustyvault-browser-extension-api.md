# RustyVault Browser Extension API Inventory

This is the canonical route inventory used by the RustyVault browser extension.

All routes are served under `/api/v1/vault` on the browser-visible Rustyfin HTTPS origin.

## Auth model

- Pairing uses plain Rustyfin HTTPS plus a short-lived pairing code.
- After pairing, the extension stores RustyVault device-session tokens.
- Session-bound extension calls send `x-rustyvault-access`.
- The master password is never sent to the server.

## Routes

### `POST /device-sessions/pair/consume`

Consumes a pairing code created from `/vault` and returns device-session tokens for the browser extension.

Used for:

- first-time extension pairing

### `POST /device-sessions/refresh`

Refreshes the short-lived access token using the revocable refresh token.

Used for:

- normal extension session refresh
- recovery from expired access tokens

### `GET /config`

Returns RustyVault bootstrap/config data, including the active wrapped key metadata and item count.

Used for:

- unlock bootstrap
- vault-available checks

### `GET /preferences`

Returns RustyVault preferences.

Used for:

- default URI match mode
- HTTP / iframe warning policy
- excluded domains
- inline fill and save prompt preferences
- password generator defaults

### `POST /lookup`

Looks up matching encrypted item summaries by blinded URI hashes.

Used for:

- current-site inline suggestions
- popup current-site matches
- save/update classification context

### `GET /items/{id}`

Returns one encrypted vault item.

Used for:

- decrypting a selected item before manual or inline fill
- fetching the current revision before update/add-site writes

### `POST /items`

Creates a new encrypted vault item.

Used for:

- save-new-login flow
- save-generated-signup flow

### `PUT /items/{id}`

Replaces an existing encrypted vault item.

Used for:

- update-existing-login flow
- add-site-to-existing-login flow

### `GET /device-sessions`

Lists active device sessions.

Used for:

- future extension session management UI
- `/vault` session review

### `DELETE /device-sessions/{id}`

Revokes one device session.

Used for:

- explicit extension revocation

### `POST /audit`

Not currently consumed as a direct extension route.

The extension currently relies on the existing RustyVault server-side audit event recording inside item and pairing handlers. New direct audit-ingest routes should only be added if a concrete extension consumer needs them.

## Constraints

- HTTPS only
- browser origin must match Rustyfin’s configured browser-visible origin exactly
- cross-user Rustyfin auth plus RustyVault session mixing remains rejected by the host
- extension writes must keep using the existing item CRUD routes; do not create a parallel extension-only vault API namespace
