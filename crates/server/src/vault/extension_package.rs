use std::io::{Cursor, Write};
use std::sync::OnceLock;

use rustfin_core::error::ApiError;
use rustfin_core::vault::VaultExtensionInfoResponse;
use serde::Deserialize;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::error::AppError;

const DOWNLOAD_PATH: &str = "/api/v1/vault/extension/package";
const INSTALL_MODE: &str = "download_zip_extract_then_load_unpacked";

#[derive(Debug, Clone, Deserialize)]
struct ExtensionManifest {
    version: String,
}

struct ExtensionAsset {
    archive_path: &'static str,
    contents: &'static [u8],
}

const EXTENSION_ASSETS: &[ExtensionAsset] = &[
    ExtensionAsset {
        archive_path: "manifest.json",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/manifest.json"
        )),
    },
    ExtensionAsset {
        archive_path: "background.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/background.js"
        )),
    },
    ExtensionAsset {
        archive_path: "content.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/content.js"
        )),
    },
    ExtensionAsset {
        archive_path: "options.html",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/options.html"
        )),
    },
    ExtensionAsset {
        archive_path: "options.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/options.js"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.css",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/popup.css"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.html",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/popup.html"
        )),
    },
    ExtensionAsset {
        archive_path: "popup.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/popup.js"
        )),
    },
    ExtensionAsset {
        archive_path: "shared/api.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/shared/api.js"
        )),
    },
    ExtensionAsset {
        archive_path: "shared/crypto.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/shared/crypto.js"
        )),
    },
    ExtensionAsset {
        archive_path: "shared/policy.js",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/shared/policy.js"
        )),
    },
    ExtensionAsset {
        archive_path: "README.md",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/rustfin-vault-webext/README.md"
        )),
    },
];

static EXTENSION_INFO: OnceLock<Result<VaultExtensionInfoResponse, String>> = OnceLock::new();
static EXTENSION_PACKAGE_BYTES: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();

fn manifest() -> Result<ExtensionManifest, String> {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../extensions/rustfin-vault-webext/manifest.json"
    )))
    .map_err(|error| format!("failed to parse vault extension manifest: {error}"))
}

fn package_filename(version: &str) -> String {
    format!("rustyfin-vault-webext-{version}.zip")
}

fn package_root(version: &str) -> String {
    format!("rustyfin-vault-webext-{version}")
}

fn build_extension_info() -> Result<VaultExtensionInfoResponse, String> {
    let manifest = manifest()?;
    Ok(VaultExtensionInfoResponse {
        name: "Rustyfin Vault".to_string(),
        version: manifest.version.clone(),
        package_filename: package_filename(&manifest.version),
        download_path: DOWNLOAD_PATH.to_string(),
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
            .map_err(|error| format!("failed to start vault extension archive entry: {error}"))?;
        archive
            .write_all(asset.contents)
            .map_err(|error| format!("failed to write vault extension archive entry: {error}"))?;
    }
    archive
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| format!("failed to finalize vault extension archive: {error}"))
}

pub fn extension_info() -> Result<VaultExtensionInfoResponse, AppError> {
    match EXTENSION_INFO.get_or_init(build_extension_info) {
        Ok(info) => Ok(info.clone()),
        Err(error) => Err(ApiError::Internal(error.clone()).into()),
    }
}

pub fn extension_package_bytes() -> Result<&'static [u8], AppError> {
    match EXTENSION_PACKAGE_BYTES.get_or_init(build_extension_package_bytes) {
        Ok(bytes) => Ok(bytes.as_slice()),
        Err(error) => Err(ApiError::Internal(error.clone()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn extension_package_contains_manifest_and_readme() {
        let info = extension_info().expect("extension info should load");
        let bytes = extension_package_bytes().expect("extension package should build");
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
        assert!(manifest_json.contains("\"name\": \"Rustyfin Vault\""));

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
