# Rustyfin Impeccable Comprehensive Coverage Playbook

This runbook is the end-to-end process for using Impeccable skills to perform a full UX/design quality sweep of Rustyfin.

Use this when you want systematic coverage across all major surfaces, not a one-off polish.

## 0) Latest execution check-off (current campaign)

This section records the most recent full run of this playbook and marks what has been completed.

### 0.1 Setup and pre-flight

- [x] Section 2 one-time setup verified (`CLAUDE.md` with `## Design Context` exists and is populated).
- [x] Section 3.1 dependency/bootstrap checks run (`git --no-pager status --short`, `npm --prefix ui ci`, `npm --prefix tests ci`).
- [x] Section 3.2 baseline snapshot run.
  - `cargo fmt --all`: pass
  - `cargo check`: pass
  - `cargo test`: fail in this environment (`rustfin_test` database missing for metadata test defaults)
  - `npm --prefix ui run lint`: pass (warnings only)
  - `npm --prefix ui run build`: pass
  - `node --test extensions/rustyvault-webext/shared/policy.test.js`: pass
- [ ] Section 3.3 runtime bring-up attempted (partial).
  - `./scripts/start-native.sh` hit shared-host/runtime conflicts (servers-agent port already in use / runtime already active).
  - Health endpoints were verified against the already-running runtime instead of replacing it.

### 0.2 Coverage map execution

- [x] Section 5.1 shared foundation pass completed.
  - Completed edits in `ui/src/app/globals.css`, `ui/src/app/layout.tsx`, `ui/src/app/NavBar.tsx`, shared components.
- [x] Section 5.2 core routes completed in this run.
  - Route-level updates were applied across the full list, including rooms, channels, player, calendar, network, servers, vault, and RustyVault feature surfaces.
- [x] Section 5.3 extension surfaces completed in this run.
  - Completed: `popup.html`, `popup.css`, `popup.js`, `options.html`, `options.js`, `content.js`, `background.js`.

### 0.3 Final confidence gates

- [x] Section 7 command bundle executed.
  - `cargo fmt --all` / `cargo check`: pass
  - `cargo test`: fails in current environment due missing `rustfin_test` DB default
  - UI lint/build + extension policy tests: pass
- [ ] `./scripts/ci/debian_native_gates.sh --allow-non-debian --skip-runtime --skip-browser-smoke` executed (partial pass).
  - Overall: fail (1 gate)
  - Failing gate: `No Docker runtime files remain`
  - Cause in this environment: `.tmp/docker-compose.hwaccel.auto.yml` exists
  - Report: `.tmp/gates/debian-native-gates-20260313T234401Z.md`
- [x] `./scripts/ci/debian_browser_smoke.sh`: pass (`1 passed` Playwright smoke test).
- [x] `./scripts/ci/rustyvault_removability_gates.sh`: pass.

### 0.4 Definition-of-done status for this campaign

- [ ] Section 8 is partially satisfied.
  - Most implementation and UI/extension validation steps were completed.
  - Section 7 did not fully pass due environment/runtime constraints noted above.

## 1) Operating model (what runs where)

All shell commands in this document are run from repository root:

```bash
cd /home/server/docker/Rustyfin
```

Use three working panes:

- **Pane A (shell):** runtime/build/test commands
- **Pane B (Copilot CLI chat):** skill prompts and implementation requests
- **Pane C (optional shell):** logs/status (`git`, runtime log tails, report paths)

## 2) One-time setup (already completed once per project)

Run once to establish persistent design context:

```text
Use the skill tool to invoke the "teach-impeccable" skill, then follow the skill's instructions.
```

Expected persistent output:

- project-root `CLAUDE.md` contains `## Design Context`

Current Rustyfin Design Context baseline:

- Users: friends/family on a home server, social media continuity first
- Brand: cinematic, confident, technical
- Direction: dark mode, keep orange -> pink -> purple gradient
- Accessibility baseline: WCAG 2.1 AA

## 3) Pre-flight and baseline commands

Run these in **Pane A** before making design changes.

