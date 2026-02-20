# Rustyfin – Comprehensive Fix Plan (Users/Auth + Media Detection)

This report is grounded in the attached codebase (`Rustyfin-a7d2aca`) and points to concrete files/functions to change. It targets:

1. **Correct auth UX**: no “Login” prompt when logged in, and a **real Logout**.
2. **Hard user separation**: **admins vs non-admins** (UI + API), no accidental admin access.
3. **One user at a time per browser**: a single auth state, consistent token handling.
4. **One account-creation pipeline**: same rules for setup-admin and admin-created users (**12+ chars**).
5. **Library scanning reliability**: detect common video containers (and stop silently accepting broken paths).

---

## 0) Current-State Audit (what’s wrong and where it comes from)

### 0.1 Backend already has role enforcement… but the UI doesn’t respect it
The server has JWT auth and explicit admin-only extractors:

- `crates/server/src/auth.rs`
  - `AuthUser` reads `Authorization: Bearer ...`
  - `AdminUser` rejects non-admins (`403`) if `role != "admin"`

Admin-only routes already exist (e.g. creating libraries, user management), because handlers take `_admin: AdminUser`:
- `crates/server/src/routes.rs`
  - `create_library(_admin: AdminUser, ...)`
  - `create_user_route(_admin: AdminUser, ...)`
  - `list_users_route(_admin: AdminUser, ...)`

So: the backend is **not** “everyone can do everything” – but the **frontend always renders admin UI links**, and the **admin page doesn’t redirect** if you’re not an admin.

### 0.2 Setup wizard creates an admin, but the user is *not* logged in
- Setup admin creation endpoint: `POST /api/v1/setup/admin`
  - `crates/server/src/setup/handlers.rs::create_admin`
  - It returns `{ user_id, setup_state }` and **does not issue a token**.
- UI wizard ends with a “Go to Login” link:
  - `ui/src/app/setup/page.tsx` → `step === 'done'`

So the “Login” button showing afterwards is technically correct, but the UX contradicts expectations.

### 0.3 Password policy mismatch: setup requires 12, admin-created users allow 4
- Setup wizard validation: `crates/server/src/setup/validation.rs::validate_admin`
  - Requires password length **12..=1024**
- Admin-create-user validation: `crates/server/src/routes.rs::create_user_route`
  - Currently: `password.len() < 4` (hardcoded)

This is exactly the “two pipelines doing the same thing” problem.

### 0.4 No logout mechanism
- UI stores JWT in localStorage on login:
  - `ui/src/app/login/page.tsx` → `localStorage.setItem('token', data.token)`
- There is no UI element to remove it, and the navbar is static:
  - `ui/src/app/layout.tsx` always renders `Admin` and `Login` links.

### 0.5 Media scan “doesn’t detect .mp4”
Scanner logic does detect `.mp4` by extension:
- `crates/scanner/src/parser.rs` has `VIDEO_EXTENSIONS` including `"mp4"`
- `crates/scanner/src/walk.rs` calls `parser::is_video_file(&name)`
- `crates/scanner/src/scan.rs` logs `files_found`

The most likely reason users “don’t see media” is **not extension detection**; it’s that Rustyfin currently allows **invalid/unreadable/unmounted library paths** to be stored, and scans quietly skip them:
- `crates/scanner/src/scan.rs`:
  - `if !root.exists() { warn!(... "path does not exist, skipping"); continue; }`
- But `create_library` (admin API) only checks `paths.is_empty()` and does **no filesystem validation**:
  - `crates/server/src/routes.rs::create_library`

This is why users think “it can’t detect mp4”: the scan never touched the directory.

---

## 1) The Fix Strategy (high-level)

### 1.1 Authentication model (keep current JWT header approach, but make it coherent)
**Short-term (fast + consistent):**
- Keep JWT in `localStorage` (as it is today).
- Add a single **Auth Context** in the UI.
- Navbar renders based on auth state (`/api/v1/users/me`).
- Add **Logout** that clears token and resets auth state.
- Add **admin route guard** so `/admin` redirects away for non-admins.

