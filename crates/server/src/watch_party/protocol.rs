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
    TrackEnded {
        position_ms: u64,
    },
    ReorderAudioQueue {
        from_index: usize,
        to_index: usize,
    },
    SetAudioShuffle {
        enabled: bool,
    },
    SetAudioRepeatMode {
        mode: AudioRepeatMode,
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
    CreateUpsertTextPage {
        page_id: String,
        page_html: String,
    },
    CreateInsertTextPage {
        page_id: String,
        page_html: String,
        #[serde(default)]
        after_page_id: Option<String>,
    },
    CreateDeleteTextPage {
        page_id: String,
    },
    CreateSetTextPageOrientation {
        page_orientation: String,
    },
    CreateSetCanvas {
        canvas_strokes: Vec<CreateCanvasStroke>,
    },
    CreateCanvasAppendStroke {
        canvas_stroke: CreateCanvasStroke,
    },
    CreateCanvasRemoveStroke {
        stroke_id: String,
    },
    CreateCanvasClear,
    PlaySetGame {
        game: String,
    },
    ChessSetPlayers {
        white_user_id: Option<String>,
        black_user_id: Option<String>,
    },
    ChessConfigureAi {
        enabled: bool,
        #[serde(default)]
        difficulty: Option<String>,
        #[serde(default)]
        human_color: Option<String>,
    },
    ChessMove {
        from: String,
        to: String,
        #[serde(default)]
        promotion: Option<String>,
    },
    ChessReset,
    ConnectFourSetPlayers {
        red_user_id: Option<String>,
        yellow_user_id: Option<String>,
    },
    ConnectFourDrop {
        column: usize,
    },
    ConnectFourReset,
    BattleshipSetPlayers {
        blue_user_id: Option<String>,
        red_user_id: Option<String>,
    },
    BattleshipAutoPlace,
    BattleshipSetReady {
        ready: bool,
    },
    BattleshipFire {
        x: u8,
        y: u8,
    },
    BattleshipReset,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRepeatMode {
    None,
    Track,
    Queue,
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
pub struct ChessLegalMove {
    pub from: String,
    pub to: String,
    pub promotion: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChessState {
    pub fen: String,
    pub turn: String,
    pub status: String,
    pub winner_color: Option<String>,
    pub white_user_id: Option<String>,
    pub black_user_id: Option<String>,
    pub last_move_from: Option<String>,
    pub last_move_to: Option<String>,
    pub last_move_promotion: Option<String>,
    pub reset_requested_white: bool,
    pub reset_requested_black: bool,
    pub ai_enabled: bool,
    pub ai_difficulty: String,
    pub ai_color: Option<String>,
    pub legal_moves: Vec<ChessLegalMove>,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectFourState {
    pub board_rows: Vec<String>,
    pub turn: String,
    pub status: String,
    pub winner_color: Option<String>,
    pub red_user_id: Option<String>,
    pub yellow_user_id: Option<String>,
    pub last_move_row: Option<u8>,
    pub last_move_col: Option<u8>,
    pub reset_requested_red: bool,
    pub reset_requested_yellow: bool,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BattleshipLastShot {
    pub by_color: String,
    pub x: u8,
    pub y: u8,
    pub result: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BattleshipState {
    pub phase: String,
    pub status: String,
    pub turn_color: String,
    pub winner_color: Option<String>,
    pub blue_user_id: Option<String>,
    pub red_user_id: Option<String>,
    pub blue_ready: bool,
    pub red_ready: bool,
    pub blue_grid_rows: Vec<String>,
    pub red_grid_rows: Vec<String>,
    pub remaining_ship_cells_blue: u16,
    pub remaining_ship_cells_red: u16,
    pub last_shot: Option<BattleshipLastShot>,
    pub reset_requested_blue: bool,
    pub reset_requested_red: bool,
    pub updated_ts_ms: i64,
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
        shuffle_enabled: bool,
        repeat_mode: AudioRepeatMode,
        members: Vec<PresenceMember>,
    },
    OnlineAudioStatus {
        room_id: String,
        video_id: Option<String>,
        track_id: Option<String>,
        stage: String,
        status: String,
        message: String,
        updated_ts_ms: i64,
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
    PlayState {
        room_id: String,
        active_game: String,
        chess: ChessState,
        connect_four: ConnectFourState,
        battleship: BattleshipState,
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
