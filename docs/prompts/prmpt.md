You are an autonomous senior full-stack engineer working inside this repository. Your job is to implement—fully, end-to-end—the fixes described in the file `docs/reports/Rustyfin_Fixes_Report.md` (in the repo/workspace). You MUST complete the whole job in one run: no TODOs, no placeholders, no “left as an exercise”, no partial refactors. If something breaks tests/build, you fix it. If you discover mismatches vs the report, you still deliver the intent of the report with clean, modern code.

Non-negotiable constraints:
- Keep backend in Rust, frontend in modern Next.js/React (App Router). No legacy patterns.
- Do not introduce duplicate validation logic: create ONE shared server-side user creation/validation pipeline and reuse it for BOTH the setup wizard admin creation and admin-created users.
- Users must be segregated by role (admin vs user) with server-side enforcement AND frontend UX enforcement.
- Auth state must be singular (only one logged-in user per browser profile): login overwrites old auth; logout clears auth.
- Implement a real Logout UX (and ensure navbar reflects logged-in status).
- Fix media detection/scanning so common video container extensions are detected and libraries fail fast if paths are invalid/unreadable.
- You must run formatting + tests + builds and ensure green.
- You must output a final summary with: commands run, tests passed, and a file-by-file change list.

High-level tasks (implement ALL; treat “optional” items in the report as REQUIRED unless impossible):
1) BACKEND (Rust)
   A. Implement shared user creation pipeline
      - Add `crates/server/src/user_pipeline.rs` implementing:
        - MIN_PASSWORD_LEN = 12
        - validate_username_password(username, password) -> Option<serde_json::Value> with consistent rules
        - normalize_library_ids(ids) -> Vec<String>
        - validate_library_ids_exist(state, library_ids) -> Result<(), AppError>
        - create_user_with_access(state, username, password, role, library_ids) -> Result<String, AppError>
      - Export the module from `crates/server/src/lib.rs`.

   B. Remove duplicated admin setup validation
      - Update `crates/server/src/setup/validation.rs` to delegate username/password validation to `user_pipeline::validate_username_password` (keep other setup validations intact).

   C. Enforce the same password rules everywhere (12+ chars)
      - Update `crates/server/src/routes.rs::create_user_route` to use the pipeline (no inline checks like “>=4 chars”).
      - Ensure API responses use the existing error/validation style (ApiError::validation JSON field errors).

   D. Update tests to comply with the new minimum password length
      - Update `crates/server/tests/integration.rs` (and any other test fixtures) so every test password is >= 12 chars.

   E. Fix library path validation (fail fast at creation time)
      - Update `crates/server/src/routes.rs::create_library` to validate every provided path:
        - trimmed non-empty
        - absolute path
        - exists
        - is_dir
        - readable by the server process (read_dir ok)
      - Return field-level validation errors (ApiError::validation JSON) pointing at `paths[i]`.
      - Store normalized paths (trimmed) in DB.

2) FRONTEND (Next.js / React)
   A. Create a single source of truth for auth state
      - Add `ui/src/lib/auth.tsx` with an AuthProvider + `useAuth()` hook.
      - AuthProvider must:
        - read JWT from localStorage key `token`
        - fetch current user via `/api/v1/users/me` (use existing api helpers if present; otherwise implement a small helper that adds Authorization header)
        - expose { me, loading, refreshMe, logout }
        - logout removes token, clears state, routes to /login

   B. Wire provider into app layout and replace static navbar with role-aware navbar
      - Add `ui/src/app/providers.tsx` and wrap the app in it from `ui/src/app/layout.tsx`.
      - Add `ui/src/app/NavBar.tsx` that:
        - shows Login ONLY when logged out
        - shows username + Logout when logged in
        - shows Admin link ONLY when me.role === "admin"
        - never shows Admin button for non-admins

   C. Fix login flow so it updates UI immediately and enforces “one user at a time”
      - Edit `ui/src/app/login/page.tsx`:
        - clear any existing token before setting the new one
        - after login success: store token, call refreshMe(), navigate to /libraries

   D. Enforce admin-only access to the admin page (frontend guard)
      - Edit `ui/src/app/admin/page.tsx`:
        - if loading, show a “checking access” state
        - if not admin (or not logged in), redirect to /libraries (router.replace)
        - also render a fallback message if briefly visible
      - Also enforce minLength={12} on the create-user password field, plus a hint.

   E. Setup wizard UX (make logged-in state match expectation)
      - Edit `ui/src/app/setup/page.tsx`:
        - enforce minLength={12} on admin password input
        - after setup completes, auto-login the created admin (call /api/v1/auth/login with the wizard credentials), store token, refreshMe, and route to /libraries (or ensure the “done” button goes to /libraries and navbar reflects state).
      - Ensure the navbar no longer shows Login when the admin is actually logged in.

3) SCANNER / MEDIA DETECTION
   A. Expand recognized video container extensions
      - Update `crates/scanner/src/parser.rs` VIDEO_EXTENSIONS list to include a robust set of common containers:
        mp4, m4v, mov, mkv, webm, avi, mpg, mpeg, mpe, mpv, ts, m2ts, mts, wmv, asf, flv, f4v, 3gp, 3g2, ogv, vob, mxf
      - Keep logic case-insensitive.

   B. Add regression tests
      - Add `crates/scanner/tests/video_extensions.rs` that asserts several common names (including uppercase extensions) are detected, and a non-video file is not.

4) VERIFICATION / QUALITY BAR (mandatory)
   A. Before changes: run and note baseline (even if failing)
      - `cargo test` (workspace)
      - `cargo fmt --all -- --check` (or run fmt at end if check fails)
      - `cargo clippy --workspace --all-targets -- -D warnings` (fix warnings)
      - For UI: detect package manager by lockfile (pnpm-lock.yaml / yarn.lock / package-lock.json) and run:
        - install
        - lint/build (`npm run build` or equivalent)
   B. After changes: run the same suite and ensure everything is green.
   C. Ensure the API still enforces admin-only access server-side (AdminUser extractor remains in use).
   D. Ensure the UI never shows Admin link to non-admins AND /admin redirects away when non-admin.

5) ACCEPTANCE CRITERIA (must all be true)
- After setup wizard completes, the admin is effectively logged in (navbar shows username + Logout, not Login).
- Logout exists and works (token removed, navbar updates, protected areas behave correctly).
- Logging in as a non-admin user:
  - cannot see Admin link in navbar
  - cannot access /admin (redirects to /libraries)
  - server endpoints requiring AdminUser return 403 with non-admin token
- Creating a user via admin UI requires password length >= 12, and backend enforces it too.
- Library creation rejects invalid/unreadable paths with clear field-level errors (paths[i]).
- Scanner recognizes common container extensions and test coverage proves it.
- All tests/builds pass; code is formatted; no clippy warnings.

6) DELIVERABLE OUTPUT (in your final response)
- A concise file-by-file list of what you changed/added.
- The exact commands you ran (tests/build/fmt/clippy) and their results.
- Any important behavioral notes (e.g., setup now auto-logins).

Now do the work. Read `docs/reports/Rustyfin_Fixes_Report.md` first, then implement everything above. No TODOs. No shortcuts. Make it production-clean.