**Security note:** OWASP recommends securing session tokens with cookie flags like `HttpOnly`, `Secure`, and `SameSite` where applicable. If you later move JWT into hardened cookies, follow OWASP session/cookie guidance.  
References: OWASP Session Management Cheat Sheet (cookie attributes) and OWASP JWT cheat sheet series.  
- https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html  
- https://cheatsheetseries.owasp.org/cheatsheets/JSON_Web_Token_for_Java_Cheat_Sheet.html

### 1.2 “One user at a time”
Within a single browser/profile: enforce a single token and a single `AuthContext` state.
- Login overwrites the token (already happens), but we’ll also:
  - clear any existing token before setting a new one, and
  - make the navbar / route guards reflect the current token immediately.

### 1.3 One user creation pipeline (server-side)
Create a shared validation + creation module and reuse it from:
- setup admin creation (`/setup/admin`)
- admin-created users (`/users`)

Password guidance in standards tends to emphasize length; NIST requires at least 8 characters for user-chosen secrets, but Rustyfin’s product requirement can be stricter at 12.  
Reference: NIST 800-63B “Memorized secrets SHALL be at least 8 characters…”  
- https://github.com/usnistgov/800-63-3/blob/nist-pages/sp800-63b/sec5_authenticators.md

---

## 2) Backend Changes (Rust)

### 2.1 Add a shared “user creation pipeline” module

**Add file:** `crates/server/src/user_pipeline.rs`  
**Add export:** `crates/server/src/lib.rs` (`pub mod user_pipeline;`)

#### `crates/server/src/user_pipeline.rs` (new)
```rust
use regex::Regex;
use rustfin_core::error::ApiError;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::LazyLock;

use crate::error::AppError;
use crate::state::AppState;

pub const MIN_PASSWORD_LEN: usize = 12;

static USERNAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9._-]{3,32}$").unwrap());

pub fn validate_username_password(username: &str, password: &str) -> Option<Value> {
    let mut fields = serde_json::Map::new();

    if username.len() < 3 || username.len() > 32 || !USERNAME_RE.is_match(username) {
        fields.insert(
            "username".to_string(),
            json!(["must match ^[a-zA-Z0-9._-]{3,32}$"]),
        );
    }

    if password.len() < MIN_PASSWORD_LEN || password.len() > 1024 {
        fields.insert(
            "password".to_string(),
            json!([format!("must be between {MIN_PASSWORD_LEN} and 1024 characters")]),
        );
    }

    if fields.is_empty() {
        None
    } else {
        Some(Value::Object(fields))
    }
}

pub fn normalize_library_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in ids {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        if seen.insert(id.to_string()) {
            out.push(id.to_string());
        }
    }
    out
}

pub async fn validate_library_ids_exist(
    state: &AppState,
    library_ids: &[String],
) -> Result<(), AppError> {
    for library_id in library_ids {
        let exists = rustfin_db::repo::libraries::get_library(&state.db, library_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .is_some();
        if !exists {
            return Err(ApiError::validation(json!({
                "library_ids": [format!("unknown library id: {library_id}")]
            }))
            .into());
        }
    }
    Ok(())
}

pub async fn create_user_with_access(
    state: &AppState,
    username: &str,
    password: &str,
    role: &str,
    library_ids: &[String],
) -> Result<String, AppError> {
    if let Some(fields) = validate_username_password(username, password) {
        return Err(ApiError::validation(fields).into());
    }

    if role != "admin" && role != "user" {
        return Err(ApiError::validation(json!({
            "role": ["must be 'admin' or 'user'"]
        }))
        .into());
    }

    let library_ids = normalize_library_ids(library_ids);

    if role == "user" && library_ids.is_empty() {
        return Err(ApiError::validation(json!({
            "library_ids": ["user accounts must include at least one library"]
        }))
        .into());
    }

    if role == "admin" && !library_ids.is_empty() {
        return Err(ApiError::validation(json!({
            "library_ids": ["admin users cannot be limited to specific libraries"]
        }))
        .into());
    }

    validate_library_ids_exist(state, &library_ids).await?;

    let id = rustfin_db::repo::users::create_user(&state.db, username, password, role)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if role == "user" {
        rustfin_db::repo::users::set_library_access(&state.db, &id, &library_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    Ok(id)
}
```

