use std::path::{Path, PathBuf};
use tracing::debug;

use crate::parser;

/// Entry discovered during a filesystem walk.
#[derive(Debug, Clone)]
pub struct MediaEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime_ts: i64,
}

/// Walk a directory recursively and collect video files, skipping ignored patterns.
pub fn walk_media_dir(root: &Path) -> Vec<MediaEntry> {
    let mut entries = Vec::new();
    walk_recursive(root, &mut entries);
    entries
}

fn walk_recursive(dir: &Path, entries: &mut Vec<MediaEntry>) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::warn!(path = %dir.display(), error = %e, "cannot read directory");
            return;
        }
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip hidden files/dirs and ignored patterns
        if name.starts_with('.') || parser::should_ignore(&name) {
            debug!(path = %path.display(), "skipping ignored entry");
            continue;
        }

        if path.is_dir() {
            // Skip known junk directories
            if name == "@eaDir" || name == "#recycle" || name == ".Trash" {
                continue;
            }
            walk_recursive(&path, entries);
        } else if parser::is_video_file(&name) {
            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            entries.push(MediaEntry {
                path,
                size_bytes: metadata.len(),
                mtime_ts: mtime,
            });
        }
    }
}
