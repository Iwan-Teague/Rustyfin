# Rustyfin Linux Bootstrap Installer Design

Date: 2026-03-14
Status: proposed design

## 1. Goal

Design a one-shot Linux installer that can take a fresh machine from "generic Linux host" to "Rustyfin installed, configured, and running" with as little manual work as possible.

Target outcome:

- detect the host OS, version, architecture, init system, and package manager
- install the packages Rustyfin depends on into locations Rustyfin already expects
- detect GPU hardware and choose sane Rustyfin defaults for AI, transcription, and transcoding
- provision database, service users, directories, config files, and `systemd` units
- build or install Rustyfin artifacts
- start the stack and verify health
- leave behind a machine-readable install manifest and human-readable summary

This document is about the installer architecture, reuse of current code, known gaps, and the safest implementation path.

Deployment assumption for this design:

- the installer is primarily for operator-managed installs, not random consumer desktops
- a valid target includes blank Debian or Ubuntu systems where installing the Rust toolchain first is acceptable
- longer term, Rustyfin may be sold as bundled hardware or an SD-card image, so appliance-style reproducibility matters more than zero-dependency bootstrap purity

## 2. Current Reusable Pieces

The repository already contains a strong Debian-native install path. The new Linux installer should reuse these behaviors rather than replace them.

### 2.1 Current scripts that already work

- `scripts/install_native_debian.sh`
  - installs supported Debian packages with `apt`
  - installs Rust via `rustup`
  - installs `yt-dlp`
  - ensures PostgreSQL is running
  - creates/updates the `rustfin` PostgreSQL role and database
  - installs a managed Java 21 runtime for Minecraft when needed
- `scripts/start-native.sh`
  - validates required tools
  - now consumes `rustfin-installer plan-native-runtime` for runtime ports, media path, DB URL, and browser/websocket origin planning
  - now consumes `rustfin-installer build-native-runtime-artifacts` for AI backend selection, Rust binary builds, UI dependency state, and Next standalone builds
  - now consumes `rustfin-installer launch-native-runtime` for native process launch, pid/log management, optional picker-helper startup, and startup health checks
  - now consumes `rustfin-installer write-native-runtime-snapshot` for persisted runtime snapshot output
  - is now mostly an env/default loader and compatibility wrapper
- `scripts/install_native_systemd.sh`
  - now acts as a compatibility wrapper around `rustfin-installer install-native-systemd`
- `scripts/deploy-native.sh`
  - now acts as a compatibility wrapper around `rustfin-installer deploy-native`
- `scripts/build_linux_binaries.sh`
  - now acts as a compatibility wrapper around `rustfin-installer build-native-binaries`
  - the target selection, zigbuild policy, and server/transcription feature-aware build orchestration now live in Rust
- `scripts/stop-native.sh`
  - now acts as a compatibility wrapper around `rustfin-installer stop-native-runtime`
- `scripts/clean_install.sh`
  - now keeps only the interactive confirmation prompt and hands destructive reset behavior to `rustfin-installer clean-native-runtime`
- `scripts/rustfin-installer.sh`
  - thin wrapper to run `rustfin-installer` from the repo through `cargo run --locked`

### 2.2 Runtime logic already present in Rust

- `crates/server/src/main.rs`
  - PostgreSQL-only enforcement
  - DB migrations on startup
  - AI model directory resolution
  - transcoder hardware-acceleration probing
- `crates/transcription-agent/src/main.rs`
  - validates GPU backend readiness for transcription
  - checks device nodes and runtime tools like `clinfo`, `nvidia-smi`, and `rocminfo`

### 2.3 Operational docs already present

- `docs/operations/debian-12-native-runtime.md`
  - defines the current supported-Debian native runtime model

## 3. Hard Constraints From The Current Project

The new installer cannot ignore the project as it exists today.

- Rustyfin is Linux-only for this effort.
- Rustyfin runtime is native, not Docker.
- PostgreSQL is mandatory.
- `systemd` is the expected service supervisor.
- Rustyfin currently assumes host-installed tools such as `ffmpeg`, `ffprobe`, `caddy`, `node`, `npm`, and `cargo`.
- AI backend support is chosen at build time for the server.
- transcription GPU support is chosen at build time for the transcription agent.
- Minecraft management currently expects Java 21 available at `/opt/rustyfin/java/current/bin/java`.

Important consequence:

The realistic target is not literally "every Linux distribution." The installer should target **systemd-based, glibc Linux hosts** first. Alpine, NixOS, and other materially different environments should be explicitly unsupported until separately designed.

## 4. Recommended Support Policy

### 4.1 Tiered distro support

Recommended first support matrix:

- Tier 1
  - Debian 12
  - Debian 13
  - Ubuntu LTS