#### `crates/server/src/lib.rs` (edit)
```diff
 pub mod auth;
 pub mod error;
 pub mod routes;
 pub mod setup;
 pub mod state;
 pub mod streaming;
+pub mod user_pipeline;
```

### 2.2 Replace duplicated user validation in setup wizard

**Edit file:** `crates/server/src/setup/validation.rs`

Instead of hardcoding username/password rules there, delegate to `user_pipeline`.

```diff
-use regex::Regex;
-use serde_json::{json, Value};
-use std::sync::LazyLock;
-
-static USERNAME_RE: LazyLock<Regex> =
-    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9._-]{3,32}$").unwrap());
+use serde_json::Value;
+use crate::user_pipeline;

 /// Validate create admin request fields. Returns field errors or None.
 pub fn validate_admin(username: &str, password: &str) -> Option<Value> {
-    let mut fields = serde_json::Map::new();
-
-    if username.len() < 3 || username.len() > 32 || !USERNAME_RE.is_match(username) {
-        fields.insert(
-            "username".to_string(),
-            json!(["must match ^[a-zA-Z0-9._-]{3,32}$"]),
-        );
-    }
-
-    if password.len() < 12 || password.len() > 1024 {
-        fields.insert(
-            "password".to_string(),
-            json!(["must be between 12 and 1024 characters"]),
-        );
-    }
-
-    if fields.is_empty() {
-        None
-    } else {
-        Some(Value::Object(fields))
-    }
+    user_pipeline::validate_username_password(username, password)
 }
```

Note: keep `REGION_RE` and other validators in this file; only username/password moves.

### 2.3 Use the pipeline in admin-created user route

**Edit file:** `crates/server/src/routes.rs`  
**Replace** the manual checks in `create_user_route` with a call to `create_user_with_access`.

```diff
 async fn create_user_route(
     _admin: AdminUser,
     State(state): State<AppState>,
     Json(body): Json<CreateUserRequest>,
 ) -> Result<Json<CreateUserResponse>, AppError> {
-    if body.username.is_empty() || body.password.len() < 4 {
-        return Err(ApiError::BadRequest(
-            "username must be non-empty and password at least 4 chars".into(),
-        )
-        .into());
-    }
-    let role = body.role;
-    if role != "admin" && role != "user" {
-        return Err(ApiError::BadRequest("role must be 'admin' or 'user'".into()).into());
-    }
-    let library_ids = normalize_library_ids(&body.library_ids);
-    if role == "user" && library_ids.is_empty() {
-        return Err(
-            ApiError::BadRequest("user accounts must include at least one library".into()).into(),
-        );
-    }
-    if role == "admin" && !library_ids.is_empty() {
-        return Err(ApiError::BadRequest(
-            "admin users cannot be limited to specific libraries".into(),
-        )
-        .into());
-    }
-    validate_library_ids_exist(&state, &library_ids).await?;
-
-    let id = rustfin_db::repo::users::create_user(&state.db, &body.username, &body.password, &role)
-        .await
-        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
-
-    if role == "user" {
-        rustfin_db::repo::users::set_library_access(&state.db, &id, &library_ids)
-            .await
-            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
-    }
+    let role = body.role.clone();
+    let library_ids = crate::user_pipeline::normalize_library_ids(&body.library_ids);
+    let id = crate::user_pipeline::create_user_with_access(
+        &state,
+        &body.username,
+        &body.password,
+        &role,
+        &library_ids,
+    )
+    .await?;

     Ok(Json(CreateUserResponse {
         id,
         username: body.username,
         role: role.clone(),
         library_ids: if role == "user" { library_ids } else { vec![] },
     }))
 }
```

