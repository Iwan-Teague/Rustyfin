# Rustyfin Linux Install Audit Second Pass Execution

Source audit: `/Users/iwanteague/Downloads/rustyfin_linux_install_audit_second_pass.md`

This document tracks the concrete cleanup carried out from that audit inside the repository.

## Decision

Direct `rustfin-installer install` remains an internal or advanced path.

The single public first-time Linux install entrypoint is:

```bash
./scripts/install_linux.sh
```

The Rust installer remains the owned implementation surface behind that wrapper, but user-facing guidance now treats the shell bootstrapper as the only first-time Linux install command.

## Required Cleanup Checklist

- [x] Remove `install_native_debian.sh` from the README as a primary install step.
- [x] Convert `install_native_debian.sh` into a shim.
- [x] Update `start-native.sh` dependency errors to reference `install_linux.sh`.
- [x] Update `start-native.sh` host wording from Debian-only to Debian 12/13 plus Ubuntu 22.04/24.04.
- [x] Update `rustfin-installer` usage/help strings so the support matrix is consistent.
- [x] Decide that direct `rustfin-installer install` remains an internal path and document that decision.
- [x] In `run-native-supervisor.sh`, recompute `RUNTIME_ROOT` and `PID_DIR` after sourcing `.rustyfin.runtime.env`.
- [x] Add an explicit Debian GPU note about required non-free repository components.
- [x] Add a note that the Whisper model is lazy-downloaded on first transcription use.
- [x] Add a note or guard around direct-root installs so the native build user is explicit.

## Implemented Changes

### Legacy installer collapse

- `scripts/install_native_debian.sh` is now a deprecation shim that emits a warning and delegates directly to `scripts/install_linux.sh`.

### Public installer language cleanup

- `README.md` now documents `./scripts/install_linux.sh` as the only first-time Linux install command.
- The old host-dependencies step using `./scripts/install_native_debian.sh` is no longer presented as a primary path.
- `AGENTS.md` now treats `install_native_debian.sh` as a deprecated compatibility shim rather than a primary install step.
- `docs/operations/debian-12-native-runtime.md` now treats `install_native_debian.sh` as a deprecated compatibility shim instead of a real manual installer path.
- `docs/README.md` now indexes this execution document.

### Support matrix and runtime wording cleanup

- `scripts/start-native.sh` now points Linux users to `./scripts/install_linux.sh` and uses Debian 12/13 plus Ubuntu 22.04/24.04 wording in its Linux-only error text.
- `crates/installer/src/main.rs` now uses the explicit Debian 12, Debian 13, Ubuntu 22.04, Ubuntu 24.04 support string in help and unsupported-host errors.
- `crates/installer/src/main.rs` now points PostgreSQL bootstrap failures at `./scripts/install_linux.sh` rather than the deprecated Debian installer.

### Runtime directory override fix

- `scripts/run-native-supervisor.sh` now recomputes runtime-root derived paths after each `.rustyfin.runtime.env` source.
- `scripts/start-native.sh` now also recomputes runtime-root derived paths after loading installer defaults and `.rustyfin.runtime.env`, so persisted `RUSTFIN_NATIVE_RUNTIME_DIR` overrides remain coherent in both direct-start and supervisor-managed flows.

### Debian GPU and Whisper notes

- `README.md` and `docs/operations/debian-12-native-runtime.md` now explicitly note:
  - Debian CUDA installs require the appropriate `non-free` / `non-free-firmware` repository components.
  - The Whisper transcription model is lazy-downloaded on first transcription use and is not pre-fetched during install.

### Direct-root install guard

- `scripts/install_linux_complete.sh` now makes the native build user explicit when launched directly as `root`:
  - it prefers `RUSTFIN_NATIVE_USER`
  - then `SUDO_USER`
  - then the repo owner when that owner is non-root
  - and emits an explicit warning if it still has to use `root`

## Notes

- The Rust-side distro package maps still include distro `caddy` / `nodejs` / `npm` packages for the internal direct prerequisite path. That is acceptable under the current decision because the public first-time Linux install contract is the shell bootstrapper plus `--skip-prereqs`, not raw `rustfin-installer install`.
- This cleanup intentionally did not broaden support beyond the currently documented Debian 12, Debian 13, Ubuntu 22.04, and Ubuntu 24.04 Linux install flow.