- Tier 2
  - Fedora
  - Rocky / Alma / RHEL-compatible
  - Arch
  - openSUSE Tumbleweed / Leap

Explicitly unsupported in the first installer version:

- Alpine / musl-first systems
- NixOS
- non-`systemd` hosts
- WSL-style partial Linux environments

This keeps the promise realistic while still covering the Linux hosts most people actually use.

### 4.2 Why a support policy matters

Package names, driver stacks, SELinux behavior, filesystem layouts, and service integration differ enough across Linux families that pretending they are the same will produce a brittle installer.

## 5. What The Current Project Does Not Yet Cover

The existing Debian scripts are good, but they are not yet a universal Linux bootstrap.

### 5.1 OS/package-manager abstraction is missing

Current state:

- only `apt-get` is implemented
- package names are Debian-specific

Missing:

- distro-family detection
- per-distro package name maps
- repo enablement for packages not in base repos

### 5.2 Dedicated install layout is not standardized

Current state:

- runtime caches, logs, and binaries are heavily repo-relative
- main runtime uses the invoking user instead of a dedicated `rustyfin` account

Missing:

- canonical install root such as `/opt/rustyfin`
- canonical config root such as `/etc/rustyfin`
- canonical data root such as `/var/lib/rustyfin`
- canonical log root such as `/var/log/rustyfin`
- canonical cache root such as `/var/cache/rustyfin`

### 5.3 GPU stack installation is not automated

Current state:

- AI backend selection only checks for tools like `nvcc`, `rocminfo`, or `vulkaninfo`
- transcription validates runtime GPU availability but does not install drivers
- transcoder runtime can probe `ffmpeg` hardware acceleration, but the installer does not provision driver stacks

Missing:

- vendor detection for NVIDIA, AMD, and Intel GPUs
- distro-specific driver installation
- post-driver reboot handling
- multi-GPU inventory and persistence
- consistent mapping from detected hardware to Rustyfin defaults

### 5.4 Security/config persistence is split

Current state:

- some values are generated dynamically at startup
- some values live in `/etc/rustyfin/servers-agent.env`
- installer-owned runtime defaults now live in `/etc/rustyfin/native-runtime.defaults.sh`
- runtime snapshots still live in `.rustyfin.runtime.env`

Missing:

- one authoritative persisted config model for install-time values
- stable secret generation and storage layout
- install manifest for audit/debugging

### 5.5 Linux host differences are not fully handled

Missing examples:

- SELinux policy handling on Fedora/RHEL
- `firewalld` / `ufw` rules
- `sudo` vs root-only operation policy
- package repo refresh semantics
- `dnf` modular streams / third-party repos
- Arch package naming and rolling release drift

### 5.6 Artifact acquisition is still repo-centric

Current state:

- scripts assume a Rustyfin checkout already exists

Missing:

- install from git URL or release tarball
- version pinning
- upgrade channel selection
- rollback path

## 6. Recommended Installer Architecture

### 6.1 High-level recommendation

For Rustyfin’s actual deployment model, the better design is a **Rust-first installer with a very thin bootstrap shell layer only where necessary**.

Recommended public entrypoints:

- `scripts/install_linux.sh`
  - minimal bootstrap for supported distros
  - installs Rust toolchain if missing
  - invokes the Rust installer
- `cargo run -p rustfin-installer -- ...`
  - development/operator path
- later: a shipped installer binary such as `/opt/rustyfin/bin/rustfin-installer`
  - ideal for SD-card or appliance-style deployments

Initial implementation note:

- `scripts/install_linux.sh` now exists as the bootstrap entrypoint
- `crates/installer` now exists as the canonical Rust installer crate
- the current Debian 12/13 install path already owns prerequisite installation policy in Rust, including package installation, native-user Rust provisioning, `yt-dlp`, PostgreSQL bootstrap, managed Java 21 provisioning, installer-written native runtime defaults, native runtime planning, runtime TLS/token/snapshot persistence, native Linux binary build orchestration, native runtime artifact builds for Rust services plus the Next standalone UI, native runtime launch/stop orchestration, native clean-reset behavior, native deploy orchestration, direct `systemd` install/refresh, and install-manifest output
- native build/start still delegate to the proven `start-native.sh` flow

Recommended repo shape:

- `crates/installer/`
  - canonical installer state machine and orchestration logic
- `crates/installer/src/distro/`
  - distro adapters for Debian/Ubuntu, Fedora/RHEL, Arch, openSUSE
- `crates/installer/src/gpu/`
  - GPU inventory and Rustyfin feature selection
- `crates/installer/src/layout/`
  - filesystem layout and ownership logic
- `crates/installer/src/config/`
  - persisted config and manifest generation