You can also delete the now-duplicated helper functions `normalize_library_ids` and `validate_library_ids_exist` from `routes.rs` once everything compiles, because they are in `user_pipeline.rs`.

### 2.4 Update server tests for new password policy

**Edit file:** `crates/server/tests/integration.rs`

These tests currently use short passwords:
- bootstrapped admin: `"admin123"`
- new user: `"testpass"`

Change them to ≥12 chars **everywhere they appear**.

Example safe replacements:
- `"admin123"` → `"admin_supersecure123"`
- `"testpass"` → `"testpass_supersecure"`

---

## 3) Frontend Changes (Next.js / React)

### 3.1 Introduce a single auth state (AuthProvider)

**Add file:** `ui/src/lib/auth.tsx`
```tsx
'use client';

import React, { createContext, useContext, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { apiJson } from './api';

export type Me = { id: string; username: string; role: 'admin' | 'user' };

type AuthState = {
  me: Me | null;
  loading: boolean;
  refreshMe: () => Promise<void>;
  logout: () => void;
};

const AuthContext = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [me, setMe] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);
  const router = useRouter();

  async function refreshMe() {
    const token = localStorage.getItem('token');
    if (!token) {
      setMe(null);
      return;
    }
    try {
      const data = await apiJson<Me>('/users/me');
      setMe(data);
    } catch {
      // token invalid/expired
      localStorage.removeItem('token');
      setMe(null);
    }
  }

  function logout() {
    localStorage.removeItem('token');
    setMe(null);
    router.push('/login');
  }

  useEffect(() => {
    refreshMe().finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const value = useMemo(() => ({ me, loading, refreshMe, logout }), [me, loading]);
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
```

### 3.2 Add a Providers wrapper for the App Router layout

**Add file:** `ui/src/app/providers.tsx`
```tsx
'use client';

import { AuthProvider } from '@/lib/auth';

export default function Providers({ children }: { children: React.ReactNode }) {
  return <AuthProvider>{children}</AuthProvider>;
}
```

### 3.3 Replace static navbar with role-aware navbar

**Add file:** `ui/src/app/NavBar.tsx`
```tsx
'use client';

import { useAuth } from '@/lib/auth';

export default function NavBar() {
  const { me, loading, logout } = useAuth();

  return (
    <nav className="app-nav animate-rise rounded-2xl px-4 py-3 sm:px-6">
      <div className="flex items-center gap-3 sm:gap-5">
        <a href="/" className="text-2xl font-semibold accent-logo">Rustyfin</a>
        <span className="chip chip-accent hidden md:inline-flex">Home Server Streaming</span>

        <a href="/libraries" className="btn-ghost px-3 py-2 text-sm sm:text-base">Libraries</a>

        {!loading && me?.role === 'admin' && (
          <a href="/admin" className="btn-ghost px-3 py-2 text-sm sm:text-base">Admin</a>
        )}

        <div className="ml-auto flex items-center gap-2">
          {loading ? (
            <span className="text-sm muted">…</span>
          ) : me ? (
            <>
              <span className="chip">{me.username}</span>
              <button onClick={logout} className="btn-secondary px-4 py-2 text-sm">
                Logout
              </button>
            </>
          ) : (
            <a href="/login" className="btn-secondary px-4 py-2 text-sm">Login</a>
          )}
        </div>
      </div>
    </nav>
  );
}
```

### 3.4 Update root layout to use Providers + NavBar

