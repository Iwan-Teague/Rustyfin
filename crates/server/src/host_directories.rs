use rustfin_core::error::ApiError;
use serde::Serialize;

#[derive(Serialize)]
pub struct HostDirectoryListEntry {
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct HostDirectoryListResponse {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub roots: Vec<String>,
    pub directories: Vec<HostDirectoryListEntry>,
}

fn collect_host_directory_roots() -> Vec<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(raw) = std::env::var("RUSTFIN_DIRECTORY_BROWSE_ROOTS") {
        for segment in raw.split(':') {
            let path = segment.trim();
            if !path.is_empty() {
                candidates.push(std::path::PathBuf::from(path));
            }
        }
    }

    if candidates.is_empty() {
        if let Ok(media_root) = std::env::var("RUSTFIN_MEDIA_PATH") {
            let trimmed = media_root.trim();
            if !trimmed.is_empty() {
                candidates.push(std::path::PathBuf::from(trimmed));
            }
        }
    }

    if candidates.is_empty() {
        candidates.push(std::path::PathBuf::from("/srv/media"));
    }

    for fallback in ["/media", "/mnt", "/home"] {
        let fallback_path = std::path::PathBuf::from(fallback);
        if !candidates
            .iter()
            .any(|candidate| candidate == &fallback_path)
        {
            candidates.push(fallback_path);
        }
    }

    let mut canonical_roots: Vec<std::path::PathBuf> = Vec::new();
    for candidate in candidates {
        if let Ok(canonical) = std::fs::canonicalize(&candidate) {
            if canonical.is_dir() && !canonical_roots.iter().any(|root| root == &canonical) {
                canonical_roots.push(canonical);
            }
        }
    }

    canonical_roots
}

pub fn build_host_directory_listing(
    requested_path: Option<String>,
) -> Result<HostDirectoryListResponse, ApiError> {
    let roots = collect_host_directory_roots();
    if roots.is_empty() {
        return Err(ApiError::BadRequest(
            "no browsable host roots are available; ensure media paths are mounted and readable by the backend".into(),
        ));
    }

    let current_path = if let Some(requested) = requested_path {
        std::fs::canonicalize(&requested).map_err(|_| {
            ApiError::BadRequest("requested path does not exist on the backend host".into())
        })?
    } else {
        roots[0].clone()
    };

    if !current_path.is_dir() {
        return Err(ApiError::BadRequest(
            "requested path is not a directory".into(),
        ));
    }

    let active_root = roots
        .iter()
        .filter(|root| current_path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .cloned()
        .ok_or_else(|| {
            ApiError::Forbidden("requested path is outside configured browse roots".into())
        })?;

    let mut directories: Vec<HostDirectoryListEntry> = Vec::new();
    let read_dir = std::fs::read_dir(&current_path)
        .map_err(|e| ApiError::BadRequest(format!("failed to read directory: {e}")))?;
    for entry_result in read_dir {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let candidate_path = entry.path();
        if !candidate_path.is_dir() {
            continue;
        }
        let canonical = match std::fs::canonicalize(&candidate_path) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if !roots.iter().any(|root| canonical.starts_with(root)) {
            continue;
        }
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy().trim().to_string();
        if name.is_empty() {
            continue;
        }
        directories.push(HostDirectoryListEntry {
            name,
            path: canonical.to_string_lossy().into_owned(),
        });
    }
    directories.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let parent_path = if current_path == active_root {
        None
    } else {
        current_path
            .parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .filter(|parent| parent.starts_with(&active_root))
            .map(|parent| parent.to_string_lossy().into_owned())
    };

    Ok(HostDirectoryListResponse {
        current_path: current_path.to_string_lossy().into_owned(),
        parent_path,
        roots: roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect(),
        directories,
    })
}
