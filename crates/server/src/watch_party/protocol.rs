#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Auth {
        token: String,
    },
    Play {
        position_ms: u64,
    },
    Pause {
        position_ms: u64,
    },
    Seek {
        position_ms: u64,
    },
    SkipNext,
    SkipPrev,
    PlayTrack {
        track_id: String,
    },
    ChangeVideo {
        video_id: String,
    },
    QueueVideo {
        video_id: String,
    },
    AdvanceQueue {
        expected_video_id: String,
    },
    PlayQueuedVideo {
        queue_index: usize,
    },
    RemoveQueuedVideo {
        queue_index: usize,
    },
    MoveQueuedVideo {
        from_index: usize,
        to_index: usize,
    },
    #[serde(rename = "search_youtube", alias = "search_you_tube")]
    SearchYouTube {
        query: String,
    },
    ChangeWebUrl {
        url: String,
    },
    CreateSetTool {
        tool: String,
    },
    CreateSetDocumentName {
        document_name: String,
    },
    CreateSetText {
        text_content: String,
        #[serde(default)]
        text_format: Option<String>,
    },
    CreateSetCanvas {
        canvas_strokes: Vec<CreateCanvasStroke>,
    },
    Ping,
    Pong,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PresenceMember {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub connected: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueueEntry {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_art_url: Option<String>,
    pub video_id: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct YouTubeSearchEntry {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub thumbnail_url: String,
    pub view_count: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateCanvasPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateCanvasStroke {
    pub id: String,
    pub color: String,
    pub size: f32,
    pub points: Vec<CreateCanvasPoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    State {
        room_id: String,
        item_id: String,
        playing: bool,
        position_ms: u64,
        updated_ts_ms: i64,
        server_ts_ms: i64,
        members: Vec<PresenceMember>,
    },
    AudioState {
        room_id: String,
        audio_source: String,
        track_id: String,
        title: String,
        artist: String,
        album: String,
        album_art_url: Option<String>,
        stream_url: Option<String>,
        duration_ms: Option<u64>,
        position_ms: u64,
        playing: bool,
        updated_ts_ms: i64,
        server_ts_ms: i64,
        queue: Vec<QueueEntry>,
        queue_index: usize,
        members: Vec<PresenceMember>,
    },
    #[serde(rename = "youtube_state")]
    YouTubeState {
        room_id: String,
        video_id: String,
        playing: bool,
        position_ms: u64,
        updated_ts_ms: i64,
        server_ts_ms: i64,
        queue: Vec<String>,
        search_query: String,
        search_results: Vec<YouTubeSearchEntry>,
        members: Vec<PresenceMember>,
    },
    WebState {
        room_id: String,
        url: String,
        updated_ts_ms: i64,
        server_ts_ms: i64,
        members: Vec<PresenceMember>,
    },
    CreateState {
        room_id: String,
        active_tool: String,
        document_name: String,
        text_format: String,
        text_content: String,
        canvas_strokes: Vec<CreateCanvasStroke>,
        updated_ts_ms: i64,
        server_ts_ms: i64,
        members: Vec<PresenceMember>,
    },
    Presence {
        user_id: String,
        connected: bool,
    },
    Error {
        message: String,
    },
    Pong,
    RoomReconfigured {
        room_mode: String,
        item_id: String,
        audio_source: Option<String>,
        audio_library_id: Option<String>,
        youtube_video_id: Option<String>,
        web_url: Option<String>,
        create_tool: Option<String>,
        create_document_name: Option<String>,
    },
    RoomEnded,
}
