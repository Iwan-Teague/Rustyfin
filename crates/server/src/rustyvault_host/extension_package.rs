use rustfin_core::error::ApiError;
use rustyvault::extension_package::{
    RustyVaultWebExtensionInfo, RustyVaultWebExtensionTarget, web_extension_info,
    web_extension_package_bytes,
};

use crate::error::AppError;

pub fn extension_info(
    target: RustyVaultWebExtensionTarget,
) -> Result<RustyVaultWebExtensionInfo, AppError> {
    web_extension_info(target).map_err(|error| AppError::from(ApiError::Internal(error)))
}

pub fn extension_package_bytes(
    target: RustyVaultWebExtensionTarget,
) -> Result<&'static [u8], AppError> {
    web_extension_package_bytes(target).map_err(|error| AppError::from(ApiError::Internal(error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    fn package_root(version: &str, target: RustyVaultWebExtensionTarget) -> String {
        format!("rustyvault-webext-{}-{version}", target.browser_family())
    }

    #[test]
    fn extension_package_contains_manifest_and_readme() {
        for target in [
            RustyVaultWebExtensionTarget::Chromium,
            RustyVaultWebExtensionTarget::Firefox,
        ] {
            let info = extension_info(target).expect("extension info should load");
            let bytes = extension_package_bytes(target).expect("extension package should build");
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
        }
    }
}
