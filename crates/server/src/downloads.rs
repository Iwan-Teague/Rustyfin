use axum::Json;
#[cfg(feature = "rustyvault")]
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rustfin_core::error::ApiError;
use serde::Serialize;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

const RUSTYVAULT_WEBEXT_ARTIFACT_ID: &str = "rustyvault-webext";
#[cfg(feature = "rustyvault")]
const RUSTYVAULT_WEBEXT_DOWNLOAD_PATH: &str =
    "/api/v1/downloads/artifacts/rustyvault-webext/package";
const RUSTYVAULT_WEBEXT_SETUP_PATH: &str = "/vault#vault-devices";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadArtifactAvailability {
    Available,
    Unavailable,
    Planned,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DownloadArtifactResponse {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub availability: DownloadArtifactAvailability,
    pub detail: String,
    pub version: Option<String>,
    pub package_filename: Option<String>,
    pub download_path: Option<String>,
    pub install_mode: Option<String>,
    pub setup_path: Option<String>,
    pub requires_sign_in: bool,
    pub install_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DownloadCatalogResponse {
    pub items: Vec<DownloadArtifactResponse>,
}

pub async fn get_download_catalog(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Response, AppError> {
    let _ = auth;
    Ok(no_store_json(build_download_catalog(&state)))
}

pub async fn download_artifact_package(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(artifact_id): Path<String>,
) -> Result<Response, AppError> {
    let _ = auth;
    match artifact_id.as_str() {
        RUSTYVAULT_WEBEXT_ARTIFACT_ID => download_rustyvault_webext_package(&state).await,
        _ => Err(ApiError::NotFound("download artifact not found".into()).into()),
    }
}

fn build_download_catalog(state: &AppState) -> DownloadCatalogResponse {
    DownloadCatalogResponse {
        items: vec![
            rustyvault_webext_artifact(state),
            planned_rustyfin_app_artifact(),
            planned_companion_tools_artifact(),
        ],
    }
}

fn planned_rustyfin_app_artifact() -> DownloadArtifactResponse {
    DownloadArtifactResponse {
        id: "rustyfin-app".to_string(),
        title: "Rustyfin App".to_string(),
        summary: "A packaged first-party Rustyfin application will be published here when the client release path is ready.".to_string(),
        availability: DownloadArtifactAvailability::Planned,
        detail: "Until then, the web UI remains the supported client surface.".to_string(),
        version: None,
        package_filename: None,
        download_path: None,
        install_mode: None,
        setup_path: None,
        requires_sign_in: true,
        install_steps: Vec::new(),
    }
}

fn planned_companion_tools_artifact() -> DownloadArtifactResponse {
    DownloadArtifactResponse {
        id: "companion-tools".to_string(),
        title: "Additional Companion Tools".to_string(),
        summary: "Future Rustyfin extensions, companion utilities, and related client packages can land on this page without moving the install flow again.".to_string(),
        availability: DownloadArtifactAvailability::Planned,
        detail: "This page is structured as a stable release surface rather than a one-off extension card.".to_string(),
        version: None,
        package_filename: None,
        download_path: None,
        install_mode: None,
        setup_path: None,
        requires_sign_in: true,
        install_steps: Vec::new(),
    }
}

#[cfg(feature = "rustyvault")]
fn rustyvault_webext_artifact(state: &AppState) -> DownloadArtifactResponse {
    let default_install_steps = vec![
        "Download the current zip package from this page.".to_string(),
        "Extract it locally, then open Chrome or Edge developer extensions and choose Load unpacked.".to_string(),
        "Select the extracted folder, open the extension popup, and finish pairing from the Vault page.".to_string(),
    ];

    if !state.rustyvault.available {
        return DownloadArtifactResponse {
            id: RUSTYVAULT_WEBEXT_ARTIFACT_ID.to_string(),
            title: "RustyVault Browser Extension".to_string(),
            summary: "Client-side pairing, conservative save prompts, manual autofill, and blinded site matching for RustyVault.".to_string(),
            availability: DownloadArtifactAvailability::Unavailable,
            detail: state.rustyvault.public_reason().to_string(),
            version: None,
            package_filename: None,
            download_path: None,
            install_mode: None,
            setup_path: Some(RUSTYVAULT_WEBEXT_SETUP_PATH.to_string()),
            requires_sign_in: true,
            install_steps: default_install_steps,
        };
    }

    match crate::rustyvault_host::extension_package::extension_info() {
        Ok(info) => DownloadArtifactResponse {
            id: RUSTYVAULT_WEBEXT_ARTIFACT_ID.to_string(),
            title: "RustyVault Browser Extension".to_string(),
            summary: "Client-side pairing, conservative save prompts, manual autofill, and blinded site matching for RustyVault.".to_string(),
            availability: DownloadArtifactAvailability::Available,
            detail: "Delivered through the host downloads registry and the authenticated extension package pipeline.".to_string(),
            version: Some(info.version),
            package_filename: Some(info.package_filename),
            download_path: Some(RUSTYVAULT_WEBEXT_DOWNLOAD_PATH.to_string()),
            install_mode: Some(info.install_mode),
            setup_path: Some(RUSTYVAULT_WEBEXT_SETUP_PATH.to_string()),
            requires_sign_in: true,
            install_steps: default_install_steps,
        },
        Err(_) => DownloadArtifactResponse {
            id: RUSTYVAULT_WEBEXT_ARTIFACT_ID.to_string(),
            title: "RustyVault Browser Extension".to_string(),
            summary: "Client-side pairing, conservative save prompts, manual autofill, and blinded site matching for RustyVault.".to_string(),
            availability: DownloadArtifactAvailability::Unavailable,
            detail: "The current extension package metadata is unavailable on this host.".to_string(),
            version: None,
            package_filename: None,
            download_path: None,
            install_mode: None,
            setup_path: Some(RUSTYVAULT_WEBEXT_SETUP_PATH.to_string()),
            requires_sign_in: true,
            install_steps: default_install_steps,
        },
    }
}

#[cfg(not(feature = "rustyvault"))]
fn rustyvault_webext_artifact(state: &AppState) -> DownloadArtifactResponse {
    DownloadArtifactResponse {
        id: RUSTYVAULT_WEBEXT_ARTIFACT_ID.to_string(),
        title: "RustyVault Browser Extension".to_string(),
        summary: "Client-side pairing, conservative save prompts, manual autofill, and blinded site matching for RustyVault.".to_string(),
        availability: DownloadArtifactAvailability::Unavailable,
        detail: state.rustyvault.public_reason().to_string(),
        version: None,
        package_filename: None,
        download_path: None,
        install_mode: None,
        setup_path: Some(RUSTYVAULT_WEBEXT_SETUP_PATH.to_string()),
        requires_sign_in: true,
        install_steps: vec![
            "Download availability is controlled by the host feature registry.".to_string(),
            "When RustyVault is enabled and migrated, the package will appear here.".to_string(),
            "Pairing and device controls remain on the Vault page.".to_string(),
        ],
    }
}

#[cfg(feature = "rustyvault")]
async fn download_rustyvault_webext_package(state: &AppState) -> Result<Response, AppError> {
    if !state.rustyvault.available {
        return Ok(service_unavailable_response(
            state.rustyvault.public_reason(),
        ));
    }

    let info = crate::rustyvault_host::extension_package::extension_info()?;
    let body = crate::rustyvault_host::extension_package::extension_package_bytes()?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                info.package_filename.replace('"', "")
            ),
        )
        .body(Body::from(body.to_vec()))
        .map_err(|error| {
            ApiError::Internal(format!(
                "failed to build artifact download response: {error}"
            ))
            .into()
        })
}

#[cfg(not(feature = "rustyvault"))]
async fn download_rustyvault_webext_package(state: &AppState) -> Result<Response, AppError> {
    Ok(service_unavailable_response(
        state.rustyvault.public_reason(),
    ))
}

fn service_unavailable_response(message: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "error": {
                "code": "service_unavailable",
                "message": message,
                "details": {}
            }
        })),
    )
        .into_response()
}

fn no_store_json<T: Serialize>(value: T) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planned_catalog_entries_remain_non_downloadable() {
        let app = planned_rustyfin_app_artifact();
        assert_eq!(app.availability, DownloadArtifactAvailability::Planned);
        assert!(app.download_path.is_none());
        assert!(app.package_filename.is_none());
    }

    #[test]
    fn service_unavailable_response_uses_503() {
        let response = service_unavailable_response("RustyVault unavailable");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