- `crates/installer/src/systemd/`
  - unit rendering and service installation
- `crates/installer/src/health/`
  - post-install validation

### 6.2 Why Rust should own the installer

Rust is the better primary language here because the hard part of this installer is not running package managers. The hard part is making a large number of install decisions correctly and reproducibly.

Rust advantages in this project:

- typed install manifests and persisted config
- explicit state machine for idempotent install phases
- safer distro/GPU decision logic than a large shell script
- easier unit/integration testing for detection and policy logic
- clearer upgrade and rollback semantics
- better long-term maintainability as Rustyfin grows new services and install variables
- better fit for appliance-style deployments where the installer may ship as a product component

### 6.3 Where shell still belongs

Shell should remain, but only as a narrow bootstrap/integration layer:

- install Rust toolchain when missing
- invoke package managers
- provide a tiny compatibility entrypoint on blank hosts

Best long-term shape:

- Rust owns install decisions, manifests, validation, and orchestration
- shell only bridges into host package-manager commands and first-run bootstrap

## 7. Proposed Install Flow

### Phase A: host preflight

The installer should:

- read `/etc/os-release`
- determine distro family and version
- verify `systemd` is present
- verify architecture (`x86_64` or `aarch64`)
- verify enough disk space and write permissions
- verify network access for package repos and Rustyfin source/artifacts

### Phase B: layout and service user

Recommended standard layout:

- app root: `/opt/rustyfin`
- config: `/etc/rustyfin`
- mutable data: `/var/lib/rustyfin`
- cache: `/var/cache/rustyfin`
- logs: `/var/log/rustyfin`
- managed Java: `/opt/rustyfin/java/current`

Recommended service account:

- create a dedicated `rustyfin` system user and group

This is better than binding the install to whichever human user happened to run the script.

### Phase C: package installation

The installer should install:

- build toolchain
- Rust toolchain
- Node/npm
- PostgreSQL server/client
- Caddy
- `ffmpeg` / `ffprobe`
- SSL/TLS tools
- Python runtime for `yt-dlp`
- GPU runtime tools where relevant

The installer must use a package abstraction layer rather than hardcoding Debian package names.

### Phase D: GPU discovery and policy selection

The installer should inventory:

- GPU vendor(s)
- GPU count
- detected device nodes
- available runtime tools
- usable media acceleration backends

Suggested detection sources:

- `lspci`
- `/dev/dri/renderD*`
- `/dev/nvidia*`
- `nvidia-smi`
- `rocminfo`
- `clinfo`
- `vulkaninfo`

Outputs should drive defaults for:

- `RUSTFIN_AI_GPU_BACKEND`
- `RUSTFIN_TRANSCRIPTION_GPU_MODE`
- `RUSTFIN_TRANSCRIPTION_REQUIRE_GPU`
- `RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES`
- `RUSTFIN_TRANSCODER_HW_ACCEL`
- `RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL`

### Phase E: database/bootstrap secrets

The installer should:

- install/start PostgreSQL
- create or update the Rustyfin role/database
- generate persistent secrets
- write a canonical env/config file under `/etc/rustyfin`

Suggested persisted values:

- DB URL
- JWT secret
- agent tokens
- public host
- media roots
- GPU-mode selections
- service ports

### Phase F: source or artifact deployment

The installer should support one of these modes:

- git checkout mode
- release tarball mode

Recommendation:

- start with git checkout mode for development
- add release artifact mode before calling the installer "production-ready"

### Phase G: build/install/start

Reuse current logic where possible:

- keep `scripts/build_linux_binaries.sh` only as a compatibility wrapper
- keep `scripts/rustfin-installer.sh` as the compatibility wrapper for installer subcommands
- keep `scripts/deploy-native.sh` only as a compatibility wrapper
- keep `scripts/start-native.sh`, `scripts/stop-native.sh`, `scripts/install_native_systemd.sh`, and `scripts/clean_install.sh` thin
- keep any remaining shell logic focused on bootstrap, compatibility, or interactive confirmation only

The new installer should call lower-level build/package primitives, but the installer policy and orchestration should live in Rust rather than being spread across shell scripts.

### Phase H: post-install validation

The installer should finish by checking:

- package/tool presence
- DB connectivity
- migration success
- AI model dir exists
- services are active
- `/health` endpoints are green
- UI responds

It should then write:

- `/var/lib/rustyfin/install-manifest.json`
- `/var/log/rustyfin/install-summary.txt`

## 8. GPU Handling Strategy

GPU handling is the hardest part of a cross-distro installer.

### 8.1 What should be automatic

- hardware inventory
- runtime tool detection
- choosing Rustyfin env defaults
- choosing build feature defaults
- warning when a reboot is needed

