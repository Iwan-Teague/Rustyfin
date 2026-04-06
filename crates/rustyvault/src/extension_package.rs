use std::io::{Cursor, Write};
use std::sync::OnceLock;

use serde::Deserialize;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RustyVaultWebExtensionTarget {
    Chromium,
    Firefox,
}

impl RustyVaultWebExtensionTarget {
    pub fn artifact_id(self) -> &'static str {
        match self {
            Self::Chromium => "rustyvault-webext-chromium",
            Self::Firefox => "rustyvault-webext-firefox",
        }
    }

    pub fn browser_family(self) -> &'static str {
        match self {
            Self::Chromium => "chromium",
            Self::Firefox => "firefox",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Chromium => "RustyVault for Chromium Browsers",
            Self::Firefox => "RustyVault for Firefox",
        }
    }

    fn package_extension(self) -> &'static str {
        match self {
            Self::Chromium => "zip",
            Self::Firefox => "xpi",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RustyVaultWebExtensionInfo {
    pub artifact_id: String,
    pub browser_family: String,
    pub display_name: String,
    pub version: String,
    pub package_filename: String,
    pub install_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ExtensionManifest {
    version: String,
}

struct ExtensionAsset {
    archive_path: &'static str,
    contents: &'static [u8],
}

const INSTALL_MODE: &str = "download_zip_extract_then_load_unpacked";

const CHROMIUM_ASSETS: &[ExtensionAsset] = &[
    ExtensionAsset {
        archive_path: "manifest.json",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/manifest.json"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.html",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/popup.html"
        )),
    },
    ExtensionAsset {
        archive_path: "options.html",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/options.html"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.css",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/popup.css"
        )),
    },
    ExtensionAsset {
        archive_path: "README.md",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/README.md"
        )),
    },
    ExtensionAsset {
        archive_path: "src/background/index.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/background/index.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/content/index.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/content/index.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/options/index.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/options/index.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/popup/index.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/popup/index.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/api.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/api.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/argon2-browser.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/argon2-browser.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/browser.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/browser.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/crypto.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/crypto.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/messages.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/messages.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/policy.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/policy.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/save-classifier.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/save-classifier.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/storage.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/storage.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/types.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/types.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/vendor/argon2-bundled.min.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/vendor/argon2-bundled.min.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/vendor/argon2.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/vendor/argon2.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/vendor/argon2.wasm",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/src/shared/vendor/argon2.wasm"
        )),
    },
];

const FIREFOX_ASSETS: &[ExtensionAsset] = &[
    ExtensionAsset {
        archive_path: "manifest.json",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/manifest.json"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.html",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/popup.html"
        )),
    },
    ExtensionAsset {
        archive_path: "options.html",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/options.html"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.css",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/popup.css"
        )),
    },
    ExtensionAsset {
        archive_path: "README.md",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/README.md"
        )),
    },
    ExtensionAsset {
        archive_path: "src/background/index.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/background/index.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/content/index.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/content/index.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/options/index.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/options/index.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/popup/index.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/popup/index.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/api.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/api.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/argon2-browser.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/argon2-browser.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/browser.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/browser.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/crypto.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/crypto.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/messages.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/messages.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/policy.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/policy.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/save-classifier.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/save-classifier.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/storage.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/storage.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/types.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/types.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/vendor/argon2-bundled.min.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/vendor/argon2-bundled.min.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/vendor/argon2.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/vendor/argon2.js"
        )),
    },
    ExtensionAsset {
        archive_path: "src/shared/vendor/argon2.wasm",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/src/shared/vendor/argon2.wasm"
        )),
    },
];

static CHROMIUM_INFO: OnceLock<Result<RustyVaultWebExtensionInfo, String>> = OnceLock::new();
static CHROMIUM_PACKAGE_BYTES: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();
static FIREFOX_INFO: OnceLock<Result<RustyVaultWebExtensionInfo, String>> = OnceLock::new();
static FIREFOX_PACKAGE_BYTES: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();

fn manifest(target: RustyVaultWebExtensionTarget) -> Result<ExtensionManifest, String> {
    let raw = match target {
        RustyVaultWebExtensionTarget::Chromium => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/chromium/manifest.json"
        )),
        RustyVaultWebExtensionTarget::Firefox => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/dist/firefox/manifest.json"
        )),
    };
    serde_json::from_str(raw)
        .map_err(|error| format!("failed to parse rustyvault extension manifest: {error}"))
}