**Edit file:** `ui/src/app/layout.tsx`
```diff
 import type { Metadata } from 'next';
 import './globals.css';
+import Providers from './providers';
+import NavBar from './NavBar';

 export const metadata: Metadata = {
   title: 'Rustfin',
   description: 'Local-first media server',
 };

 export default function RootLayout({ children }: { children: React.ReactNode }) {
   return (
     <html lang="en">
       <body className="min-h-screen text-[var(--text-main)]">
-        <div className="mx-auto max-w-[90rem] px-4 pb-8 pt-5 sm:px-6 lg:px-10">
-          <nav className="app-nav animate-rise rounded-2xl px-4 py-3 sm:px-6">
-            <div className="flex items-center gap-3 sm:gap-5">
-              <a href="/" className="text-2xl font-semibold accent-logo">Rustyfin</a>
-              <span className="chip chip-accent hidden md:inline-flex">Home Server Streaming</span>
-              <a href="/libraries" className="btn-ghost px-3 py-2 text-sm sm:text-base">Libraries</a>
-              <a href="/admin" className="btn-ghost px-3 py-2 text-sm sm:text-base">Admin</a>
-              <div className="ml-auto">
-                <a href="/login" className="btn-secondary px-4 py-2 text-sm">Login</a>
-              </div>
-            </div>
-          </nav>
-          <main className="mx-auto max-w-7xl px-0 py-8 sm:py-10">{children}</main>
-          <footer className="mt-4 px-1 text-xs muted">
-            Local-first media, styled for modern home servers.
-          </footer>
-        </div>
+        <Providers>
+          <div className="mx-auto max-w-[90rem] px-4 pb-8 pt-5 sm:px-6 lg:px-10">
+            <NavBar />
+            <main className="mx-auto max-w-7xl px-0 py-8 sm:py-10">{children}</main>
+            <footer className="mt-4 px-1 text-xs muted">
+              Local-first media, styled for modern home servers.
+            </footer>
+          </div>
+        </Providers>
       </body>
     </html>
   );
 }
```

### 3.5 Admin route guard (no admin page for non-admins)

**Edit file:** `ui/src/app/admin/page.tsx`  
At the top of the component, add:

```tsx
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';

// inside AdminPage()
const router = useRouter();
const { me, loading } = useAuth();

useEffect(() => {
  if (!loading && (!me || me.role !== 'admin')) {
    router.replace('/libraries');
  }
}, [loading, me, router]);

if (loading) {
  return <div className="panel-soft px-5 py-4"><p className="text-sm muted">Checking access…</p></div>;
}

if (!me || me.role !== 'admin') {
  return <div className="panel px-6 py-8"><p className="text-sm muted">Admin access required.</p></div>;
}
```

This guarantees a non-admin cannot “sit” on the admin dashboard.

### 3.6 Make login update AuthContext immediately

**Edit file:** `ui/src/app/login/page.tsx`

- Clear existing token first.
- After storing token, call `refreshMe()` (from AuthContext).

```diff
 import { useState } from 'react';
 import { useRouter } from 'next/navigation';
+import { useAuth } from '@/lib/auth';

 export default function LoginPage() {
   const [username, setUsername] = useState('');
   const [password, setPassword] = useState('');
   const [error, setError] = useState('');
   const router = useRouter();
+  const { refreshMe } = useAuth();

   async function handleSubmit(e: React.FormEvent) {
     e.preventDefault();
     setError('');

     try {
+      localStorage.removeItem('token');
       const res = await fetch('/api/v1/auth/login', {
         method: 'POST',
         headers: { 'Content-Type': 'application/json' },
         body: JSON.stringify({ username, password }),
       });

       if (!res.ok) {
         const body = await res.json().catch(() => ({}));
         setError(body?.error?.message || 'Login failed');
         return;
       }

       const data = await res.json();
       localStorage.setItem('token', data.token);
+      await refreshMe();
       router.push('/libraries');
     } catch {
       setError('Network error');
     }
   }
```

### 3.7 Setup wizard: auto-login after setup completion (optional but matches expectation)

In `ui/src/app/setup/page.tsx`, after `completeSetup()` succeeds, immediately login as the created admin and store token, then redirect to `/libraries`.

Add import:
```tsx
import { useAuth } from '@/lib/auth';
```

Inside `SetupWizard`, grab `refreshMe` and use it after setting token:

```tsx
const { refreshMe } = useAuth();
```