### 3.1 Dependency/bootstrap checks

```bash
git --no-pager status --short
npm --prefix ui ci
npm --prefix tests ci
```

### 3.2 Baseline quality snapshot

```bash
cargo fmt --all
cargo check
cargo test
npm --prefix ui run lint
npm --prefix ui run build
node --test extensions/rustyvault-webext/shared/policy.test.js
```

### 3.3 Runtime bring-up for manual UX validation

```bash
./scripts/start-native.sh
```

Runtime stop:

```bash
./scripts/stop-native.sh
```

Native runtime logs and pids:

- `.tmp/native-runtime/logs/`
- `.tmp/native-runtime/pids/`

## 4) Impeccable skill command library (copy/paste templates)

Use these in **Pane B**. Replace `<scope>` and objective text each time.

### 4.1 Foundation skills

```text
Use the skill tool to invoke the "audit" skill for <scope>. Return severity-ranked issues across accessibility, performance, theming consistency, and responsive behavior, then implement all high/medium fixes following CLAUDE.md Design Context.
```

```text
Use the skill tool to invoke the "critique" skill for <scope>. Evaluate hierarchy, information architecture, and emotional tone, then implement the recommended improvements.
```

```text
Use the skill tool to invoke the "normalize" skill for <scope>. Align spacing, typography, and component usage to existing Rustyfin patterns and shared tokens.
```

```text
Use the skill tool to invoke the "polish" skill for <scope>. Perform final alignment/spacing/consistency cleanup and implement.
```

### 4.2 UX clarity and structure

```text
Use the skill tool to invoke the "clarify" skill for <scope>. Improve labels, helper text, errors, and microcopy to reduce ambiguity while keeping the cinematic technical tone.
```

```text
Use the skill tool to invoke the "distill" skill for <scope>. Remove visual/interaction clutter and keep only high-value controls.
```

```text
Use the skill tool to invoke the "extract" skill for <scope>. Consolidate repeated UI patterns into reusable components/tokens without changing behavior.
```

```text
Use the skill tool to invoke the "onboard" skill for <scope>. Improve first-run/empty-state/onboarding guidance and implement.
```

### 4.3 Visual direction and motion

```text
Use the skill tool to invoke the "colorize" skill for <scope>. Apply color intentionally while preserving Rustyfin's dark palette and orange -> pink -> purple accent identity.
```

```text
Use the skill tool to invoke the "bolder" skill for <scope>. Increase visual impact safely while maintaining usability.
```

```text
Use the skill tool to invoke the "quieter" skill for <scope>. Reduce visual aggression and improve focus hierarchy.
```

```text
Use the skill tool to invoke the "animate" skill for <scope>. Add purposeful micro-interactions using existing shared animation patterns.
```

```text
Use the skill tool to invoke the "delight" skill for <scope>. Introduce subtle personality moments without harming clarity or speed.
```

### 4.4 Robustness/performance/responsiveness

```text
Use the skill tool to invoke the "adapt" skill for <scope>. Improve behavior and layout across phone/tablet/desktop breakpoints.
```

```text
Use the skill tool to invoke the "harden" skill for <scope>. Address edge cases, long text, overflow, empty/error/loading states, and robustness concerns.
```

```text
Use the skill tool to invoke the "optimize" skill for <scope>. Improve render/load performance and remove unnecessary UI cost.
```

### 4.5 Net-new UI surfaces

```text
Use the skill tool to invoke the "frontend-design" skill to create or redesign <surface> in production-ready code that matches Rustyfin's Design Context and existing component conventions.
```

## 5) Full-project coverage map

Use this exact order so shared foundations are stabilized before route-level work.

### 5.1 Shared foundation (do first)

1. `ui/src/app/globals.css`
2. `ui/src/app/layout.tsx`
3. `ui/src/app/NavBar.tsx`
4. `ui/src/app/components/**`

Recommended skill chain:

`audit -> critique -> normalize -> polish`

### 5.2 Core product routes

