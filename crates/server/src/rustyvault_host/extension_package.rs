use rustfin_core::error::ApiError;
use rustyvault::extension_package::RustyVaultWebExtensionInfo;

use crate::error::AppError;

pub fn extension_info() -> Result<RustyVaultWebExtensionInfo, AppError> {
    rustyvault::extension_package::web_extension_info()
        .map_err(|error| AppError::from(ApiError::Internal(error)))
}

pub fn extension_package_bytes() -> Result<&'static [u8], AppError> {
    rustyvault::extension_package::web_extension_package_bytes()
        .map_err(|error| AppError::from(ApiError::Internal(error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::io::Read;

    fn package_root(version: &str) -> String {
        format!("rustyvault-webext-{version}")
    }

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

        let mut readme = String::new();
        archive
            .by_name(&format!("{root}/README.md"))
            .expect("readme should exist in the package")
            .read_to_string(&mut readme)
            .expect("readme should read");
        assert!(readme.contains("Load unpacked"));
        assert_eq!(
            info.package_filename,
            format!("rustyvault-webext-{}.zip", info.version)
        );
    }
}
