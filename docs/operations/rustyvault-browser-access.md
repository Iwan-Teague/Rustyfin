# RustyVault Browser Access

This guide is the supported path for making Rustyfin `/vault` reachable from a browser securely.

## Security Model

Only expose the HTTPS edge.

Keep these listeners internal-only or loopback-only:

- internal UI: `3001`
- backend API: `8096`
- calendar: `8099`
- TMDB agent: `8100`
- YouTube agent: `8101`
- transcription agent: `8102`
- servers agent: `8103`
- PostgreSQL: `5432`

For `/vault`, trusted HTTPS is mandatory. Do not treat browser certificate-warning bypasses or direct backend-port access as supported.

RustyVault web access does not require the browser extension. The web vault uses native browser Argon2id when it is available and falls back to shipped portable runtime assets (`argon2.js` plus `argon2.wasm`) when it is not, but both paths still require a trusted HTTPS browser context.

Vault unlocks must honor the stored wrapped-key KDF parameters and fail fast with a visible UI error if the browser-side crypto path stalls or cannot initialize.

The `/vault` HTML response must keep a CSP that allows the portable Argon2id WebAssembly runtime to initialize. Do not remove the Vault-specific `unsafe-eval` / `wasm-unsafe-eval` allowance from the route headers unless the fallback implementation changes.

## Choose One Access Mode

### Private or VPN-only access

Use this when Rustyfin stays on a private LAN, tailnet, or VPN-only network.

- Keep `RUSTFIN_EDGE_TLS_MODE=manual`
- Use a certificate the browser trusts for the exact browser-visible host
- Browse to the exact HTTPS edge origin, for example:
  - `https://my-host:3000`
  - `https://tailnet-name:3000`

### Public hostname access

Use this when `/vault` must be reachable from arbitrary browsers on the public internet.

- Set `RUSTFIN_PUBLIC_HOST` to the real public hostname
- Set `RUSTFIN_EDGE_TLS_MODE=auto`
- Publish only the edge port
- Let Caddy manage the certificate automatically

Example:

```bash
RUSTFIN_PUBLIC_HOST=vault.example.com
RUSTFIN_EDGE_TLS_MODE=auto
```

## Exact Origin Settings

These values must match what the browser actually uses.

Typical public-hostname example:

```bash
RUSTFIN_PUBLIC_HOST=vault.example.com
RUSTYFIN_BROWSER_BACKEND_ORIGIN=https://vault.example.com
RUSTFIN_WS_ALLOWED_ORIGINS=https://vault.example.com
```

Typical private or VPN example on the default edge port:

```bash
RUSTFIN_PUBLIC_HOST=my-host-or-tailnet-name
RUSTYFIN_BROWSER_BACKEND_ORIGIN=https://my-host-or-tailnet-name:3000
RUSTFIN_WS_ALLOWED_ORIGINS=https://my-host-or-tailnet-name:3000
```

If `RUSTYFIN_BROWSER_BACKEND_ORIGIN` is unset, the installer/runtime now defaults it to the HTTPS edge origin instead of the raw backend port.

## Native Runtime Notes

- `RUSTFIN_EDGE_TLS_MODE=manual` keeps the installer-managed certificate/key path in front of the HTTPS edge port.
- `RUSTFIN_EDGE_TLS_MODE=auto` renders a hostname-based Caddy config for automatic HTTPS. This requires `RUSTFIN_PUBLIC_HOST` to be a real hostname, not an IP address.
- The native edge sends HSTS once it is serving HTTPS.
- Native health checks follow the configured browser origin, so hostname-based access does not depend on local hairpin DNS.

## Recommended Flow

1. Run the supported Linux install flow with `./scripts/install_linux.sh`.
2. Decide whether the vault is private/VPN-only or publicly hosted.
3. Set `RUSTFIN_PUBLIC_HOST` to the real browser-visible host.
4. Keep `RUSTYFIN_BROWSER_BACKEND_ORIGIN` and `RUSTFIN_WS_ALLOWED_ORIGINS` aligned to that exact HTTPS origin.
5. Use `RUSTFIN_EDGE_TLS_MODE=manual` for trusted local/private certificates, or `RUSTFIN_EDGE_TLS_MODE=auto` for a public hostname with automatic HTTPS.
6. Expose only the HTTPS edge.
7. Browse to `/vault` through that HTTPS origin.

## Non-goals

These are not supported vault publication models:

- direct browser access to `8096`
- exposing internal service ports directly
- plain HTTP vault access
- relying on a browser certificate warning bypass as the normal path
