# 2026-04-02 First-Install Dependency Audit Follow-Up

This document captures the first-install findings from the native Rustyfin installer examination so the work can be resumed later without redoing the audit.

## Scope Examined

- `/Users/iwanteague/Desktop/Rustyfin/scripts/install_linux.sh`
- `/Users/iwanteague/Desktop/Rustyfin/scripts/install_native_debian.sh`
- `/Users/iwanteague/Desktop/Rustyfin/scripts/start-native.sh`
- `/Users/iwanteague/Desktop/Rustyfin/scripts/run-native-supervisor.sh`
- `/Users/iwanteague/Desktop/Rustyfin/scripts/run-native-post-healthcheck.sh`
- `/Users/iwanteague/Desktop/Rustyfin/crates/installer/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/installer/src/distro/mod.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/installer/src/distro/debian.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/installer/src/distro/ubuntu.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/installer/src/utils.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/ui/package.json`

## What First Install Already Does Well

- `./scripts/install_linux.sh` bootstraps base host packages, installs Rust when needed, and hands off to the Rust installer.
- The Rust installer covers the main native runtime path:
  - distro package installation
  - GPU support hooks
  - native-user Rust toolchain setup
  - `yt-dlp`
  - PostgreSQL bootstrap and configuration
  - managed Java 21
  - native runtime defaults
  - starter AI model seeding attempt
  - native artifact builds
  - `systemd` unit installation
  - post-install startup validation
- Runtime assets are written into sensible locations such as:
  - `/etc/rustyfin/native-runtime.defaults.sh`
  - `/etc/rustyfin/servers-agent.env`
  - `/var/lib/rustyfin/install-manifest.json`
  - `/var/lib/rustyfin/ai/models`
  - `/opt/rustyfin/java/current`
  - repo-local `.rustyfin.runtime.env`
  - repo-local `.native-bins/...`
  - `ui/.next/standalone`
- The UI build path already handles dependency installation with `npm ci` when the lockfile hash changes.
- Native `systemd` validation is not silent; it checks backend, agents, and HTTPS UI readiness before reporting success.

## High-Priority Gaps

1. `RUSTFIN_JWT_SECRET` persistence is not guaranteed by the installer/runtime snapshot.
   - The server generates a random secret when this value is missing.
   - Result: first install can come up healthy but session stability across restart/reboot is not guaranteed.

2. Starter AI model seeding is best-effort, not hard-guaranteed.
   - The installer attempts to seed a starter GGUF into the active AI model directory.
   - Certain download failures are logged and tolerated.
   - Result: a nominally successful first install may still leave `/ai` without a usable local model.

3. The transcription model is lazily downloaded on first use instead of during install.
   - Result: first install does not guarantee that speech/transcription assets are already present on disk.

4. Transcription defaults still assume a working GPU path.
   - The native defaults lean toward GPU/OpenCL and can reject work if that runtime is not actually usable.
   - Result: first install can pass health checks while `/ai` voice fallback remains unavailable.

5. The installer does not enforce a specific Node.js version before building the Next.js UI.
   - Result: UI build success still depends on the distro `nodejs` package being sufficiently modern.

## Lower-Priority Gaps

- Documentation is stricter than the installer implementation on supported host platforms.
  - Repo guidance says native Debian 12/13 is the supported target.
  - The installer code still contains Ubuntu adapters.
  - Result: support posture and runtime reality are not perfectly aligned.

## Follow-Up Checklist

- Persist a stable `RUSTFIN_JWT_SECRET` during first install and deploy/update flows.
- Decide whether starter AI model availability is mandatory or explicitly best-effort, then align installer behavior and docs.
- Decide whether Whisper/transcription assets should be preseeded on install or remain lazy by design, then document that clearly.
- Revisit transcription defaults so first install is usable on hosts without a working GPU stack.
- Add a hard Node.js version gate before UI build.
- Align the supported-platform documentation with the actual installer support policy.

## Bottom Line

The current first-install path is mostly sound for core packages, layout, and runtime bootstrap, but it is not yet accurate to say that every dependency, model, and runtime prerequisite is guaranteed to be installed and ready for first use on every supported host.