Then update `handleComplete`:
```diff
  const handleComplete = async () => {
    clearErrors();
    setSaving(true);
    try {
      await completeSetup();
      clearOwnerToken();
+     // Auto-login as the admin that was just created in this wizard session.
+     const res = await fetch('/api/v1/auth/login', {
+       method: 'POST',
+       headers: { 'Content-Type': 'application/json' },
+       body: JSON.stringify({ username: adminUsername, password: adminPassword }),
+     });
+     if (res.ok) {
+       const data = await res.json();
+       localStorage.setItem('token', data.token);
+       await refreshMe();
+     }
      setStep('done');
    } catch (err) {
      handleError(err);
    }
    setSaving(false);
  };
```

Also change the “done” view button text to “Go to Libraries” and link to `/libraries`.

### 3.8 Enforce 12-character minimum in the Admin “Create User” form

**Edit file:** `ui/src/app/admin/page.tsx`
Locate the password input for `newUser.password` and add:

```tsx
<input
  type="password"
  minLength={12}
  value={newUser.password}
  onChange={(e) => setNewUser({ ...newUser, password: e.target.value })}
  className="input px-4 py-2.5"
  required
/>
<p className="text-xs muted mt-1">Minimum 12 characters.</p>
```

Do the same for setup admin password fields in `ui/src/app/setup/page.tsx` (add `minLength={12}` and a small hint).

---

## 4) Library Scanning & “Video Type Detection” Fixes

### 4.1 Fix the “silent failure” by validating library paths at creation time

**Why:** Right now `/api/v1/libraries` accepts any string path and stores it. Later, scans skip missing/unreadable paths and users interpret it as “mp4 not detected”.

**Edit file:** `crates/server/src/routes.rs` in `create_library`

Add filesystem validation **before** calling `rustfin_db::repo::libraries::create_library`.

```rust
use std::path::Path;

// inside create_library:
let mut normalized_paths = Vec::new();
for (i, raw) in body.paths.iter().enumerate() {
    let p = raw.trim();
    if p.is_empty() {
        return Err(ApiError::validation(json!({
            format!("paths[{i}]"): ["must not be empty"]
        })).into());
    }
    let path = Path::new(p);

    if !path.is_absolute() {
        return Err(ApiError::validation(json!({
            format!("paths[{i}]"): ["must be an absolute path (no ~ expansion)"]
        })).into());
    }
    if !path.exists() {
        return Err(ApiError::validation(json!({
            format!("paths[{i}]"): ["path does not exist on the server"]
        })).into());
    }
    if !path.is_dir() {
        return Err(ApiError::validation(json!({
            format!("paths[{i}]"): ["path is not a directory"]
        })).into());
    }
    if path.read_dir().is_err() {
        return Err(ApiError::validation(json!({
            format!("paths[{i}]"): ["directory is not readable by the server process"]
        })).into());
    }

    normalized_paths.push(p.to_string());
}
```

Then pass `normalized_paths` to the DB create call (instead of `body.paths`).

This change alone will prevent the “scan found 0 files” confusion and will very likely resolve your immediate “mp4 not detected” report for Docker / mount mistakes.

### 4.2 Expand the video extension allowlist to “top ~20” container formats

The scanner currently supports a reasonable list, but you explicitly want to cover the common cases. MDN’s container list is a good baseline for containers you’re likely to encounter.  
Reference: MDN media container formats.  
- https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Containers

**Edit file:** `crates/scanner/src/parser.rs`  
Replace `VIDEO_EXTENSIONS` with a more comprehensive set (still conservative):

```diff
 static VIDEO_EXTENSIONS: &[&str] = &[
-    "mkv", "mp4", "avi", "m4v", "mov", "wmv", "flv", "webm", "ts", "mpg", "mpeg", "3gp", "ogv",
+    // Common video containers
+    "mp4", "m4v", "mov", "mkv", "webm", "avi",
+    "mpg", "mpeg", "mpe", "mpv",
+    "ts", "m2ts", "mts",
+    "wmv", "asf",
+    "flv", "f4v",
+    "3gp", "3g2",
+    "ogv",
+    "vob",
+    "mxf",
 ];
```

