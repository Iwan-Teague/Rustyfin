# Rustyfin First-Party Clients

This directory contains the first-party client applications for Rustyfin.
These clients are implemented as "thin shells" around the hosted Rustyfin web interface,
focusing on OS integration (system tray, media keys, notifications) rather than reimplementing the UI.

## Strategies

### Desktop (Windows, macOS, Linux)
- **Path**: `clients/desktop`
- **Framework**: Tauri (v2)
- **Strategy**: 
  - Render the configured host URL in a WebView.
  - Inject host API tokens into the native layer for OS integration.
  - Handle deep links and window management natively.

### Mobile (Android, iOS)
- **Path**: `clients/mobile`
- **Framework**: Capacitor
- **Strategy**:
  - Render the configured host URL in a WebView.
  - Bridge native capabilities (background audio, notifications) to the web app.

## Distribution
Release artifacts are managed via the host's `/downloads` catalog (Task 2A).
- Desktop builds are uploaded as binaries/installers.
- Android builds are uploaded as APKs.
- iOS distribution is handled via App Store/TestFlight links in the catalog.
