use std::io::{Cursor, Write};
use std::sync::OnceLock;

use serde::Deserialize;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone)]
pub struct RustyVaultWebExtensionInfo {
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

const EXTENSION_ASSETS: &[ExtensionAsset] = &[
    ExtensionAsset {
        archive_path: "manifest.json",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/manifest.json"
        )),
    },
    ExtensionAsset {
        archive_path: "background.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/background.js"
        )),
    },
    ExtensionAsset {
        archive_path: "content.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/content.js"
        )),
    },
    ExtensionAsset {
        archive_path: "options.html",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/options.html"
        )),
    },
    ExtensionAsset {
        archive_path: "options.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/options.js"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.css",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/popup.css"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.html",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/popup.html"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/popup.js"
        )),
    },
    ExtensionAsset {
        archive_path: "shared/api.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/shared/api.js"
        )),
    },
    ExtensionAsset {
        archive_path: "shared/crypto.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/shared/crypto.js"
        )),
    },
    ExtensionAsset {
        archive_path: "shared/policy.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/shared/policy.js"
        )),
    },
    ExtensionAsset {
        archive_path: "README.md",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustyvault-webext/README.md"
        )),
    },
];

static EXTENSION_INFO: OnceLock<Result<RustyVaultWebExtensionInfo, String>> = OnceLock::new();
static EXTENSION_PACKAGE_BYTES: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();

fn manifest() -> Result<ExtensionManifest, String> {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../extensions/rustyvault-webext/manifest.json"
    )))
    .map_err(|error| format!("failed to parse rustyvault extension manifest: {error}"))
}

fn package_filename(version: &str) -> String {
    format!("rustyvault-webext-{version}.zip")
}

fn package_root(version: &str) -> String {
    format!("rustyvault-webext-{version}")
}

fn build_extension_info() -> Result<RustyVaultWebExtensionInfo, String> {
    let manifest = manifest()?;
    Ok(RustyVaultWebExtensionInfo {
        display_name: "RustyVault".to_string(),
        version: manifest.version.clone(),
        package_filename: package_filename(&manifest.version),
        install_mode: INSTALL_MODE.to_string(),
    })
}

fn build_extension_package_bytes() -> Result<Vec<u8>, String> {
    let manifest = manifest()?;
    let root = package_root(&manifest.version);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let cursor = Cursor::new(Vec::<u8>::new());
    let mut archive = zip::ZipWriter::new(cursor);
    for asset in EXTENSION_ASSETS {
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

pub fn web_extension_info() -> Result<RustyVaultWebExtensionInfo, String> {
    match EXTENSION_INFO.get_or_init(build_extension_info) {
        Ok(info) => Ok(info.clone()),
        Err(error) => Err(error.clone()),
    }
}

pub fn web_extension_package_bytes() -> Result<&'static [u8], String> {
    match EXTENSION_PACKAGE_BYTES.get_or_init(build_extension_package_bytes) {
        Ok(bytes) => Ok(bytes.as_slice()),
        Err(error) => Err(error.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn extension_package_contains_manifest_and_readme() {
        let info = web_extension_info().expect("extension info should load");
        let bytes = web_extension_package_bytes().expect("extension package should build");
        assert!(bytes.starts_with(b"PK"));

        let root = package_root(&info.version);
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
        assert!(readme.contains("Load unpacked"));
        assert_eq!(info.package_filename, package_filename(&info.version));
    }
}
