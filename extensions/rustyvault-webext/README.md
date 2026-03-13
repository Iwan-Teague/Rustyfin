# RustyVault WebExtension

This folder contains the RustyVault browser-extension MVP.

Current behavior:

- pairs to Rustyfin using a short-lived pairing code created from `/vault`
- stores only vault device-session tokens in extension storage
- keeps unlocked vault keys in memory only
- detects pages with password fields
- computes blinded lookup hashes locally
- shows manual-fill matches in the popup
- captures login submissions and asks the user to confirm save
- keeps page-load autofill disabled by default
- suppresses save prompts and autofill suggestions on excluded domains
- blocks manual fill on HTTP pages by default unless the user opts into it
- blocks cross-origin iframe fill targets

Current limitations:

- this is an unpacked MVP, not a store-packaged release
- inline save badges are conservative and popup-driven
- page-load autofill is intentionally not implemented in the MVP
- broad host permissions are required because detection runs on arbitrary sites

To load it from Rustyfin:

1. open Rustyfin `/vault`
2. download the extension package from the vault page
3. extract the zip locally
4. open your browser extension developer mode page
5. choose `Load unpacked`
6. select the extracted `rustyvault-webext-*` folder
7. open the extension popup and set the Rustyfin server URL
8. create a pairing code from Rustyfin `/vault`
9. pair the extension, then unlock with the vault master password

To load it directly from the repository during development:

1. open your browser extension developer mode page
2. choose `Load unpacked`
3. select `/Users/iwanteague/Desktop/Rustyfin/extensions/rustyvault-webext`