fn package_filename(version: &str, target: RustyVaultWebExtensionTarget) -> String {
    format!(
        "rustyvault-webext-{}-{version}.{}",
        target.browser_family(),
        target.package_extension()
    )
}

fn package_root(version: &str, target: RustyVaultWebExtensionTarget) -> String {
    format!("rustyvault-webext-{}-{version}", target.browser_family())
}

fn assets_for_target(target: RustyVaultWebExtensionTarget) -> &'static [ExtensionAsset] {
    match target {
        RustyVaultWebExtensionTarget::Chromium => CHROMIUM_ASSETS,
        RustyVaultWebExtensionTarget::Firefox => FIREFOX_ASSETS,
    }
}

fn build_extension_info(
    target: RustyVaultWebExtensionTarget,
) -> Result<RustyVaultWebExtensionInfo, String> {
    let manifest = manifest(target)?;
    Ok(RustyVaultWebExtensionInfo {
        artifact_id: target.artifact_id().to_string(),
        browser_family: target.browser_family().to_string(),
        display_name: target.display_name().to_string(),
        version: manifest.version.clone(),
        package_filename: package_filename(&manifest.version, target),
        install_mode: INSTALL_MODE.to_string(),
    })
}

fn build_extension_package_bytes(target: RustyVaultWebExtensionTarget) -> Result<Vec<u8>, String> {
    let manifest = manifest(target)?;
    let root = package_root(&manifest.version, target);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let cursor = Cursor::new(Vec::<u8>::new());
    let mut archive = zip::ZipWriter::new(cursor);
    for asset in assets_for_target(target) {
        archive
            .start_file(format!("{root}/{}", asset.archive_path), options)
            .map_err(|error| {
                format!("failed to start rustyvault extension archive entry: {error}")
            })?;
        archive.write_all(asset.contents).map_err(|error| {
            format!("failed to write rustyvault extension archive entry: {error}")
        })?;
    }
    archive
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| format!("failed to finalize rustyvault extension archive: {error}"))
}

pub fn web_extension_info(
    target: RustyVaultWebExtensionTarget,
) -> Result<RustyVaultWebExtensionInfo, String> {
    let cell = match target {
        RustyVaultWebExtensionTarget::Chromium => &CHROMIUM_INFO,
        RustyVaultWebExtensionTarget::Firefox => &FIREFOX_INFO,
    };
    match cell.get_or_init(|| build_extension_info(target)) {
        Ok(info) => Ok(info.clone()),
        Err(error) => Err(error.clone()),
    }
}

pub fn web_extension_package_bytes(
    target: RustyVaultWebExtensionTarget,
) -> Result<&'static [u8], String> {
    let cell = match target {
        RustyVaultWebExtensionTarget::Chromium => &CHROMIUM_PACKAGE_BYTES,
        RustyVaultWebExtensionTarget::Firefox => &FIREFOX_PACKAGE_BYTES,
    };
    match cell.get_or_init(|| build_extension_package_bytes(target)) {
        Ok(bytes) => Ok(bytes.as_slice()),
        Err(error) => Err(error.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn extension_packages_contain_manifest_and_readme_for_all_targets() {
        for target in [
            RustyVaultWebExtensionTarget::Chromium,
            RustyVaultWebExtensionTarget::Firefox,
        ] {
            let info = web_extension_info(target).expect("extension info should load");
            let bytes =
                web_extension_package_bytes(target).expect("extension package should build");
            assert!(bytes.starts_with(b"PK"));

            let root = package_root(&info.version, target);
            let mut archive =
                zip::ZipArchive::new(Cursor::new(bytes)).expect("zip archive should open");

            let mut manifest_json = String::new();
            archive
                .by_name(&format!("{root}/manifest.json"))
                .expect("manifest should exist in the package")
                .read_to_string(&mut manifest_json)
                .expect("manifest should read");
            assert!(manifest_json.contains("\"manifest_version\": 3"));

            let mut readme = String::new();
            archive
                .by_name(&format!("{root}/README.md"))
                .expect("readme should exist in the package")
                .read_to_string(&mut readme)
                .expect("readme should read");
            assert!(readme.contains("dist/chromium") || readme.contains("dist/firefox"));
            assert_eq!(info.artifact_id, target.artifact_id());
        }
    }
}
