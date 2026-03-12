# Rustyfin Vault WebExtension

This folder contains the Rustyfin Vault browser-extension MVP.

Current behavior:

- pairs to Rustyfin using a short-lived pairing code created from `/vault`
- stores only vault device-session tokens in extension storage
- keeps unlocked vault keys in memory only
- detects pages with password fields
- computes blinded lookup hashes locally
- shows manual-fill matches in the popup
- captures login submissions and asks the user to confirm save
- keeps page-load autofill disabled by default

Current limitations:

- this is an unpacked MVP, not a store-packaged release
- inline save badges are conservative and popup-driven
- page-load autofill is intentionally not implemented in the MVP
- broad host permissions are required because detection runs on arbitrary sites

To load it:

1. open your browser extension developer mode page
2. choose `Load unpacked`
3. select `/Users/iwanteague/Desktop/Rustyfin/extensions/rustfin-vault-webext`
4. open the extension popup and set the Rustyfin server URL
5. create a pairing code from Rustyfin `/vault`
6. pair the extension, then unlock with the vault master password