### 8.2 What should be conditional

- installing NVIDIA drivers
- installing ROCm packages
- installing OpenCL runtimes
- enabling Vulkan tools

These should be done only when:

- the distro is explicitly supported
- the vendor is positively identified
- the required package set is known

### 8.3 What should not be blindly automated

- replacing an existing driver stack
- purging vendor drivers
- changing BIOS/IOMMU settings
- enabling Secure Boot signing flows

Those operations are too risky for a generic "run once and trust it" installer.

### 8.4 Multi-GPU expectations

The installer should record:

- GPU count
- vendor per GPU
- which GPU class was selected for AI
- which GPU class was selected for transcription
- which GPU class was selected for transcoding

That inventory should go into the manifest so later support and upgrades remain explainable.

## 9. Variables Likely To Change Over Rustyfin’s Life

The installer design needs to expect drift over time.

### 9.1 Package drift

- package names by distro
- Node major version requirements
- PostgreSQL package naming/version
- `ffmpeg` feature availability
- Caddy packaging or repo setup

### 9.2 Hardware/runtime drift

- AI backend support may expand or shrink
- transcription GPU backend defaults may change
- transcoder hardware acceleration priorities may change
- Java version requirements for hosted game servers will change

### 9.3 Product/runtime drift

- new Rustyfin services may be added
- service ports may change
- install-time secrets/config may grow
- additional agent processes may need their own units/tokens
- setup wizard requirements may evolve

### 9.4 OS policy drift

- SELinux/AppArmor defaults
- package signing policies
- systemd unit hardening expectations
- repo enablement for third-party packages

Installer design implication:

all package and policy data should be isolated behind versioned adapter files rather than buried inside one huge script.

## 10. Gaps To Close Before Building The Universal Installer

Recommended prerequisite work:

1. standardize runtime/config/data/log paths away from repo-relative defaults
2. introduce a dedicated `rustyfin` service account model
3. centralize persisted install/runtime config under `/etc/rustyfin`
4. define the Rust installer crate structure and install state machine
5. create distro package maps and command adapters
6. define GPU support policy by distro/vendor
7. define install manifest schema
8. define upgrade and rollback behavior

## 11. `.sh` Versus `.rs`

### 11.1 Shell-only approach

Pros:

- best bootstrap compatibility
- no Rust prerequisite
- easiest package-manager orchestration
- simplest privilege escalation

Cons:

- weaker structured data handling
- harder to test large decision trees cleanly
- more brittle for complex manifest logic
- harder to keep reproducible as the install matrix grows

### 11.2 Rust-only approach

Pros:

- better structure and testability
- stronger typed config/manifests
- easier future extension
- better fit for productized/appliance deployments
- easier to make the installer itself a supportable product component

Cons:

- Rust toolchain must exist before installer logic can run
- still needs shell or subprocess wrappers for package managers and `systemd`
- slightly more bootstrap ceremony on a blank host

### 11.3 Recommended approach

Use a **hybrid**, but keep Rust as the canonical installer:

- a tiny shell bootstrap installs Rust if required and hands off immediately
- the Rust installer binary owns detection, policy, manifests, service setup, and health validation

Given the intended audience and deployment model, this is the best balance of reliability, maintainability, and product quality.

## 12. Proposed First Implementation Scope

The first version should not try to solve every distro and GPU case at once.

Recommended scope:

- Debian 12, Debian 13, and Ubuntu LTS first
- Rust-first installer crate with a minimal bootstrap shell
- dedicated install layout
- dedicated `rustyfin` system user
- persistent config under `/etc/rustyfin`
- GPU inventory plus default selection
- no forced driver installation outside known-safe distro paths
- full post-install health validation

Then expand to Fedora/RHEL, Arch, and openSUSE with Rust distro adapters.

## 13. Acceptance Criteria

The installer is "good enough" when a fresh supported Linux host can do this:

1. fetch Rustyfin
2. run one install command as root or through `sudo`
3. wait for completion
4. open the reported HTTPS URL
5. reach Rustyfin setup/login without hand-editing files

Secondary acceptance criteria:

- rerunning the installer is idempotent
- upgrades reuse the same config/state layout
- the install manifest clearly shows what the installer decided
- unsupported hosts fail fast with a precise message

## 14. Recommended Next Step

Before writing the full universal installer, do one extraction pass:

- define `crates/installer` and its install-state model
- preserve the current Debian-native shell scripts as reference behavior
- move host detection, config layout, manifest generation, and service rendering into Rust first
- keep a minimal `bootstrap_linux.sh` only for first-run toolchain/package-manager handoff

That gives a controlled path from "works on Debian shell scripts" to "works through a real installer product" without losing the proven native flow.
