/// HLS playlist and segment content-type helpers.

/// Content-Type for HLS master/variant playlists.
pub const PLAYLIST_CONTENT_TYPE: &str = "application/vnd.apple.mpegurl";

/// Content-Type for MPEG-TS segments.
pub const SEGMENT_CONTENT_TYPE_TS: &str = "video/MP2T";

/// Content-Type for fMP4 segments.
pub const SEGMENT_CONTENT_TYPE_MP4: &str = "video/mp4";

/// Determine segment content type from filename extension.
pub fn segment_content_type(filename: &str) -> &'static str {
    if filename.ends_with(".m4s") || filename.ends_with(".mp4") {
        SEGMENT_CONTENT_TYPE_MP4
    } else {
        SEGMENT_CONTENT_TYPE_TS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types() {
        assert_eq!(segment_content_type("seg_00001.ts"), SEGMENT_CONTENT_TYPE_TS);
        assert_eq!(segment_content_type("seg_00001.m4s"), SEGMENT_CONTENT_TYPE_MP4);
        assert_eq!(segment_content_type("init.mp4"), SEGMENT_CONTENT_TYPE_MP4);
    }
}