1. `ui/src/app/page.tsx` (home; prioritize Continue Watching + joinable rooms visibility)
2. `ui/src/app/rooms/page.tsx`
3. `ui/src/app/rooms/[roomId]/page.tsx`
4. `ui/src/app/channels/page.tsx`
5. `ui/src/app/player/[id]/page.tsx`
6. `ui/src/app/items/[id]/page.tsx`
7. `ui/src/app/libraries/page.tsx`
8. `ui/src/app/libraries/[id]/page.tsx`
9. `ui/src/app/calendar/page.tsx`
10. `ui/src/app/network/page.tsx`
11. `ui/src/app/servers/page.tsx`
12. `ui/src/app/downloads/page.tsx`
13. `ui/src/app/account/page.tsx`
14. `ui/src/app/admin/page.tsx`
15. `ui/src/app/login/page.tsx`
16. `ui/src/app/setup/page.tsx`
17. `ui/src/app/vault/page.tsx`
18. `ui/src/features/rustyvault/RustyVaultPage.tsx`

Recommended skill chain per route:

`critique -> clarify -> distill -> adapt -> harden -> polish`

Use as needed:

- `colorize` when hierarchy lacks emphasis
- `bolder`/`quieter` for intensity tuning
- `animate`/`delight` after structure is stable
- `optimize` when route has heavy content/rendering

### 5.3 Extension surfaces (Vault)

1. `extensions/rustyvault-webext/popup.html`
2. `extensions/rustyvault-webext/popup.css`
3. `extensions/rustyvault-webext/popup.js`
4. `extensions/rustyvault-webext/options.html`
5. `extensions/rustyvault-webext/options.js`
6. `extensions/rustyvault-webext/content.js`
7. `extensions/rustyvault-webext/background.js`

Recommended chain:

`audit -> clarify -> harden -> polish`

## 6) Execution loop for each scope

For each scope in Section 5:

1. Run one or more skill prompts in **Pane B** and implement.
2. Validate quickly in **Pane A**:

```bash
npm --prefix ui run build
```

3. If extension code changed, also run:

```bash
node --test extensions/rustyvault-webext/shared/policy.test.js
```

4. Manually verify in browser against running native stack (`https://<host>:3000`).
5. Check diff hygiene:

```bash
git --no-pager status --short
git --no-pager diff --stat
```

6. Move to next scope only after current scope builds and visually behaves as intended.

## 7) Final confidence gates (mandatory for comprehensive pass)

Run at the end of campaign in **Pane A**:

```bash
cargo fmt --all
cargo check
cargo test
npm --prefix ui run lint
npm --prefix ui run build
node --test extensions/rustyvault-webext/shared/policy.test.js
```

If running on Debian 12 runtime host:

```bash
./scripts/ci/debian_native_gates.sh
./scripts/ci/debian_browser_smoke.sh
```

If not on Debian 12 and you still need a broad confidence pass:

```bash
./scripts/ci/debian_native_gates.sh --allow-non-debian --skip-runtime
```

If your changes touch RustyVault availability/removability paths:

```bash
./scripts/ci/rustyvault_removability_gates.sh
```

## 8) Definition of done for a true comprehensive pass

A pass is complete only when all are true:

1. Shared foundation and every scope in Section 5 has gone through the skill loop.
2. Home page clearly surfaces resume state and available rooms to join.
3. Design remains dark, cinematic, confident, technical, and gradient-consistent.
4. No surface feels cramped/clustered or excessively sparse.
5. All commands in Section 7 pass.
6. `CLAUDE.md` Design Context remains accurate after final outcomes.

## 9) Suggested campaign cadence

Recommended sequencing for each area:

1. `audit` (find objective problems)
2. `critique` (improve UX architecture)
3. `clarify` and `distill` (reduce confusion and clutter)
4. `adapt` and `harden` (responsive and edge-case resilience)
5. `colorize` / `bolder` / `quieter` (visual tuning)
6. `animate` / `delight` (micro-interaction pass)
7. `normalize` and `polish` (final consistency pass)
8. `optimize` (if area is heavy)
9. `extract` (if repetition appears)

Use `frontend-design` for new surfaces that do not yet exist.