> Note on **AV1/HEVC**: those are codecs, not container extensions. Most AV1/HEVC content appears as `.mp4` or `.mkv`. Supporting the container list above covers those codec cases in practice.

### 4.3 (Optional) Add content sniffing as a fallback (handles misnamed files)

If you want to be robust against missing/wrong extensions, add a secondary “sniff” path:

- Add dependency (workspace or scanner crate):
  - `infer = "0.16"` (or latest compatible)

Then in `walk.rs`, for files that fail `is_video_file(name)`, read a small prefix (e.g., first 8–16 KB) and use `infer` to detect `video/*` MIME types.

This is optional; **path validation** + a solid extension allowlist usually fixes 99% of home-server cases.

### 4.4 Add tests so regressions don’t come back

**Add file:** `crates/scanner/tests/video_extensions.rs`
```rust
use rustfin_scanner::parser::is_video_file;

#[test]
fn recognizes_common_video_extensions() {
    for name in [
        "a.mp4", "b.MKV", "c.mov", "d.m2ts", "e.webm", "f.avi", "g.mpeg", "h.ts"
    ] {
        assert!(is_video_file(name), "should detect {name}");
    }
    assert!(!is_video_file("notes.txt"));
}
```

---

## 5) End-to-End Verification Checklist

### 5.1 User/auth
1. Run setup wizard and create admin with 12+ password.
2. Finish setup:
   - expected: auto-login (if you implement 3.7) and navbar shows username + Logout.
3. Confirm navbar behavior:
   - logged out → shows `Login`, hides `Admin`
   - admin → shows `Admin`, shows `Logout`
   - user → hides `Admin`, shows `Logout`
4. Admin page:
   - login as user, visit `/admin`
   - expected: redirect to `/libraries` and a brief “Admin required” message.
5. Password enforcement:
   - attempt admin-created user with password length 11
   - expected: API `422 validation_failed` and UI shows error.

### 5.2 Media scanning
1. Create a library with a known-good absolute path that is mounted/readable by the server process.
2. Trigger scan.
3. Verify server logs show:
   - `scan found video files` with `files_found > 0`
4. Open library page and verify items appear.

---

## 6) File-by-file Summary

### Backend (Rust)
- **ADD** `crates/server/src/user_pipeline.rs`
- **EDIT** `crates/server/src/lib.rs` (export module)
- **EDIT** `crates/server/src/setup/validation.rs` (delegate username/password validation)
- **EDIT** `crates/server/src/routes.rs` (use pipeline for user creation; validate library paths)
- **EDIT** `crates/server/tests/integration.rs` (update passwords to ≥12)

### Frontend (Next.js)
- **ADD** `ui/src/lib/auth.tsx`
- **ADD** `ui/src/app/providers.tsx`
- **ADD** `ui/src/app/NavBar.tsx`
- **EDIT** `ui/src/app/layout.tsx` (use Providers + NavBar)
- **EDIT** `ui/src/app/login/page.tsx` (refresh auth; clear previous token)
- **EDIT** `ui/src/app/admin/page.tsx` (route guard; minLength 12 for user passwords)
- **EDIT (optional)** `ui/src/app/setup/page.tsx` (auto-login; minLength 12)

### Scanner
- **EDIT** `crates/scanner/src/parser.rs` (extend `VIDEO_EXTENSIONS`)
- **ADD** `crates/scanner/tests/video_extensions.rs`

---

## 7) Notes on “Modern and Quick”
- The **fastest** reliable fix is: shared user pipeline + auth-aware navbar + admin route guard + library path validation.
- Everything above is “modern” in the sense that it is:
  - role-based authorization (server enforced),
  - single auth state in the SPA,
  - consistent validation (no duplicated rules),
  - fail-fast on invalid media paths, instead of silently doing nothing.
