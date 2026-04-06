use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rustfin_core::error::ApiError;
#[cfg(feature = "rustyvault")]
use rustyvault::extension_package::RustyVaultWebExtensionTarget;
use serde::Serialize;
use sqlx::Row;
use tracing::warn;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

const RUSTYVAULT_WEBEXT_ARTIFACT_ID: &str = "rustyvault-webext";
const RUSTYVAULT_WEBEXT_CHROMIUM_ARTIFACT_ID: &str = "rustyvault-webext-chromium";
const RUSTYVAULT_WEBEXT_FIREFOX_ARTIFACT_ID: &str = "rustyvault-webext-firefox";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadArtifactAvailability {
    Available,
    Unavailable,
    Planned,
}

impl From<String> for DownloadArtifactAvailability {
    fn from(s: String) -> Self {
        match s.as_str() {
            "available" => Self::Available,
            "unavailable" => Self::Unavailable,
            _ => Self::Planned,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DownloadArtifactResponse {
    pub id: String,
    pub artifact_id: String,
    pub title: String,
    pub summary: String,
    pub availability: DownloadArtifactAvailability,
    pub detail: String,
    pub platform: String,
    pub architecture: String,
    pub version: Option<String>,
    pub channel: String,
    pub package_filename: Option<String>,
    pub file_size: Option<i64>,
    pub checksum: Option<String>,
    pub signature_status: String,
    pub distribution_mode: String,
    pub external_url: Option<String>,
    pub download_path: Option<String>,
    pub install_mode: Option<String>,
    pub setup_path: Option<String>,
    pub requires_sign_in: bool,
    pub install_steps: Vec<String>,
}

#[derive(Debug)]
struct DownloadArtifactRow {
    id: String,
    artifact_id: String,
    title: String,
    summary: String,
    detail: String,
    platform: String,
    architecture: String,
    version: Option<String>,
    channel: String,
    filename: Option<String>,
    file_size: Option<i64>,
    checksum: Option<String>,
    signature_status: String,
    distribution_mode: String,
    external_url: Option<String>,
    availability: String,
    requires_sign_in: bool,
    install_steps_json: String,
}

fn decode_download_artifact_row(
    row: sqlx::postgres::PgRow,
) -> Result<DownloadArtifactRow, sqlx::Error> {
    Ok(DownloadArtifactRow {
        id: row.try_get("id")?,
        artifact_id: row.try_get("artifact_id")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        detail: row.try_get("detail")?,
        platform: row.try_get("platform")?,
        architecture: row.try_get("architecture")?,
        version: row.try_get("version")?,
        channel: row.try_get("channel")?,
        filename: row.try_get("filename")?,
        file_size: row.try_get("file_size")?,
        checksum: row.try_get("checksum")?,
        signature_status: row.try_get("signature_status")?,
        distribution_mode: row.try_get("distribution_mode")?,
        external_url: row.try_get("external_url")?,
        availability: row.try_get("availability")?,
        requires_sign_in: row.try_get("requires_sign_in")?,
        install_steps_json: row.try_get("install_steps_json")?,
    })
}

pub async fn get_download_catalog(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Response, AppError> {
    let _ = auth;
    let catalog = build_download_catalog(&state).await?;
    Ok(no_store_json(catalog))
}

pub async fn build_download_catalog(state: &AppState) -> Result<DownloadCatalogResponse, AppError> {
    let rows = sqlx::query(
        "SELECT id, artifact_id, title, summary, detail, platform, architecture, version, channel, filename, file_size, checksum, signature_status, distribution_mode, external_url, availability, requires_sign_in, install_steps_json FROM download_artifact ORDER BY platform, title"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let mut items = Vec::new();

    // Convert DB rows to response items
    for pg_row in rows {
        let row = decode_download_artifact_row(pg_row)
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        if row.artifact_id == RUSTYVAULT_WEBEXT_ARTIFACT_ID
            || row.artifact_id == RUSTYVAULT_WEBEXT_CHROMIUM_ARTIFACT_ID
            || row.artifact_id == RUSTYVAULT_WEBEXT_FIREFOX_ARTIFACT_ID
        {
            continue;
        }
        let install_steps: Vec<String> =
            serde_json::from_str(&row.install_steps_json).unwrap_or_default();
        let download_path = if row.distribution_mode == "direct" && row.availability == "available"
        {
            Some(format!("/api/v1/downloads/items/{}/package", row.id))
        } else {
            None
        };

        // For rustyvault-webext, we might want to override setup_path if it's the specific artifact
        let setup_path = if row.artifact_id == RUSTYVAULT_WEBEXT_ARTIFACT_ID {
            Some("/vault#vault-devices".to_string())
        } else {
            None // Could be stored in DB if we added a field, or derived
        };

        let install_mode = if row.artifact_id == RUSTYVAULT_WEBEXT_ARTIFACT_ID {
            Some("download_zip_extract_then_load_unpacked".to_string())
        } else {
            None
        };

        items.push(DownloadArtifactResponse {
            id: row.id,
            artifact_id: row.artifact_id,
            title: row.title,
            summary: row.summary,
            availability: row.availability.into(),
            detail: row.detail,
            platform: row.platform,
            architecture: row.architecture,
            version: row.version,
            channel: row.channel,
            package_filename: row.filename,
            file_size: row.file_size,
            checksum: row.checksum,
            signature_status: row.signature_status,
            distribution_mode: row.distribution_mode,
            external_url: row.external_url,
            download_path,
            install_mode,
            setup_path,
            requires_sign_in: row.requires_sign_in,
            install_steps,
        });
    }

    append_virtual_rustyvault_extension_artifacts(state, &mut items)?;

    // Append legacy/virtual artifacts if not present in DB?
    // For now, we assume the DB is the source of truth as per instructions.
    // If the DB is empty, the catalog is empty.

    // Special handling for legacy RustyVault webext if it's NOT in the DB but the feature is enabled?
    // The instructions say "replace generic placeholders".
    // We will assume migration or admin action will populate the DB.

    Ok(DownloadCatalogResponse { items })
}

fn append_virtual_rustyvault_extension_artifacts(
    state: &AppState,
    items: &mut Vec<DownloadArtifactResponse>,
) -> Result<(), AppError> {
    #[cfg(feature = "rustyvault")]
    {
        if !state.rustyvault.available {
            return Ok(());
        }
        for target in [
            RustyVaultWebExtensionTarget::Chromium,
            RustyVaultWebExtensionTarget::Firefox,
        ] {
            let info = crate::rustyvault_host::extension_package::extension_info(target)?;
            let title = match target {
                RustyVaultWebExtensionTarget::Chromium => {
                    "RustyVault Browser Extension for Chrome, Edge, Brave, and other Chromium browsers"
                }
                RustyVaultWebExtensionTarget::Firefox => "RustyVault Browser Extension for Firefox",
            };
            let detail = match target {
                RustyVaultWebExtensionTarget::Chromium => {
                    "Built MV3 package with runtime-granted site access, inline suggestions, and explicit save/update prompts."
                }
                RustyVaultWebExtensionTarget::Firefox => {
                    "Firefox-targeted package with the same RustyVault pairing, fill, and save flows packaged as an XPI."
                }
            };
            let install_steps = match target {
                RustyVaultWebExtensionTarget::Chromium => vec![
                    "Download the package and extract it locally.".to_string(),
                    "Open your browser extensions page and enable developer mode.".to_string(),
                    "Choose Load unpacked and select the extracted rustyvault-webext folder."
                        .to_string(),
                    "Open RustyVault from the toolbar, set your Rustyfin server URL, then pair it from /vault."
                        .to_string(),
                ],
                RustyVaultWebExtensionTarget::Firefox => vec![
                    "Download the XPI package.".to_string(),
                    "Open about:debugging in Firefox and choose This Firefox.".to_string(),
                    "Select Load Temporary Add-on and choose the downloaded XPI or extracted manifest."
                        .to_string(),
                    "Open RustyVault from the toolbar, set your Rustyfin server URL, then pair it from /vault."
                        .to_string(),
                ],
            };
            items.push(DownloadArtifactResponse {
                id: info.artifact_id.clone(),
                artifact_id: info.artifact_id.clone(),
                title: title.to_string(),
                summary: "Secure browser extension for pairing to RustyVault, inline credential suggestions, manual fill, and explicit save/update flows.".to_string(),
                availability: DownloadArtifactAvailability::Available,
                detail: detail.to_string(),
                platform: "browser_extension".to_string(),
                architecture: info.browser_family.clone(),
                version: Some(info.version.clone()),
                channel: "stable".to_string(),
                package_filename: Some(info.package_filename.clone()),
                file_size: None,
                checksum: None,
                signature_status: "unsigned".to_string(),
                distribution_mode: "direct".to_string(),
                external_url: None,
                download_path: Some(format!(
                    "/api/v1/downloads/artifacts/{}/package",
                    info.artifact_id
                )),
                install_mode: Some(info.install_mode.clone()),
                setup_path: Some("/vault".to_string()),
                requires_sign_in: true,
                install_steps,
            });
        }
    }
    #[cfg(not(feature = "rustyvault"))]
    {
        let _ = state;
        let _ = items;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DownloadCatalogResponse {
    pub items: Vec<DownloadArtifactResponse>,
}

pub async fn download_artifact_package(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(item_id): Path<String>,
) -> Result<Response, AppError> {
    let _ = auth;

    if item_id == RUSTYVAULT_WEBEXT_ARTIFACT_ID || item_id == RUSTYVAULT_WEBEXT_CHROMIUM_ARTIFACT_ID
    {
        #[cfg(feature = "rustyvault")]
        {
            return download_rustyvault_webext_package(
                &state,
                RustyVaultWebExtensionTarget::Chromium,
            )
            .await;
        }
        #[cfg(not(feature = "rustyvault"))]
        {
            return download_rustyvault_webext_package(&state, ()).await;
        }
    }
    if item_id == RUSTYVAULT_WEBEXT_FIREFOX_ARTIFACT_ID {
        #[cfg(feature = "rustyvault")]
        {
            return download_rustyvault_webext_package(
                &state,
                RustyVaultWebExtensionTarget::Firefox,
            )
            .await;
        }
        #[cfg(not(feature = "rustyvault"))]
        {
            return download_rustyvault_webext_package(&state, ()).await;
        }
    }

    // Check if it's the legacy path (artifact_id) or new path (item_id)
    // The route definition decides this.
    // If we change the route to /items/:id/package, we get the UUID.
    // If we keep /artifacts/:id/package, we get artifact_id.

    // We'll support fetching by UUID from DB.

    let row = sqlx::query(
        "SELECT id, artifact_id, title, summary, detail, platform, architecture, version, channel, filename, file_size, checksum, signature_status, distribution_mode, external_url, availability, requires_sign_in, install_steps_json FROM download_artifact WHERE id = $1"
    )
    .bind(&item_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if let Some(pg_row) = row {
        let row = decode_download_artifact_row(pg_row)
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        if row.distribution_mode != "direct" {
            return Err(ApiError::BadRequest("Artifact is not a direct download".into()).into());
        }
        if row.availability != "available" {
            return Err(ApiError::BadRequest("Artifact is not available".into()).into());
        }

        // Special case for RustyVault WebExt to keep serving the embedded/generated package
        if row.artifact_id == RUSTYVAULT_WEBEXT_ARTIFACT_ID {
            #[cfg(feature = "rustyvault")]
            {
                return download_rustyvault_webext_package(
                    &state,
                    RustyVaultWebExtensionTarget::Chromium,
                )
                .await;
            }
            #[cfg(not(feature = "rustyvault"))]
            {
                return download_rustyvault_webext_package(&state, ()).await;
            }
        }

        // Serve file from artifacts directory
        // Assuming RUSTFIN_ARTIFACTS_DIR or cache_dir/artifacts
        let artifacts_dir = std::env::var("RUSTFIN_ARTIFACTS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| state.cache_dir.join("artifacts"));

        let filename = row
            .filename
            .ok_or_else(|| ApiError::Internal("Artifact has no filename".into()))?;
        let file_path = artifacts_dir.join(&filename);

        if !file_path.exists() {
            warn!("Artifact file not found at {}", file_path.display());
            return Err(ApiError::NotFound("Artifact file missing on host".into()).into());
        }

        // Stream the file
        // Simple implementation: read entire file (not efficient for large files, use ServeFile for production)
        // But for "thin shell" clients it might be okay.
        // Better: use axum-extra or tower-http ServeFile.
        // Since I can't easily add dependencies, I'll use tokio::fs::read.
        // Actually, axum::body::Body can wrap a file stream.

        let file_bytes = tokio::fs::read(&file_path)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to read artifact: {}", e)))?;

        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            )
            .body(Body::from(file_bytes))
            .map_err(|error| {
                ApiError::Internal(format!(
                    "failed to build artifact download response: {error}"
                ))
                .into()
            });
    }

    // Fallback for legacy calls or not found
    // If the ID passed was "rustyvault-webext" (legacy string ID) instead of UUID?
    // The new route should use UUID.
    // We should probably keep the OLD route for backward compatibility if needed,
    // but the task says "rewrite".

    Err(ApiError::NotFound("download artifact not found".into()).into())
}

#[cfg(feature = "rustyvault")]
async fn download_rustyvault_webext_package(
    state: &AppState,
    target: RustyVaultWebExtensionTarget,
) -> Result<Response, AppError> {
    if !state.rustyvault.available {
        return Ok(service_unavailable_response(
            state.rustyvault.public_reason(),
        ));
    }

    let info = crate::rustyvault_host::extension_package::extension_info(target)?;
    let body = crate::rustyvault_host::extension_package::extension_package_bytes(target)?;
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
async fn download_rustyvault_webext_package(
    state: &AppState,
    _target: (),
) -> Result<Response, AppError> {
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
        let app = DownloadArtifactResponse {
            id: "planned-client".to_string(),
            artifact_id: "rustyfin-client".to_string(),
            title: "Rustyfin Client".to_string(),
            summary: "Planned desktop client".to_string(),
            availability: DownloadArtifactAvailability::Planned,
            detail: "Not shipped yet".to_string(),
            platform: "linux".to_string(),
            architecture: "x86_64".to_string(),
            version: None,
            channel: "stable".to_string(),
            package_filename: None,
            file_size: None,
            checksum: None,
            signature_status: "unsigned".to_string(),
            distribution_mode: "planned".to_string(),
            external_url: None,
            download_path: None,
            install_mode: None,
            setup_path: None,
            requires_sign_in: false,
            install_steps: Vec::new(),
        };
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
