# RustyVault WebExtension

This folder contains the built RustyVault browser-extension project.

Current product behavior:

- pairs to Rustyfin using a short-lived pairing code created from `/vault`
- stores only revocable device-session tokens persistently
- keeps unlocked vault keys in memory only
- requests site access at runtime instead of requiring blanket host access up front
- shows inline credential suggestions on granted sites
- keeps popup-based manual fill as a fallback
- captures login submissions conservatively and classifies them into save, update, or add-site actions
- offers inline save and update prompts after likely successful login or password-change flows
- offers generated-password insertion on signup and password-change pages using RustyVault preference defaults
- suppresses prompts and fill on excluded domains
- blocks manual fill on HTTP pages by default unless the user opts in
- blocks cross-origin iframe fill targets

Build targets:

- `dist/chromium` for Chrome, Edge, Brave, and other Chromium-family browsers
- `dist/firefox` for Firefox desktop packaging

Build locally:

```bash
node ./scripts/build.mjs
```

Build one target only:

```bash
BROWSER=chromium node ./scripts/build.mjs
BROWSER=firefox node ./scripts/build.mjs
```

To load from Rustyfin Downloads:

1. Open Rustyfin `/downloads`
2. Download the Chromium or Firefox RustyVault package
3. Extract the package locally
4. For Chromium-family browsers:
   - open the browser extensions page
   - enable developer mode
   - choose `Load unpacked`
   - select the extracted `rustyvault-webext-chromium-*` folder
5. For Firefox:
   - open `about:debugging#/runtime/this-firefox`
   - choose `Load Temporary Add-on`
   - select the extracted manifest or XPI contents
6. Open the extension popup and set the Rustyfin server URL
7. Create a pairing code from Rustyfin `/vault`
8. Pair the extension, unlock it with the vault master password, then grant site access when you first use it

To load directly from the repository during development:

1. run the build step above
2. load `extensions/rustyvault-webext/dist/chromium` or `extensions/rustyvault-webext/dist/firefox`
