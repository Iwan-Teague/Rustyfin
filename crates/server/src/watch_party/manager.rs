use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use chess::{Board, BoardStatus, ChessMove, Color, MoveGen, Piece, Square};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::sync::{RwLock, broadcast};

use super::protocol::{AudioRepeatMode, CreateCanvasStroke, ServerMessage, YouTubeSearchEntry};

const MAX_ACTIVE_ROOMS: usize = 512;
const EMPTY_ROOM_TTL_SECONDS: i64 = 5 * 60;
const CONNECT_FOUR_ROWS: usize = 6;
const CONNECT_FOUR_COLS: usize = 7;
const CONNECT_FOUR_CELLS: usize = CONNECT_FOUR_ROWS * CONNECT_FOUR_COLS;
const BATTLESHIP_BOARD_SIZE: usize = 10;
const BATTLESHIP_BOARD_CELLS: usize = BATTLESHIP_BOARD_SIZE * BATTLESHIP_BOARD_SIZE;
const BATTLESHIP_SHIP_SIZES: [u8; 5] = [5, 4, 3, 3, 2];
const BATTLESHIP_TOTAL_SHIP_CELLS: u16 = 17;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaybackState {
    pub playing: bool,
    pub position_ms: u64,
    pub updated_ts_ms: i64,
    pub playback_rate: f32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            playing: false,
            position_ms: 0,
            updated_ts_ms: chrono::Utc::now().timestamp_millis(),
            playback_rate: 1.0,
        }
    }
}

pub enum PlaybackAction {
    Play { position_ms: u64 },
    Pause { position_ms: u64 },
    Seek { position_ms: u64 },
}

#[derive(Debug, Clone)]
pub struct AudioQueueState {
    pub track_ids: Vec<String>,
    pub current_index: usize,
    pub position_ms: u64,
    pub playing: bool,
    pub shuffle_enabled: bool,
    pub repeat_mode: AudioRepeatMode,
    pub updated_ts_ms: i64,
}

impl Default for AudioQueueState {
    fn default() -> Self {
        Self {
            track_ids: Vec::new(),
            current_index: 0,
            position_ms: 0,
            playing: false,
            shuffle_enabled: false,
            repeat_mode: AudioRepeatMode::None,
            updated_ts_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

pub enum AudioAction {
    SkipNext,
    SkipPrev,
    PlayTrack { track_id: String },
    SetPlayingState { position_ms: u64, playing: bool },
}

#[derive(Debug, Clone)]
pub struct CreateState {
    pub active_tool: String,
    pub document_name: String,
    pub text_format: String,
    pub text_content: String,
    pub canvas_strokes: Vec<CreateCanvasStroke>,
    pub updated_ts_ms: i64,
}

impl Default for CreateState {
    fn default() -> Self {
        Self {
            active_tool: "text".to_string(),
            document_name: "Untitled Document".to_string(),
            text_format: "plain".to_string(),
            text_content: String::new(),
            canvas_strokes: Vec::new(),
            updated_ts_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChessLastMove {
    pub from: String,
    pub to: String,
    pub promotion: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChessState {
    pub fen: String,
    pub status: String,
    pub winner_color: Option<String>,
    pub white_user_id: Option<String>,
    pub black_user_id: Option<String>,
    pub last_move: Option<ChessLastMove>,
    pub reset_requested_white: bool,
    pub reset_requested_black: bool,
    pub ai_enabled: bool,
    pub ai_difficulty: String,
    pub ai_color: Option<String>,
    pub updated_ts_ms: i64,
}

impl Default for ChessState {
    fn default() -> Self {
        Self {
            fen: Board::default().to_string(),
            status: "active".to_string(),
            winner_color: None,
            white_user_id: None,
            black_user_id: None,
            last_move: None,
            reset_requested_white: false,
            reset_requested_black: false,
            ai_enabled: false,
            ai_difficulty: "medium".to_string(),
            ai_color: None,
            updated_ts_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectFourState {
    pub board: [u8; CONNECT_FOUR_CELLS],
    pub turn: String,
    pub status: String,
    pub winner_color: Option<String>,
    pub red_user_id: Option<String>,
    pub yellow_user_id: Option<String>,
    pub ai_enabled: bool,
    pub ai_difficulty: String,
    pub ai_color: Option<String>,
    pub last_move_row: Option<u8>,
    pub last_move_col: Option<u8>,
    pub reset_requested_red: bool,
    pub reset_requested_yellow: bool,
    pub updated_ts_ms: i64,
}

impl Default for ConnectFourState {
    fn default() -> Self {
        Self {
            board: [0; CONNECT_FOUR_CELLS],
            turn: "red".to_string(),
            status: "active".to_string(),
            winner_color: None,
            red_user_id: None,
            yellow_user_id: None,
            ai_enabled: false,
            ai_difficulty: "medium".to_string(),
            ai_color: None,
            last_move_row: None,
            last_move_col: None,
            reset_requested_red: false,
            reset_requested_yellow: false,
            updated_ts_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BattleshipLastShot {
    pub by_color: String,
    pub x: u8,
    pub y: u8,
    pub result: String,
}

#[derive(Debug, Clone)]
pub struct BattleshipState {
    pub phase: String,
    pub status: String,
    pub turn_color: String,
    pub winner_color: Option<String>,
    pub blue_user_id: Option<String>,
    pub red_user_id: Option<String>,
    pub ai_enabled: bool,
    pub ai_difficulty: String,
    pub ai_color: Option<String>,
    pub blue_ready: bool,
    pub red_ready: bool,
    pub blue_ships: [u8; BATTLESHIP_BOARD_CELLS],
    pub red_ships: [u8; BATTLESHIP_BOARD_CELLS],
    pub blue_shots: [u8; BATTLESHIP_BOARD_CELLS],
    pub red_shots: [u8; BATTLESHIP_BOARD_CELLS],
    pub remaining_ship_cells_blue: u16,
    pub remaining_ship_cells_red: u16,
    pub last_shot: Option<BattleshipLastShot>,
    pub reset_requested_blue: bool,
    pub reset_requested_red: bool,
    pub updated_ts_ms: i64,
}

impl Default for BattleshipState {
    fn default() -> Self {
        Self {
            phase: "setup".to_string(),
            status: "setup".to_string(),
            turn_color: "blue".to_string(),
            winner_color: None,
            blue_user_id: None,
            red_user_id: None,
            ai_enabled: false,
            ai_difficulty: "medium".to_string(),
            ai_color: None,
            blue_ready: false,
            red_ready: false,
            blue_ships: [0; BATTLESHIP_BOARD_CELLS],
            red_ships: [0; BATTLESHIP_BOARD_CELLS],
            blue_shots: [0; BATTLESHIP_BOARD_CELLS],
            red_shots: [0; BATTLESHIP_BOARD_CELLS],
            remaining_ship_cells_blue: 0,
            remaining_ship_cells_red: 0,
            last_shot: None,
            reset_requested_blue: false,
            reset_requested_red: false,
            updated_ts_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayState {
    pub active_game: String,
    pub chess: ChessState,
    pub connect_four: ConnectFourState,
    pub battleship: BattleshipState,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone)]
pub struct PresenceMemberSnapshot {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Clone)]
struct PresenceMembersCache {
    updated_ts_ms: i64,
    members: Vec<PresenceMemberSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChessResetOutcome {
    Applied,
    AwaitingOtherPlayer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectFourResetOutcome {
    Applied,
    AwaitingOtherPlayer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleshipResetOutcome {
    Applied,
    AwaitingOtherPlayer,
}

impl Default for PlayState {
    fn default() -> Self {
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self {
            active_game: "chess".to_string(),
            chess: ChessState::default(),
            connect_four: ConnectFourState::default(),
            battleship: BattleshipState::default(),
            updated_ts_ms: now_ms,
        }
    }
}

pub struct RoomRuntime {
    pub room_id: String,
    pub item_id: String,
    pub room_mode: String,
    pub audio_source: Option<String>,
    pub audio_library_id: Option<String>,
    pub state: RwLock<PlaybackState>,
    pub audio_queue: Option<RwLock<AudioQueueState>>,
    pub youtube_video_id: RwLock<Option<String>>,
    pub youtube_queue: RwLock<Vec<String>>,
    pub youtube_search_query: RwLock<String>,
    pub youtube_search_results: RwLock<Vec<YouTubeSearchEntry>>,
    pub web_url: RwLock<String>,
    pub web_updated_ts_ms: RwLock<i64>,
    pub create_state: Option<RwLock<CreateState>>,
    pub play_state: Option<RwLock<PlayState>>,
    presence_members_cache: RwLock<Option<PresenceMembersCache>>,
    pub connected_user_ids: RwLock<HashSet<String>>,
    pub tx: broadcast::Sender<ServerMessage>,
    pub last_activity_ts_ms: RwLock<i64>,
}

impl RoomRuntime {
    fn sync_audio_position_to_now(queue: &mut AudioQueueState, now_ms: i64) {
        if queue.playing && now_ms > queue.updated_ts_ms {
            let elapsed_ms = (now_ms - queue.updated_ts_ms) as u64;
            queue.position_ms = queue.position_ms.saturating_add(elapsed_ms);
        }
        queue.updated_ts_ms = now_ms;
    }

    pub fn new(room_id: String, item_id: String) -> Self {
        Self::new_video(room_id, item_id)
    }

    pub fn new_video(room_id: String, item_id: String) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            room_id,
            item_id,
            room_mode: "video".to_string(),
            audio_source: None,
            audio_library_id: None,
            state: RwLock::new(PlaybackState::default()),
            audio_queue: None,
            youtube_video_id: RwLock::new(None),
            youtube_queue: RwLock::new(Vec::new()),
            youtube_search_query: RwLock::new(String::new()),
            youtube_search_results: RwLock::new(Vec::new()),
            web_url: RwLock::new(String::new()),
            web_updated_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
            create_state: None,
            play_state: None,
            presence_members_cache: RwLock::new(None),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
        }
    }

    pub fn new_audio(
        room_id: String,
        item_id: String,
        audio_source: String,
        audio_library_id: Option<String>,
        track_ids: Vec<String>,
    ) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            room_id,
            item_id,
            room_mode: "audio".to_string(),
            audio_source: Some(audio_source),
            audio_library_id,
            state: RwLock::new(PlaybackState::default()),
            audio_queue: Some(RwLock::new(AudioQueueState {
                track_ids,
                ..AudioQueueState::default()
            })),
            youtube_video_id: RwLock::new(None),
            youtube_queue: RwLock::new(Vec::new()),
            youtube_search_query: RwLock::new(String::new()),
            youtube_search_results: RwLock::new(Vec::new()),
            web_url: RwLock::new(String::new()),
            web_updated_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
            create_state: None,
            play_state: None,
            presence_members_cache: RwLock::new(None),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
        }
    }

    pub fn new_youtube(room_id: String, initial_video_id: Option<String>) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            room_id,
            item_id: String::new(),
            room_mode: "youtube".to_string(),
            audio_source: None,
            audio_library_id: None,
            state: RwLock::new(PlaybackState::default()),
            audio_queue: None,
            youtube_video_id: RwLock::new(initial_video_id),
            youtube_queue: RwLock::new(Vec::new()),
            youtube_search_query: RwLock::new(String::new()),
            youtube_search_results: RwLock::new(Vec::new()),
            web_url: RwLock::new(String::new()),
            web_updated_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
            create_state: None,
            play_state: None,
            presence_members_cache: RwLock::new(None),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
        }
    }

    pub fn new_web(room_id: String, initial_url: String) -> Self {
        let (tx, _) = broadcast::channel(256);
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self {
            room_id,
            item_id: String::new(),
            room_mode: "web".to_string(),
            audio_source: None,
            audio_library_id: None,
            state: RwLock::new(PlaybackState::default()),
            audio_queue: None,
            youtube_video_id: RwLock::new(None),
            youtube_queue: RwLock::new(Vec::new()),
            youtube_search_query: RwLock::new(String::new()),
            youtube_search_results: RwLock::new(Vec::new()),
            web_url: RwLock::new(initial_url),
            web_updated_ts_ms: RwLock::new(now_ms),
            create_state: None,
            play_state: None,
            presence_members_cache: RwLock::new(None),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(now_ms),
        }
    }

    pub fn new_create(room_id: String, initial_state: CreateState) -> Self {
        let (tx, _) = broadcast::channel(256);
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self {
            room_id,
            item_id: String::new(),
            room_mode: "create".to_string(),
            audio_source: None,
            audio_library_id: None,
            state: RwLock::new(PlaybackState::default()),
            audio_queue: None,
            youtube_video_id: RwLock::new(None),
            youtube_queue: RwLock::new(Vec::new()),
            youtube_search_query: RwLock::new(String::new()),
            youtube_search_results: RwLock::new(Vec::new()),
            web_url: RwLock::new(String::new()),
            web_updated_ts_ms: RwLock::new(now_ms),
            create_state: Some(RwLock::new(initial_state)),
            play_state: None,
            presence_members_cache: RwLock::new(None),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(now_ms),
        }
    }

    pub fn new_play(room_id: String) -> Self {
        let (tx, _) = broadcast::channel(256);
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self {
            room_id,
            item_id: String::new(),
            room_mode: "play".to_string(),
            audio_source: None,
            audio_library_id: None,
            state: RwLock::new(PlaybackState::default()),
            audio_queue: None,
            youtube_video_id: RwLock::new(None),
            youtube_queue: RwLock::new(Vec::new()),
            youtube_search_query: RwLock::new(String::new()),
            youtube_search_results: RwLock::new(Vec::new()),
            web_url: RwLock::new(String::new()),
            web_updated_ts_ms: RwLock::new(now_ms),
            create_state: None,
            play_state: Some(RwLock::new(PlayState::default())),
            presence_members_cache: RwLock::new(None),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(now_ms),
        }
    }

    pub async fn get_youtube_video_id(&self) -> Option<String> {
        self.youtube_video_id.read().await.clone()
    }

    pub async fn set_youtube_video_id(&self, video_id: String) {
        let mut guard = self.youtube_video_id.write().await;
        *guard = Some(video_id);
        drop(guard);
        // Reset playback state when video changes
        let mut state = self.state.write().await;
        state.playing = false;
        state.position_ms = 0;
        state.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = state.updated_ts_ms;
    }

    pub async fn snapshot_youtube_queue(&self) -> Vec<String> {
        self.youtube_queue.read().await.clone()
    }

    pub async fn snapshot_youtube_search(&self) -> (String, Vec<YouTubeSearchEntry>) {
        (
            self.youtube_search_query.read().await.clone(),
            self.youtube_search_results.read().await.clone(),
        )
    }

    pub async fn get_web_url(&self) -> String {
        self.web_url.read().await.clone()
    }

    pub async fn set_web_url(&self, url: String) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        {
            let mut guard = self.web_url.write().await;
            *guard = url;
        }
        {
            let mut updated = self.web_updated_ts_ms.write().await;
            *updated = now_ms;
        }
        self.touch_activity().await;
    }

    pub async fn snapshot_web_state(&self) -> (String, i64) {
        let url = self.web_url.read().await.clone();
        let updated_ts_ms = *self.web_updated_ts_ms.read().await;
        (url, updated_ts_ms)
    }

    pub async fn snapshot_create_state(&self) -> Option<CreateState> {
        let state = self.create_state.as_ref()?;
        Some(state.read().await.clone())
    }

    pub async fn set_create_tool(&self, tool: String) -> Option<CreateState> {
        let state = self.create_state.as_ref()?;
        let mut guard = state.write().await;
        guard.active_tool = tool;
        guard.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn set_create_document_name(&self, document_name: String) -> Option<CreateState> {
        let state = self.create_state.as_ref()?;
        let mut guard = state.write().await;
        guard.document_name = document_name;
        guard.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn set_create_text(
        &self,
        text_content: String,
        text_format: Option<String>,
    ) -> Option<CreateState> {
        let state = self.create_state.as_ref()?;
        let mut guard = state.write().await;
        guard.text_content = text_content;
        if let Some(format) = text_format {
            guard.text_format = format;
        }
        guard.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn set_create_canvas(
        &self,
        canvas_strokes: Vec<CreateCanvasStroke>,
    ) -> Option<CreateState> {
        let state = self.create_state.as_ref()?;
        let mut guard = state.write().await;
        guard.canvas_strokes = canvas_strokes;
        guard.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn append_create_canvas_stroke(
        &self,
        canvas_stroke: CreateCanvasStroke,
    ) -> Option<CreateState> {
        let state = self.create_state.as_ref()?;
        let mut guard = state.write().await;
        guard.canvas_strokes.push(canvas_stroke);
        guard.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn remove_create_canvas_stroke(&self, stroke_id: &str) -> Option<CreateState> {
        let state = self.create_state.as_ref()?;
        let mut guard = state.write().await;
        let before_len = guard.canvas_strokes.len();
        guard.canvas_strokes.retain(|stroke| stroke.id != stroke_id);
        if guard.canvas_strokes.len() == before_len {
            return Some(guard.clone());
        }
        guard.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn clear_create_canvas(&self) -> Option<CreateState> {
        let state = self.create_state.as_ref()?;
        let mut guard = state.write().await;
        if guard.canvas_strokes.is_empty() {
            return Some(guard.clone());
        }
        guard.canvas_strokes.clear();
        guard.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn snapshot_play_state(&self) -> Option<PlayState> {
        let state = self.play_state.as_ref()?;
        Some(state.read().await.clone())
    }

    pub async fn set_play_game(&self, game: String) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let normalized = game.trim().to_ascii_lowercase();
        if !matches!(normalized.as_str(), "chess" | "connect_four" | "battleship") {
            return Err(
                "invalid play game; expected one of: chess, connect_four, battleship".to_string(),
            );
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        guard.active_game = normalized;
        guard.updated_ts_ms = now_ms;
        guard.chess.updated_ts_ms = now_ms;
        guard.connect_four.updated_ts_ms = now_ms;
        guard.battleship.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn set_chess_players(
        &self,
        actor_user_id: &str,
        white_user_id: Option<String>,
        black_user_id: Option<String>,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let white = white_user_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let black = black_user_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if white.is_some() && white == black {
            return Err("white and black seats must be assigned to different users".to_string());
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;

        let current_white = guard.chess.white_user_id.clone();
        let current_black = guard.chess.black_user_id.clone();

        validate_chess_seat_change(
            "white",
            current_white.as_deref(),
            white.as_deref(),
            actor_user_id,
        )?;
        validate_chess_seat_change(
            "black",
            current_black.as_deref(),
            black.as_deref(),
            actor_user_id,
        )?;

        // Explicit player assignment means local (non-AI) control.
        guard.chess.ai_enabled = false;
        guard.chess.ai_color = None;
        guard.chess.white_user_id = white;
        guard.chess.black_user_id = black;
        clear_chess_reset_requests(&mut guard.chess);
        guard.updated_ts_ms = now_ms;
        guard.chess.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn reset_chess(&self) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        let white_user_id = guard.chess.white_user_id.clone();
        let black_user_id = guard.chess.black_user_id.clone();
        let ai_enabled = guard.chess.ai_enabled;
        let ai_difficulty = guard.chess.ai_difficulty.clone();
        let ai_color = guard.chess.ai_color.clone();
        let next_chess = fresh_chess_state(
            white_user_id,
            black_user_id,
            ai_enabled,
            ai_difficulty,
            ai_color,
            now_ms,
        );
        guard.active_game = "chess".to_string();
        guard.chess = next_chess;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn request_chess_reset(
        &self,
        requester_user_id: &str,
    ) -> Result<(Option<PlayState>, ChessResetOutcome), String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok((None, ChessResetOutcome::Applied)),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "chess" {
            return Err("active play game is not chess".to_string());
        }

        let white = guard.chess.white_user_id.clone();
        let black = guard.chess.black_user_id.clone();
        let has_white = white.is_some();
        let has_black = black.is_some();

        if !has_white && !has_black {
            let white_user_id = guard.chess.white_user_id.clone();
            let black_user_id = guard.chess.black_user_id.clone();
            let ai_enabled = guard.chess.ai_enabled;
            let ai_difficulty = guard.chess.ai_difficulty.clone();
            let ai_color = guard.chess.ai_color.clone();
            let next_chess = fresh_chess_state(
                white_user_id,
                black_user_id,
                ai_enabled,
                ai_difficulty,
                ai_color,
                now_ms,
            );
            guard.active_game = "chess".to_string();
            guard.chess = next_chess;
            guard.updated_ts_ms = now_ms;
            let snapshot = guard.clone();
            drop(guard);
            self.touch_activity().await;
            return Ok((Some(snapshot), ChessResetOutcome::Applied));
        }

        let requester_is_white = white.as_deref() == Some(requester_user_id);
        let requester_is_black = black.as_deref() == Some(requester_user_id);
        if !requester_is_white && !requester_is_black {
            return Err("only active players can request a board reset".to_string());
        }

        let two_players_assigned = has_white && has_black && white != black;
        if two_players_assigned {
            if requester_is_white {
                guard.chess.reset_requested_white = true;
            }
            if requester_is_black {
                guard.chess.reset_requested_black = true;
            }

            if !(guard.chess.reset_requested_white && guard.chess.reset_requested_black) {
                guard.chess.updated_ts_ms = now_ms;
                guard.updated_ts_ms = now_ms;
                let snapshot = guard.clone();
                drop(guard);
                self.touch_activity().await;
                return Ok((Some(snapshot), ChessResetOutcome::AwaitingOtherPlayer));
            }
        }

        let white_user_id = guard.chess.white_user_id.clone();
        let black_user_id = guard.chess.black_user_id.clone();
        let ai_enabled = guard.chess.ai_enabled;
        let ai_difficulty = guard.chess.ai_difficulty.clone();
        let ai_color = guard.chess.ai_color.clone();
        let next_chess = fresh_chess_state(
            white_user_id,
            black_user_id,
            ai_enabled,
            ai_difficulty,
            ai_color,
            now_ms,
        );
        guard.active_game = "chess".to_string();
        guard.chess = next_chess;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok((Some(snapshot), ChessResetOutcome::Applied))
    }

    pub async fn apply_chess_move(
        &self,
        user_id: &str,
        from: &str,
        to: &str,
        promotion: Option<&str>,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "chess" {
            return Err("active play game is not chess".to_string());
        }

        let board = Board::from_str(&guard.chess.fen)
            .map_err(|_| "failed to load chess board state".to_string())?;
        let side_to_move = board.side_to_move();

        if guard.chess.ai_enabled
            && guard.chess.ai_color.as_deref().and_then(parse_color_name) == Some(side_to_move)
        {
            return Err("it is AI's turn".to_string());
        }

        let assigned_user = match side_to_move {
            Color::White => guard.chess.white_user_id.as_deref(),
            Color::Black => guard.chess.black_user_id.as_deref(),
        };
        if let Some(assigned_user_id) = assigned_user {
            if assigned_user_id != user_id {
                return Err(format!(
                    "only the assigned {} player may move right now",
                    color_name(side_to_move)
                ));
            }
        }

        let from_square = parse_square(from)?;
        let to_square = parse_square(to)?;
        let promotion_piece = parse_promotion_piece(promotion)?;
        let mv = ChessMove::new(from_square, to_square, promotion_piece);

        if !MoveGen::new_legal(&board).any(|candidate| candidate == mv) {
            return Err("illegal chess move".to_string());
        }

        let next_board = board.make_move_new(mv);
        let (status, winner_color) = chess_status(&next_board);

        guard.chess.fen = next_board.to_string();
        guard.chess.status = status;
        guard.chess.winner_color = winner_color;
        guard.chess.last_move = Some(ChessLastMove {
            from: square_to_string(from_square),
            to: square_to_string(to_square),
            promotion: promotion_piece.map(piece_to_promotion),
        });
        clear_chess_reset_requests(&mut guard.chess);
        guard.chess.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;

        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn configure_chess_ai(
        &self,
        configured_by_user_id: &str,
        enabled: bool,
        difficulty: Option<String>,
        human_color: Option<String>,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "chess" {
            return Err("active play game is not chess".to_string());
        }

        if enabled {
            let difficulty = normalize_ai_difficulty(
                difficulty
                    .as_deref()
                    .unwrap_or(guard.chess.ai_difficulty.as_str()),
            )?;
            let preferred_human_color = human_color
                .as_deref()
                .and_then(parse_color_name)
                .or_else(|| {
                    if guard.chess.white_user_id.as_deref() == Some(configured_by_user_id) {
                        Some(Color::White)
                    } else if guard.chess.black_user_id.as_deref() == Some(configured_by_user_id) {
                        Some(Color::Black)
                    } else {
                        None
                    }
                })
                .unwrap_or(Color::White);

            let ai_color = opposite_color(preferred_human_color);
            guard.chess.ai_enabled = true;
            guard.chess.ai_difficulty = difficulty.to_string();
            guard.chess.ai_color = Some(color_name(ai_color).to_string());

            match preferred_human_color {
                Color::White => {
                    guard.chess.white_user_id = Some(configured_by_user_id.to_string());
                    guard.chess.black_user_id = None;
                }
                Color::Black => {
                    guard.chess.black_user_id = Some(configured_by_user_id.to_string());
                    guard.chess.white_user_id = None;
                }
            }
        } else {
            guard.chess.ai_enabled = false;
            guard.chess.ai_color = None;
        }

        clear_chess_reset_requests(&mut guard.chess);
        guard.updated_ts_ms = now_ms;
        guard.chess.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn apply_chess_ai_move_if_needed(&self) -> Result<bool, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(false),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "chess" || !guard.chess.ai_enabled || guard.chess.status != "active"
        {
            return Ok(false);
        }

        let board = Board::from_str(&guard.chess.fen)
            .map_err(|_| "failed to load chess board state".to_string())?;
        let ai_color = match guard.chess.ai_color.as_deref().and_then(parse_color_name) {
            Some(color) => color,
            None => return Ok(false),
        };
        if board.side_to_move() != ai_color {
            return Ok(false);
        }

        let legal_moves: Vec<ChessMove> = MoveGen::new_legal(&board).collect();
        if legal_moves.is_empty() {
            return Ok(false);
        }

        let selected_move =
            select_ai_move(&board, &legal_moves, ai_color, &guard.chess.ai_difficulty);
        let next_board = board.make_move_new(selected_move);
        let (status, winner_color) = chess_status(&next_board);

        guard.chess.fen = next_board.to_string();
        guard.chess.status = status;
        guard.chess.winner_color = winner_color;
        guard.chess.last_move = Some(ChessLastMove {
            from: square_to_string(selected_move.get_source()),
            to: square_to_string(selected_move.get_dest()),
            promotion: selected_move.get_promotion().map(piece_to_promotion),
        });
        clear_chess_reset_requests(&mut guard.chess);
        guard.chess.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;
        drop(guard);
        self.touch_activity().await;
        Ok(true)
    }

    pub async fn configure_connect_four_ai(
        &self,
        configured_by_user_id: &str,
        enabled: bool,
        difficulty: Option<String>,
        human_color: Option<String>,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "connect_four" {
            return Err("active play game is not connect_four".to_string());
        }

        if enabled {
            let difficulty = normalize_ai_difficulty(
                difficulty
                    .as_deref()
                    .unwrap_or(guard.connect_four.ai_difficulty.as_str()),
            )?;
            let preferred_human_color = human_color
                .as_deref()
                .and_then(parse_connect_four_color_name)
                .or_else(|| {
                    if guard.connect_four.red_user_id.as_deref() == Some(configured_by_user_id) {
                        Some(ConnectFourColor::Red)
                    } else if guard.connect_four.yellow_user_id.as_deref()
                        == Some(configured_by_user_id)
                    {
                        Some(ConnectFourColor::Yellow)
                    } else {
                        None
                    }
                })
                .unwrap_or(ConnectFourColor::Red);

            let ai_color = opposite_connect_four_color(preferred_human_color);
            guard.connect_four.ai_enabled = true;
            guard.connect_four.ai_difficulty = difficulty.to_string();
            guard.connect_four.ai_color = Some(connect_four_color_name(ai_color).to_string());

            match preferred_human_color {
                ConnectFourColor::Red => {
                    guard.connect_four.red_user_id = Some(configured_by_user_id.to_string());
                    guard.connect_four.yellow_user_id = None;
                }
                ConnectFourColor::Yellow => {
                    guard.connect_four.yellow_user_id = Some(configured_by_user_id.to_string());
                    guard.connect_four.red_user_id = None;
                }
            }
        } else {
            guard.connect_four.ai_enabled = false;
            guard.connect_four.ai_color = None;
        }

        guard.connect_four.reset_requested_red = false;
        guard.connect_four.reset_requested_yellow = false;
        guard.connect_four.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn apply_connect_four_ai_move_if_needed(&self) -> Result<bool, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(false),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "connect_four"
            || !guard.connect_four.ai_enabled
            || guard.connect_four.status != "active"
        {
            return Ok(false);
        }

        let ai_color = match guard
            .connect_four
            .ai_color
            .as_deref()
            .and_then(parse_connect_four_color_name)
        {
            Some(color) => color,
            None => return Ok(false),
        };
        if connect_four_turn_to_color(&guard.connect_four.turn) != ai_color {
            return Ok(false);
        }

        let selected_column = match select_connect_four_ai_column(
            &guard.connect_four.board,
            ai_color,
            &guard.connect_four.ai_difficulty,
        ) {
            Some(column) => column,
            None => return Ok(false),
        };

        let token = connect_four_color_to_token(ai_color);
        let Some(row) =
            connect_four_drop_in_place(&mut guard.connect_four.board, selected_column, token)
        else {
            return Ok(false);
        };

        if connect_four_has_winning_line(&guard.connect_four.board, row, selected_column, token) {
            guard.connect_four.status = "win".to_string();
            guard.connect_four.winner_color = Some(connect_four_color_name(ai_color).to_string());
        } else if guard.connect_four.board.iter().all(|cell| *cell != 0) {
            guard.connect_four.status = "draw".to_string();
            guard.connect_four.winner_color = None;
        } else {
            guard.connect_four.turn =
                connect_four_color_name(opposite_connect_four_color(ai_color)).to_string();
        }

        guard.connect_four.last_move_row = Some(row as u8);
        guard.connect_four.last_move_col = Some(selected_column as u8);
        guard.connect_four.reset_requested_red = false;
        guard.connect_four.reset_requested_yellow = false;
        guard.connect_four.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;
        drop(guard);
        self.touch_activity().await;
        Ok(true)
    }

    pub async fn set_connect_four_players(
        &self,
        actor_user_id: &str,
        red_user_id: Option<String>,
        yellow_user_id: Option<String>,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let red = red_user_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let yellow = yellow_user_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if red.is_some() && red == yellow {
            return Err("red and yellow seats must be assigned to different users".to_string());
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;

        let current_red = guard.connect_four.red_user_id.clone();
        let current_yellow = guard.connect_four.yellow_user_id.clone();
        validate_chess_seat_change("red", current_red.as_deref(), red.as_deref(), actor_user_id)?;
        validate_chess_seat_change(
            "yellow",
            current_yellow.as_deref(),
            yellow.as_deref(),
            actor_user_id,
        )?;

        let next_state =
            fresh_connect_four_state(red, yellow, false, "medium".to_string(), None, now_ms);
        guard.connect_four = next_state;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn request_connect_four_reset(
        &self,
        requester_user_id: &str,
    ) -> Result<(Option<PlayState>, ConnectFourResetOutcome), String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok((None, ConnectFourResetOutcome::Applied)),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "connect_four" {
            return Err("active play game is not connect_four".to_string());
        }

        let red = guard.connect_four.red_user_id.clone();
        let yellow = guard.connect_four.yellow_user_id.clone();
        let has_red = red.is_some();
        let has_yellow = yellow.is_some();

        if !has_red && !has_yellow {
            let ai_enabled = guard.connect_four.ai_enabled;
            let ai_difficulty = guard.connect_four.ai_difficulty.clone();
            let ai_color = guard.connect_four.ai_color.clone();
            let next_state =
                fresh_connect_four_state(red, yellow, ai_enabled, ai_difficulty, ai_color, now_ms);
            guard.connect_four = next_state;
            guard.updated_ts_ms = now_ms;
            let snapshot = guard.clone();
            drop(guard);
            self.touch_activity().await;
            return Ok((Some(snapshot), ConnectFourResetOutcome::Applied));
        }

        let requester_is_red = red.as_deref() == Some(requester_user_id);
        let requester_is_yellow = yellow.as_deref() == Some(requester_user_id);
        if !requester_is_red && !requester_is_yellow {
            return Err("only active players can request a board reset".to_string());
        }

        let two_players_assigned = has_red && has_yellow && red != yellow;
        if two_players_assigned {
            if requester_is_red {
                guard.connect_four.reset_requested_red = true;
            }
            if requester_is_yellow {
                guard.connect_four.reset_requested_yellow = true;
            }

            if !(guard.connect_four.reset_requested_red
                && guard.connect_four.reset_requested_yellow)
            {
                guard.connect_four.updated_ts_ms = now_ms;
                guard.updated_ts_ms = now_ms;
                let snapshot = guard.clone();
                drop(guard);
                self.touch_activity().await;
                return Ok((Some(snapshot), ConnectFourResetOutcome::AwaitingOtherPlayer));
            }
        }

        let ai_enabled = guard.connect_four.ai_enabled;
        let ai_difficulty = guard.connect_four.ai_difficulty.clone();
        let ai_color = guard.connect_four.ai_color.clone();
        let next_state =
            fresh_connect_four_state(red, yellow, ai_enabled, ai_difficulty, ai_color, now_ms);
        guard.connect_four = next_state;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok((Some(snapshot), ConnectFourResetOutcome::Applied))
    }

    pub async fn apply_connect_four_drop(
        &self,
        user_id: &str,
        column: usize,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        if column >= CONNECT_FOUR_COLS {
            return Err("column out of range".to_string());
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "connect_four" {
            return Err("active play game is not connect_four".to_string());
        }
        if guard.connect_four.status != "active" {
            return Err("connect four game is not active".to_string());
        }
        if guard.connect_four.ai_enabled
            && guard
                .connect_four
                .ai_color
                .as_deref()
                .and_then(parse_connect_four_color_name)
                == Some(connect_four_turn_to_color(&guard.connect_four.turn))
        {
            return Err("it is AI's turn".to_string());
        }

        let (turn_code, turn_name) = if guard.connect_four.turn == "yellow" {
            (2_u8, "yellow")
        } else {
            (1_u8, "red")
        };
        let display_turn_name = if turn_name == "yellow" {
            "blue"
        } else {
            turn_name
        };
        let assigned_user = if turn_code == 1 {
            guard.connect_four.red_user_id.as_deref()
        } else {
            guard.connect_four.yellow_user_id.as_deref()
        };
        let Some(assigned_user_id) = assigned_user else {
            return Err(format!(
                "no {display_turn_name} player is assigned for this turn"
            ));
        };
        if assigned_user_id != user_id {
            return Err(format!(
                "only the assigned {display_turn_name} player may move right now"
            ));
        }

        let Some(row) =
            connect_four_drop_in_place(&mut guard.connect_four.board, column, turn_code)
        else {
            return Err("selected column is full".to_string());
        };

        if connect_four_has_winning_line(&guard.connect_four.board, row, column, turn_code) {
            guard.connect_four.status = "win".to_string();
            guard.connect_four.winner_color = Some(turn_name.to_string());
        } else if guard.connect_four.board.iter().all(|cell| *cell != 0) {
            guard.connect_four.status = "draw".to_string();
            guard.connect_four.winner_color = None;
        } else {
            guard.connect_four.turn = if turn_code == 1 {
                "yellow".to_string()
            } else {
                "red".to_string()
            };
        }

        guard.connect_four.last_move_row = Some(row as u8);
        guard.connect_four.last_move_col = Some(column as u8);
        guard.connect_four.reset_requested_red = false;
        guard.connect_four.reset_requested_yellow = false;
        guard.connect_four.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;

        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn set_battleship_players(
        &self,
        actor_user_id: &str,
        blue_user_id: Option<String>,
        red_user_id: Option<String>,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let blue = blue_user_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let red = red_user_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if blue.is_some() && blue == red {
            return Err("blue and red seats must be assigned to different users".to_string());
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;

        let current_blue = guard.battleship.blue_user_id.clone();
        let current_red = guard.battleship.red_user_id.clone();
        validate_chess_seat_change(
            "blue",
            current_blue.as_deref(),
            blue.as_deref(),
            actor_user_id,
        )?;
        validate_chess_seat_change("red", current_red.as_deref(), red.as_deref(), actor_user_id)?;

        let next_state =
            fresh_battleship_state(blue, red, false, "medium".to_string(), None, now_ms);
        guard.battleship = next_state;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn configure_battleship_ai(
        &self,
        configured_by_user_id: &str,
        enabled: bool,
        difficulty: Option<String>,
        human_color: Option<String>,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "battleship" {
            return Err("active play game is not battleship".to_string());
        }

        if enabled {
            let difficulty = normalize_ai_difficulty(
                difficulty
                    .as_deref()
                    .unwrap_or(guard.battleship.ai_difficulty.as_str()),
            )?;
            let preferred_human_color = human_color
                .as_deref()
                .and_then(parse_battleship_color_name)
                .or_else(|| {
                    if guard.battleship.blue_user_id.as_deref() == Some(configured_by_user_id) {
                        Some(BattleshipColor::Blue)
                    } else if guard.battleship.red_user_id.as_deref() == Some(configured_by_user_id)
                    {
                        Some(BattleshipColor::Red)
                    } else {
                        None
                    }
                })
                .unwrap_or(BattleshipColor::Blue);

            let ai_color = opposite_battleship_color(preferred_human_color);
            let (blue_user_id, red_user_id) = match preferred_human_color {
                BattleshipColor::Blue => (Some(configured_by_user_id.to_string()), None),
                BattleshipColor::Red => (None, Some(configured_by_user_id.to_string())),
            };
            let next_state = fresh_battleship_state(
                blue_user_id,
                red_user_id,
                true,
                difficulty.to_string(),
                Some(battleship_color_name(ai_color).to_string()),
                now_ms,
            );
            guard.battleship = next_state;
        } else {
            guard.battleship.ai_enabled = false;
            guard.battleship.ai_color = None;
            clear_battleship_reset_requests(&mut guard.battleship);
            guard.battleship.updated_ts_ms = now_ms;
        }

        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn battleship_auto_place(
        &self,
        actor_user_id: &str,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "battleship" {
            return Err("active play game is not battleship".to_string());
        }
        if guard.battleship.phase != "setup" {
            return Err("battleship board can only be configured during setup".to_string());
        }

        let actor_color = battleship_color_for_user(&guard.battleship, actor_user_id)
            .ok_or_else(|| "only assigned players can place battleship ships".to_string())?;
        let seed = chrono::Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
            .unsigned_abs()
            ^ (actor_user_id.bytes().fold(0_u64, |acc, byte| {
                acc.wrapping_mul(131).wrapping_add(byte as u64)
            }));
        let ships = generate_battleship_ships(seed)?;

        if actor_color == "blue" {
            guard.battleship.blue_ships = ships;
            guard.battleship.blue_shots = [0; BATTLESHIP_BOARD_CELLS];
            guard.battleship.blue_ready = false;
            guard.battleship.remaining_ship_cells_blue = BATTLESHIP_TOTAL_SHIP_CELLS;
        } else {
            guard.battleship.red_ships = ships;
            guard.battleship.red_shots = [0; BATTLESHIP_BOARD_CELLS];
            guard.battleship.red_ready = false;
            guard.battleship.remaining_ship_cells_red = BATTLESHIP_TOTAL_SHIP_CELLS;
        }

        guard.battleship.status = "setup".to_string();
        guard.battleship.last_shot = None;
        clear_battleship_reset_requests(&mut guard.battleship);
        guard.battleship.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn battleship_place_ship(
        &self,
        actor_user_id: &str,
        ship_id: u8,
        x: u8,
        y: u8,
        orientation: &str,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "battleship" {
            return Err("active play game is not battleship".to_string());
        }
        if guard.battleship.phase != "setup" {
            return Err("battleship board can only be configured during setup".to_string());
        }

        let actor_color = battleship_color_for_user(&guard.battleship, actor_user_id)
            .ok_or_else(|| "only assigned players can place battleship ships".to_string())?;
        let ship_size = battleship_ship_size(ship_id)
            .ok_or_else(|| "invalid battleship ship selection".to_string())?;
        let horizontal = match orientation.trim().to_ascii_lowercase().as_str() {
            "horizontal" | "h" => true,
            "vertical" | "v" => false,
            _ => return Err("invalid battleship ship orientation".to_string()),
        };
        let x = x as usize;
        let y = y as usize;
        if x >= BATTLESHIP_BOARD_SIZE || y >= BATTLESHIP_BOARD_SIZE {
            return Err("battleship ship placement is outside the board".to_string());
        }

        let max_x = if horizontal {
            BATTLESHIP_BOARD_SIZE - ship_size
        } else {
            BATTLESHIP_BOARD_SIZE - 1
        };
        let max_y = if horizontal {
            BATTLESHIP_BOARD_SIZE - 1
        } else {
            BATTLESHIP_BOARD_SIZE - ship_size
        };
        if x > max_x || y > max_y {
            return Err("selected ship does not fit at that board position".to_string());
        }

        let ships = if actor_color == "blue" {
            &mut guard.battleship.blue_ships
        } else {
            &mut guard.battleship.red_ships
        };

        for cell in ships.iter_mut() {
            if *cell == ship_id {
                *cell = 0;
            }
        }

        for offset in 0..ship_size {
            let px = if horizontal { x + offset } else { x };
            let py = if horizontal { y } else { y + offset };
            let idx = py * BATTLESHIP_BOARD_SIZE + px;
            if ships[idx] != 0 {
                return Err("ships cannot overlap in battleship".to_string());
            }
        }

        for offset in 0..ship_size {
            let px = if horizontal { x + offset } else { x };
            let py = if horizontal { y } else { y + offset };
            let idx = py * BATTLESHIP_BOARD_SIZE + px;
            ships[idx] = ship_id;
        }

        let placed_cells = count_ship_cells(ships);
        if actor_color == "blue" {
            guard.battleship.blue_ready = false;
            guard.battleship.remaining_ship_cells_blue = placed_cells;
        } else {
            guard.battleship.red_ready = false;
            guard.battleship.remaining_ship_cells_red = placed_cells;
        }
        guard.battleship.phase = "setup".to_string();
        guard.battleship.status = "setup".to_string();
        guard.battleship.last_shot = None;
        clear_battleship_reset_requests(&mut guard.battleship);
        guard.battleship.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn battleship_set_ready(
        &self,
        actor_user_id: &str,
        ready: bool,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "battleship" {
            return Err("active play game is not battleship".to_string());
        }
        if guard.battleship.phase == "finished" {
            return Err("battleship game is finished; reset to start a new game".to_string());
        }

        let actor_color = battleship_color_for_user(&guard.battleship, actor_user_id)
            .ok_or_else(|| "only assigned players can change ready state".to_string())?;

        if ready {
            if actor_color == "blue" {
                if count_ship_cells(&guard.battleship.blue_ships) != BATTLESHIP_TOTAL_SHIP_CELLS {
                    return Err("blue board must be placed before setting ready".to_string());
                }
                guard.battleship.blue_ready = true;
            } else {
                if count_ship_cells(&guard.battleship.red_ships) != BATTLESHIP_TOTAL_SHIP_CELLS {
                    return Err("red board must be placed before setting ready".to_string());
                }
                guard.battleship.red_ready = true;
            }
        } else if actor_color == "blue" {
            guard.battleship.blue_ready = false;
        } else {
            guard.battleship.red_ready = false;
        }

        let ai_color = guard
            .battleship
            .ai_color
            .as_deref()
            .and_then(parse_battleship_color_name);
        let blue_active =
            guard.battleship.blue_user_id.is_some() || ai_color == Some(BattleshipColor::Blue);
        let red_active =
            guard.battleship.red_user_id.is_some() || ai_color == Some(BattleshipColor::Red);
        if blue_active && red_active && guard.battleship.blue_ready && guard.battleship.red_ready {
            guard.battleship.phase = "active".to_string();
            guard.battleship.status = "active".to_string();
            guard.battleship.turn_color = if now_ms % 2 == 0 {
                "blue".to_string()
            } else {
                "red".to_string()
            };
        } else {
            guard.battleship.phase = "setup".to_string();
            guard.battleship.status = "setup".to_string();
        }

        clear_battleship_reset_requests(&mut guard.battleship);
        guard.battleship.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn apply_battleship_ai_action_if_needed(&self) -> Result<bool, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(false),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "battleship" || !guard.battleship.ai_enabled {
            return Ok(false);
        }

        let ai_color = match guard
            .battleship
            .ai_color
            .as_deref()
            .and_then(parse_battleship_color_name)
        {
            Some(color) => color,
            None => return Ok(false),
        };

        if guard.battleship.phase == "setup" {
            let already_placed = match ai_color {
                BattleshipColor::Blue => {
                    count_ship_cells(&guard.battleship.blue_ships) == BATTLESHIP_TOTAL_SHIP_CELLS
                }
                BattleshipColor::Red => {
                    count_ship_cells(&guard.battleship.red_ships) == BATTLESHIP_TOTAL_SHIP_CELLS
                }
            };

            let needs_ready = match ai_color {
                BattleshipColor::Blue => !guard.battleship.blue_ready,
                BattleshipColor::Red => !guard.battleship.red_ready,
            };

            let mut changed = false;
            if !already_placed || needs_ready {
                let seed = chrono::Utc::now()
                    .timestamp_nanos_opt()
                    .unwrap_or_default()
                    .unsigned_abs()
                    ^ 0xB47713541u64;
                let ships = if already_placed {
                    None
                } else {
                    Some(generate_battleship_ships(seed)?)
                };

                match ai_color {
                    BattleshipColor::Blue => {
                        if let Some(ships) = ships {
                            guard.battleship.blue_ships = ships;
                            guard.battleship.blue_shots = [0; BATTLESHIP_BOARD_CELLS];
                            guard.battleship.remaining_ship_cells_blue =
                                BATTLESHIP_TOTAL_SHIP_CELLS;
                            changed = true;
                        }
                        changed |= !guard.battleship.blue_ready;
                        guard.battleship.blue_ready = true;
                    }
                    BattleshipColor::Red => {
                        if let Some(ships) = ships {
                            guard.battleship.red_ships = ships;
                            guard.battleship.red_shots = [0; BATTLESHIP_BOARD_CELLS];
                            guard.battleship.remaining_ship_cells_red = BATTLESHIP_TOTAL_SHIP_CELLS;
                            changed = true;
                        }
                        changed |= !guard.battleship.red_ready;
                        guard.battleship.red_ready = true;
                    }
                }
            }
            let blue_active =
                guard.battleship.blue_user_id.is_some() || ai_color == BattleshipColor::Blue;
            let red_active =
                guard.battleship.red_user_id.is_some() || ai_color == BattleshipColor::Red;
            if blue_active
                && red_active
                && guard.battleship.blue_ready
                && guard.battleship.red_ready
            {
                if guard.battleship.phase != "active" || guard.battleship.status != "active" {
                    guard.battleship.phase = "active".to_string();
                    guard.battleship.status = "active".to_string();
                    guard.battleship.turn_color = if now_ms % 2 == 0 {
                        "blue".to_string()
                    } else {
                        "red".to_string()
                    };
                    changed = true;
                }
            } else if guard.battleship.phase != "setup" || guard.battleship.status != "setup" {
                guard.battleship.phase = "setup".to_string();
                guard.battleship.status = "setup".to_string();
                changed = true;
            }

            if changed {
                clear_battleship_reset_requests(&mut guard.battleship);
                guard.battleship.updated_ts_ms = now_ms;
                guard.updated_ts_ms = now_ms;
                drop(guard);
                self.touch_activity().await;
                return Ok(true);
            }
            return Ok(false);
        }

        if guard.battleship.phase != "active" || guard.battleship.status != "active" {
            return Ok(false);
        }
        if parse_battleship_color_name(&guard.battleship.turn_color) != Some(ai_color) {
            return Ok(false);
        }

        let ai_difficulty = guard.battleship.ai_difficulty.clone();
        let (x, y, result, did_win) = match ai_color {
            BattleshipColor::Blue => {
                let target_ships = guard.battleship.red_ships;
                let Some((x, y)) = select_battleship_ai_target(
                    &guard.battleship.red_shots,
                    ai_color,
                    &ai_difficulty,
                ) else {
                    return Ok(false);
                };
                let target_idx = y * BATTLESHIP_BOARD_SIZE + x;
                if guard.battleship.red_shots[target_idx] != 0 {
                    return Ok(false);
                }

                let (result, did_win) = if target_ships[target_idx] == 0 {
                    guard.battleship.red_shots[target_idx] = 1;
                    ("miss".to_string(), false)
                } else {
                    guard.battleship.red_shots[target_idx] = 2;
                    if guard.battleship.remaining_ship_cells_red > 0 {
                        guard.battleship.remaining_ship_cells_red -= 1;
                    }
                    let ship_id = target_ships[target_idx];
                    if guard.battleship.remaining_ship_cells_red == 0 {
                        ("win".to_string(), true)
                    } else if is_battleship_ship_sunk(
                        &target_ships,
                        &guard.battleship.red_shots,
                        ship_id,
                    ) {
                        ("sunk".to_string(), false)
                    } else {
                        ("hit".to_string(), false)
                    }
                };
                (x, y, result, did_win)
            }
            BattleshipColor::Red => {
                let target_ships = guard.battleship.blue_ships;
                let Some((x, y)) = select_battleship_ai_target(
                    &guard.battleship.blue_shots,
                    ai_color,
                    &ai_difficulty,
                ) else {
                    return Ok(false);
                };
                let target_idx = y * BATTLESHIP_BOARD_SIZE + x;
                if guard.battleship.blue_shots[target_idx] != 0 {
                    return Ok(false);
                }

                let (result, did_win) = if target_ships[target_idx] == 0 {
                    guard.battleship.blue_shots[target_idx] = 1;
                    ("miss".to_string(), false)
                } else {
                    guard.battleship.blue_shots[target_idx] = 2;
                    if guard.battleship.remaining_ship_cells_blue > 0 {
                        guard.battleship.remaining_ship_cells_blue -= 1;
                    }
                    let ship_id = target_ships[target_idx];
                    if guard.battleship.remaining_ship_cells_blue == 0 {
                        ("win".to_string(), true)
                    } else if is_battleship_ship_sunk(
                        &target_ships,
                        &guard.battleship.blue_shots,
                        ship_id,
                    ) {
                        ("sunk".to_string(), false)
                    } else {
                        ("hit".to_string(), false)
                    }
                };
                (x, y, result, did_win)
            }
        };

        if did_win {
            guard.battleship.phase = "finished".to_string();
            guard.battleship.status = "finished".to_string();
            guard.battleship.winner_color = Some(battleship_color_name(ai_color).to_string());
        } else {
            guard.battleship.turn_color =
                battleship_color_name(opposite_battleship_color(ai_color)).to_string();
        }

        guard.battleship.last_shot = Some(BattleshipLastShot {
            by_color: battleship_color_name(ai_color).to_string(),
            x: x as u8,
            y: y as u8,
            result,
        });
        clear_battleship_reset_requests(&mut guard.battleship);
        guard.battleship.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;
        drop(guard);
        self.touch_activity().await;
        Ok(true)
    }

    pub async fn battleship_fire(
        &self,
        actor_user_id: &str,
        x: u8,
        y: u8,
    ) -> Result<Option<PlayState>, String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        if x as usize >= BATTLESHIP_BOARD_SIZE || y as usize >= BATTLESHIP_BOARD_SIZE {
            return Err("shot coordinates are out of range".to_string());
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "battleship" {
            return Err("active play game is not battleship".to_string());
        }
        if guard.battleship.phase != "active" || guard.battleship.status != "active" {
            return Err("battleship game is not active".to_string());
        }

        let actor_color = battleship_color_for_user(&guard.battleship, actor_user_id)
            .ok_or_else(|| "only assigned players can fire in battleship".to_string())?;
        if guard.battleship.turn_color != actor_color {
            return Err("it is not your turn".to_string());
        }

        let target_idx = y as usize * BATTLESHIP_BOARD_SIZE + x as usize;
        let (result, did_win) = if actor_color == "blue" {
            let target_ships = guard.battleship.red_ships;
            if guard.battleship.red_shots[target_idx] != 0 {
                return Err("that coordinate has already been targeted".to_string());
            }

            if target_ships[target_idx] == 0 {
                guard.battleship.red_shots[target_idx] = 1;
                ("miss".to_string(), false)
            } else {
                guard.battleship.red_shots[target_idx] = 2;
                if guard.battleship.remaining_ship_cells_red > 0 {
                    guard.battleship.remaining_ship_cells_red -= 1;
                }
                let ship_id = target_ships[target_idx];
                if guard.battleship.remaining_ship_cells_red == 0 {
                    ("win".to_string(), true)
                } else if is_battleship_ship_sunk(
                    &target_ships,
                    &guard.battleship.red_shots,
                    ship_id,
                ) {
                    ("sunk".to_string(), false)
                } else {
                    ("hit".to_string(), false)
                }
            }
        } else {
            let target_ships = guard.battleship.blue_ships;
            if guard.battleship.blue_shots[target_idx] != 0 {
                return Err("that coordinate has already been targeted".to_string());
            }

            if target_ships[target_idx] == 0 {
                guard.battleship.blue_shots[target_idx] = 1;
                ("miss".to_string(), false)
            } else {
                guard.battleship.blue_shots[target_idx] = 2;
                if guard.battleship.remaining_ship_cells_blue > 0 {
                    guard.battleship.remaining_ship_cells_blue -= 1;
                }
                let ship_id = target_ships[target_idx];
                if guard.battleship.remaining_ship_cells_blue == 0 {
                    ("win".to_string(), true)
                } else if is_battleship_ship_sunk(
                    &target_ships,
                    &guard.battleship.blue_shots,
                    ship_id,
                ) {
                    ("sunk".to_string(), false)
                } else {
                    ("hit".to_string(), false)
                }
            }
        };

        if did_win {
            guard.battleship.phase = "finished".to_string();
            guard.battleship.status = "finished".to_string();
            guard.battleship.winner_color = Some(actor_color.to_string());
        }

        guard.battleship.last_shot = Some(BattleshipLastShot {
            by_color: actor_color.to_string(),
            x,
            y,
            result,
        });

        if guard.battleship.phase == "active" {
            guard.battleship.turn_color = if actor_color == "blue" {
                "red".to_string()
            } else {
                "blue".to_string()
            };
        }

        clear_battleship_reset_requests(&mut guard.battleship);
        guard.battleship.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn request_battleship_reset(
        &self,
        requester_user_id: &str,
    ) -> Result<(Option<PlayState>, BattleshipResetOutcome), String> {
        let state = match self.play_state.as_ref() {
            Some(state) => state,
            None => return Ok((None, BattleshipResetOutcome::Applied)),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        if guard.active_game != "battleship" {
            return Err("active play game is not battleship".to_string());
        }

        let blue = guard.battleship.blue_user_id.clone();
        let red = guard.battleship.red_user_id.clone();
        let has_blue = blue.is_some();
        let has_red = red.is_some();

        if !has_blue && !has_red {
            let ai_enabled = guard.battleship.ai_enabled;
            let ai_difficulty = guard.battleship.ai_difficulty.clone();
            let ai_color = guard.battleship.ai_color.clone();
            let next_state =
                fresh_battleship_state(blue, red, ai_enabled, ai_difficulty, ai_color, now_ms);
            guard.battleship = next_state;
            guard.updated_ts_ms = now_ms;
            let snapshot = guard.clone();
            drop(guard);
            self.touch_activity().await;
            return Ok((Some(snapshot), BattleshipResetOutcome::Applied));
        }

        let requester_is_blue = blue.as_deref() == Some(requester_user_id);
        let requester_is_red = red.as_deref() == Some(requester_user_id);
        if !requester_is_blue && !requester_is_red {
            return Err("only active players can request a board reset".to_string());
        }

        let two_players_assigned = has_blue && has_red && blue != red;
        if two_players_assigned {
            if requester_is_blue {
                guard.battleship.reset_requested_blue = true;
            }
            if requester_is_red {
                guard.battleship.reset_requested_red = true;
            }

            if !(guard.battleship.reset_requested_blue && guard.battleship.reset_requested_red) {
                guard.battleship.updated_ts_ms = now_ms;
                guard.updated_ts_ms = now_ms;
                let snapshot = guard.clone();
                drop(guard);
                self.touch_activity().await;
                return Ok((Some(snapshot), BattleshipResetOutcome::AwaitingOtherPlayer));
            }
        }

        let ai_enabled = guard.battleship.ai_enabled;
        let ai_difficulty = guard.battleship.ai_difficulty.clone();
        let ai_color = guard.battleship.ai_color.clone();
        let next_state =
            fresh_battleship_state(blue, red, ai_enabled, ai_difficulty, ai_color, now_ms);
        guard.battleship = next_state;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok((Some(snapshot), BattleshipResetOutcome::Applied))
    }

    pub async fn set_youtube_search_state(
        &self,
        search_query: String,
        search_results: Vec<YouTubeSearchEntry>,
    ) {
        {
            let mut query = self.youtube_search_query.write().await;
            *query = search_query;
        }
        {
            let mut results = self.youtube_search_results.write().await;
            *results = search_results;
        }
        self.touch_activity().await;
    }

    pub async fn youtube_queue_video_at(&self, queue_index: usize) -> Option<String> {
        self.youtube_queue.read().await.get(queue_index).cloned()
    }

    pub async fn enqueue_youtube_video(&self, video_id: String) -> Vec<String> {
        let mut queue = self.youtube_queue.write().await;
        queue.push(video_id);
        let snapshot = queue.clone();
        drop(queue);
        self.touch_activity().await;
        snapshot
    }

    pub async fn enqueue_youtube_video_unique(&self, video_id: String) -> Option<Vec<String>> {
        if self.get_youtube_video_id().await.as_deref() == Some(video_id.as_str()) {
            return None;
        }

        let mut queue = self.youtube_queue.write().await;
        if queue.iter().any(|queued| queued == &video_id) {
            return None;
        }

        queue.push(video_id);
        let snapshot = queue.clone();
        drop(queue);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn remove_youtube_queue_index(&self, queue_index: usize) -> Option<Vec<String>> {
        let mut queue = self.youtube_queue.write().await;
        if queue_index >= queue.len() {
            return None;
        }
        queue.remove(queue_index);
        let snapshot = queue.clone();
        drop(queue);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn move_youtube_queue_item(
        &self,
        from_index: usize,
        to_index: usize,
    ) -> Option<Vec<String>> {
        let mut queue = self.youtube_queue.write().await;
        if from_index >= queue.len() || to_index >= queue.len() {
            return None;
        }
        if from_index != to_index {
            let item = queue.remove(from_index);
            queue.insert(to_index, item);
        }
        let snapshot = queue.clone();
        drop(queue);
        self.touch_activity().await;
        Some(snapshot)
    }

    pub async fn play_youtube_queue_index_now(&self, queue_index: usize) -> Option<String> {
        let next_video_id = {
            let mut queue = self.youtube_queue.write().await;
            if queue_index >= queue.len() {
                return None;
            }
            queue.remove(queue_index)
        };

        self.set_youtube_video_id(next_video_id.clone()).await;
        self.apply_action(PlaybackAction::Play { position_ms: 0 })
            .await;
        self.touch_activity().await;
        Some(next_video_id)
    }

    /// Advance to the next queued YouTube video if the currently playing video
    /// still matches `expected_current_video_id`. Returns true when advanced.
    pub async fn advance_youtube_queue(&self, expected_current_video_id: &str) -> bool {
        let current_video_id = self.get_youtube_video_id().await.unwrap_or_default();
        if !expected_current_video_id.is_empty() && current_video_id != expected_current_video_id {
            return false;
        }

        let next_video_id = {
            let mut queue = self.youtube_queue.write().await;
            if queue.is_empty() {
                None
            } else {
                Some(queue.remove(0))
            }
        };

        if let Some(next_video_id) = next_video_id {
            self.set_youtube_video_id(next_video_id).await;
            true
        } else {
            let mut state = self.state.write().await;
            state.playing = false;
            state.updated_ts_ms = chrono::Utc::now().timestamp_millis();
            let mut activity = self.last_activity_ts_ms.write().await;
            *activity = state.updated_ts_ms;
            false
        }
    }

    pub async fn snapshot_state(&self) -> PlaybackState {
        self.state.read().await.clone()
    }

    pub async fn snapshot_audio_queue(&self) -> Option<AudioQueueState> {
        let queue = self.audio_queue.as_ref()?;
        Some(queue.read().await.clone())
    }

    /// Append a track to the audio queue. Optionally start playback from it.
    pub async fn enqueue_audio_track(
        &self,
        track_id: String,
        play_now: bool,
    ) -> Option<AudioQueueState> {
        let queue_lock = self.audio_queue.as_ref()?;
        let mut queue = queue_lock.write().await;

        queue.track_ids.push(track_id);
        if play_now {
            queue.current_index = queue.track_ids.len().saturating_sub(1);
            queue.position_ms = 0;
            queue.playing = true;
        } else if queue.track_ids.len() == 1 {
            queue.current_index = 0;
            queue.position_ms = 0;
            queue.playing = false;
        }

        queue.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = queue.updated_ts_ms;

        Some(queue.clone())
    }

    pub async fn set_audio_queue(
        &self,
        track_ids: Vec<String>,
        current_index: usize,
        position_ms: u64,
        playing: bool,
    ) -> Option<AudioQueueState> {
        let queue_lock = self.audio_queue.as_ref()?;
        let mut queue = queue_lock.write().await;
        queue.track_ids = track_ids;
        queue.current_index = current_index.min(queue.track_ids.len().saturating_sub(1));
        queue.position_ms = position_ms;
        queue.playing = playing;
        queue.updated_ts_ms = chrono::Utc::now().timestamp_millis();

        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = queue.updated_ts_ms;

        Some(queue.clone())
    }

    pub async fn reorder_audio_queue(
        &self,
        from_index: usize,
        to_index: usize,
    ) -> Option<AudioQueueState> {
        let queue_lock = self.audio_queue.as_ref()?;
        let mut queue = queue_lock.write().await;
        let len = queue.track_ids.len();
        if from_index >= len || to_index >= len {
            return None;
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self::sync_audio_position_to_now(&mut queue, now_ms);
        if from_index != to_index {
            let current_track_id = queue.track_ids.get(queue.current_index).cloned();
            let moved = queue.track_ids.remove(from_index);
            queue.track_ids.insert(to_index, moved);
            if let Some(current_id) = current_track_id {
                if let Some(next_index) = queue.track_ids.iter().position(|id| id == &current_id) {
                    queue.current_index = next_index;
                } else {
                    queue.current_index = 0;
                }
            }
        }
        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = queue.updated_ts_ms;
        Some(queue.clone())
    }

    pub async fn set_audio_shuffle_enabled(&self, enabled: bool) -> Option<AudioQueueState> {
        let queue_lock = self.audio_queue.as_ref()?;
        let mut queue = queue_lock.write().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self::sync_audio_position_to_now(&mut queue, now_ms);
        queue.shuffle_enabled = enabled;
        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = queue.updated_ts_ms;
        Some(queue.clone())
    }

    pub async fn set_audio_repeat_mode(&self, mode: AudioRepeatMode) -> Option<AudioQueueState> {
        let queue_lock = self.audio_queue.as_ref()?;
        let mut queue = queue_lock.write().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self::sync_audio_position_to_now(&mut queue, now_ms);
        queue.repeat_mode = mode;
        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = queue.updated_ts_ms;
        Some(queue.clone())
    }

    pub async fn handle_audio_track_ended(&self, position_ms: u64) -> Option<AudioQueueState> {
        let queue_lock = self.audio_queue.as_ref()?;
        let mut queue = queue_lock.write().await;
        let len = queue.track_ids.len();

        if len == 0 {
            queue.playing = false;
            queue.position_ms = 0;
        } else if queue.repeat_mode == AudioRepeatMode::Track {
            queue.playing = true;
            queue.position_ms = 0;
        } else if queue.shuffle_enabled && len > 1 {
            let now_ms = chrono::Utc::now().timestamp_millis().unsigned_abs() as usize;
            let mut next_index = now_ms % len;
            if next_index == queue.current_index {
                next_index = (next_index + 1) % len;
            }
            queue.current_index = next_index;
            queue.playing = true;
            queue.position_ms = 0;
        } else if queue.current_index + 1 < len {
            queue.current_index += 1;
            queue.playing = true;
            queue.position_ms = 0;
        } else if queue.repeat_mode == AudioRepeatMode::Queue {
            queue.current_index = 0;
            queue.playing = true;
            queue.position_ms = 0;
        } else {
            queue.playing = false;
            queue.position_ms = position_ms;
        }

        queue.updated_ts_ms = chrono::Utc::now().timestamp_millis();
        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = queue.updated_ts_ms;
        Some(queue.clone())
    }

    pub async fn apply_action(&self, action: PlaybackAction) -> PlaybackState {
        let mut state = self.state.write().await;
        match action {
            PlaybackAction::Play { position_ms } => {
                state.playing = true;
                state.position_ms = position_ms;
            }
            PlaybackAction::Pause { position_ms } => {
                state.playing = false;
                state.position_ms = position_ms;
            }
            PlaybackAction::Seek { position_ms } => {
                state.position_ms = position_ms;
            }
        }
        state.updated_ts_ms = chrono::Utc::now().timestamp_millis();

        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = state.updated_ts_ms;

        state.clone()
    }

    /// Apply an audio action. Returns the updated AudioQueueState if this is an audio room.
    pub async fn apply_audio_action(&self, action: AudioAction) -> Option<AudioQueueState> {
        let queue_lock = self.audio_queue.as_ref()?;
        let mut queue = queue_lock.write().await;
        let len = queue.track_ids.len();

        match action {
            AudioAction::SkipNext => {
                if len > 0 {
                    if queue.shuffle_enabled && len > 1 {
                        let now_ms = chrono::Utc::now().timestamp_millis().unsigned_abs() as usize;
                        let mut next_index = now_ms % len;
                        if next_index == queue.current_index {
                            next_index = (next_index + 1) % len;
                        }
                        queue.current_index = next_index;
                    } else {
                        queue.current_index = (queue.current_index + 1) % len;
                    }
                }
                queue.position_ms = 0;
                queue.playing = true;
            }
            AudioAction::SkipPrev => {
                if len > 0 {
                    queue.current_index = if queue.current_index == 0 {
                        len - 1
                    } else {
                        queue.current_index - 1
                    };
                }
                queue.position_ms = 0;
                queue.playing = true;
            }
            AudioAction::PlayTrack { track_id } => {
                if let Some(idx) = queue.track_ids.iter().position(|id| id == &track_id) {
                    queue.current_index = idx;
                }
                queue.position_ms = 0;
                queue.playing = true;
            }
            AudioAction::SetPlayingState {
                position_ms,
                playing,
            } => {
                queue.position_ms = position_ms;
                queue.playing = playing;
            }
        }

        queue.updated_ts_ms = chrono::Utc::now().timestamp_millis();

        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = queue.updated_ts_ms;

        Some(queue.clone())
    }

    pub async fn touch_activity(&self) {
        let mut activity = self.last_activity_ts_ms.write().await;
        *activity = chrono::Utc::now().timestamp_millis();
    }

    pub async fn last_activity_ts_ms(&self) -> i64 {
        *self.last_activity_ts_ms.read().await
    }

    pub async fn get_presence_members_cache(
        &self,
        now_ms: i64,
        ttl_ms: i64,
    ) -> Option<Vec<PresenceMemberSnapshot>> {
        let cache = self.presence_members_cache.read().await;
        let cache = cache.as_ref()?;
        if now_ms.saturating_sub(cache.updated_ts_ms) > ttl_ms {
            return None;
        }
        Some(cache.members.clone())
    }

    pub async fn set_presence_members_cache(
        &self,
        members: Vec<PresenceMemberSnapshot>,
        now_ms: i64,
    ) {
        let mut cache = self.presence_members_cache.write().await;
        *cache = Some(PresenceMembersCache {
            updated_ts_ms: now_ms,
            members,
        });
    }

    pub async fn invalidate_presence_members_cache(&self) {
        let mut cache = self.presence_members_cache.write().await;
        *cache = None;
    }
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "white",
        Color::Black => "black",
    }
}

fn opposite_color(color: Color) -> Color {
    match color {
        Color::White => Color::Black,
        Color::Black => Color::White,
    }
}

fn parse_color_name(raw: &str) -> Option<Color> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "white" => Some(Color::White),
        "black" => Some(Color::Black),
        _ => None,
    }
}

fn normalize_ai_difficulty(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "easy" => Ok("easy"),
        "medium" => Ok("medium"),
        "hard" => Ok("hard"),
        _ => Err("invalid AI difficulty; expected easy, medium, or hard".to_string()),
    }
}

fn fresh_chess_state(
    white_user_id: Option<String>,
    black_user_id: Option<String>,
    ai_enabled: bool,
    ai_difficulty: String,
    ai_color: Option<String>,
    now_ms: i64,
) -> ChessState {
    ChessState {
        white_user_id,
        black_user_id,
        ai_enabled,
        ai_difficulty,
        ai_color,
        updated_ts_ms: now_ms,
        ..ChessState::default()
    }
}

fn fresh_connect_four_state(
    red_user_id: Option<String>,
    yellow_user_id: Option<String>,
    ai_enabled: bool,
    ai_difficulty: String,
    ai_color: Option<String>,
    now_ms: i64,
) -> ConnectFourState {
    ConnectFourState {
        red_user_id,
        yellow_user_id,
        ai_enabled,
        ai_difficulty,
        ai_color,
        updated_ts_ms: now_ms,
        ..ConnectFourState::default()
    }
}

fn fresh_battleship_state(
    blue_user_id: Option<String>,
    red_user_id: Option<String>,
    ai_enabled: bool,
    ai_difficulty: String,
    ai_color: Option<String>,
    now_ms: i64,
) -> BattleshipState {
    BattleshipState {
        blue_user_id,
        red_user_id,
        ai_enabled,
        ai_difficulty,
        ai_color,
        updated_ts_ms: now_ms,
        ..BattleshipState::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectFourColor {
    Red,
    Yellow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BattleshipColor {
    Blue,
    Red,
}

fn connect_four_color_name(color: ConnectFourColor) -> &'static str {
    match color {
        ConnectFourColor::Red => "red",
        ConnectFourColor::Yellow => "yellow",
    }
}

fn parse_connect_four_color_name(raw: &str) -> Option<ConnectFourColor> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "red" => Some(ConnectFourColor::Red),
        "yellow" | "blue" => Some(ConnectFourColor::Yellow),
        _ => None,
    }
}

fn opposite_connect_four_color(color: ConnectFourColor) -> ConnectFourColor {
    match color {
        ConnectFourColor::Red => ConnectFourColor::Yellow,
        ConnectFourColor::Yellow => ConnectFourColor::Red,
    }
}

fn connect_four_color_to_token(color: ConnectFourColor) -> u8 {
    match color {
        ConnectFourColor::Red => 1,
        ConnectFourColor::Yellow => 2,
    }
}

fn connect_four_turn_to_color(turn: &str) -> ConnectFourColor {
    if turn == "yellow" {
        ConnectFourColor::Yellow
    } else {
        ConnectFourColor::Red
    }
}

fn battleship_color_name(color: BattleshipColor) -> &'static str {
    match color {
        BattleshipColor::Blue => "blue",
        BattleshipColor::Red => "red",
    }
}

fn parse_battleship_color_name(raw: &str) -> Option<BattleshipColor> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "blue" => Some(BattleshipColor::Blue),
        "red" => Some(BattleshipColor::Red),
        _ => None,
    }
}

fn opposite_battleship_color(color: BattleshipColor) -> BattleshipColor {
    match color {
        BattleshipColor::Blue => BattleshipColor::Red,
        BattleshipColor::Red => BattleshipColor::Blue,
    }
}

fn connect_four_drop_in_place(
    board: &mut [u8; CONNECT_FOUR_CELLS],
    column: usize,
    token: u8,
) -> Option<usize> {
    for row in (0..CONNECT_FOUR_ROWS).rev() {
        let idx = row * CONNECT_FOUR_COLS + column;
        if board[idx] == 0 {
            board[idx] = token;
            return Some(row);
        }
    }
    None
}

fn connect_four_preview_drop(
    board: &[u8; CONNECT_FOUR_CELLS],
    column: usize,
    token: u8,
) -> Option<([u8; CONNECT_FOUR_CELLS], usize)> {
    let mut next = *board;
    let row = connect_four_drop_in_place(&mut next, column, token)?;
    Some((next, row))
}

fn connect_four_valid_columns(board: &[u8; CONNECT_FOUR_CELLS]) -> Vec<usize> {
    (0..CONNECT_FOUR_COLS)
        .filter(|column| board[*column] == 0)
        .collect()
}

fn select_connect_four_ai_column(
    board: &[u8; CONNECT_FOUR_CELLS],
    ai_color: ConnectFourColor,
    difficulty: &str,
) -> Option<usize> {
    let valid_columns = connect_four_valid_columns(board);
    if valid_columns.is_empty() {
        return None;
    }

    if difficulty == "easy" {
        let mut rng = StdRng::seed_from_u64(
            chrono::Utc::now().timestamp_millis().unsigned_abs() ^ 0xC0DEC4u64,
        );
        return valid_columns
            .get(rng.gen_range(0..valid_columns.len()))
            .copied();
    }

    let ai_token = connect_four_color_to_token(ai_color);
    let opponent_color = opposite_connect_four_color(ai_color);
    let opponent_token = connect_four_color_to_token(opponent_color);

    for column in &valid_columns {
        if let Some((preview, row)) = connect_four_preview_drop(board, *column, ai_token) {
            if connect_four_has_winning_line(&preview, row, *column, ai_token) {
                return Some(*column);
            }
        }
    }

    for column in &valid_columns {
        if let Some((preview, row)) = connect_four_preview_drop(board, *column, opponent_token) {
            if connect_four_has_winning_line(&preview, row, *column, opponent_token) {
                return Some(*column);
            }
        }
    }

    if difficulty == "medium" {
        return valid_columns
            .into_iter()
            .min_by_key(|column| (*column as i32 - 3).abs());
    }

    valid_columns.into_iter().max_by_key(|column| {
        let Some((preview, _row)) = connect_four_preview_drop(board, *column, ai_token) else {
            return i32::MIN;
        };
        let center_bonus = 6 - ((*column as i32 - 3).abs() * 2);
        center_bonus + evaluate_connect_four_board(&preview, ai_token)
    })
}

fn evaluate_connect_four_board(board: &[u8; CONNECT_FOUR_CELLS], ai_token: u8) -> i32 {
    let opponent_token = if ai_token == 1 { 2 } else { 1 };
    let mut score = 0_i32;

    for row in 0..CONNECT_FOUR_ROWS {
        let center_idx = row * CONNECT_FOUR_COLS + 3;
        if board[center_idx] == ai_token {
            score += 6;
        } else if board[center_idx] == opponent_token {
            score -= 6;
        }
    }

    for row in 0..CONNECT_FOUR_ROWS {
        for col in 0..CONNECT_FOUR_COLS {
            if col + 3 < CONNECT_FOUR_COLS {
                score += evaluate_connect_four_window(board, row, col, 1, 0, ai_token);
            }
            if row + 3 < CONNECT_FOUR_ROWS {
                score += evaluate_connect_four_window(board, row, col, 0, 1, ai_token);
            }
            if row + 3 < CONNECT_FOUR_ROWS && col + 3 < CONNECT_FOUR_COLS {
                score += evaluate_connect_four_window(board, row, col, 1, 1, ai_token);
            }
            if row + 3 < CONNECT_FOUR_ROWS && col >= 3 {
                score += evaluate_connect_four_window(board, row, col, -1, 1, ai_token);
            }
        }
    }

    score
}

fn evaluate_connect_four_window(
    board: &[u8; CONNECT_FOUR_CELLS],
    start_row: usize,
    start_col: usize,
    dx: isize,
    dy: isize,
    ai_token: u8,
) -> i32 {
    let opponent_token = if ai_token == 1 { 2 } else { 1 };
    let mut ai_count = 0;
    let mut opponent_count = 0;
    let mut empty_count = 0;

    for step in 0..4 {
        let row = (start_row as isize + dy * step) as usize;
        let col = (start_col as isize + dx * step) as usize;
        let value = board[row * CONNECT_FOUR_COLS + col];
        if value == ai_token {
            ai_count += 1;
        } else if value == opponent_token {
            opponent_count += 1;
        } else {
            empty_count += 1;
        }
    }

    if ai_count > 0 && opponent_count > 0 {
        return 0;
    }

    match (ai_count, opponent_count, empty_count) {
        (4, 0, 0) => 100_000,
        (3, 0, 1) => 120,
        (2, 0, 2) => 18,
        (1, 0, 3) => 2,
        (0, 4, 0) => -100_000,
        (0, 3, 1) => -140,
        (0, 2, 2) => -22,
        (0, 1, 3) => -2,
        _ => 0,
    }
}

fn select_ai_move(
    board: &Board,
    legal_moves: &[ChessMove],
    ai_color: Color,
    difficulty: &str,
) -> ChessMove {
    match difficulty.trim().to_ascii_lowercase().as_str() {
        "easy" => {
            let seed = chrono::Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
                .unsigned_abs() as usize;
            legal_moves[seed % legal_moves.len()]
        }
        "hard" => {
            let mut best_move = legal_moves[0];
            let mut best_score = i32::MIN;
            for mv in legal_moves {
                let next = board.make_move_new(*mv);
                let score = minimax_score(&next, 2, ai_color, i32::MIN / 2, i32::MAX / 2);
                if score > best_score {
                    best_score = score;
                    best_move = *mv;
                }
            }
            best_move
        }
        _ => {
            // Medium: evaluate immediate material outcome and prefer strongest one-ply move.
            let mut best_move = legal_moves[0];
            let mut best_score = i32::MIN;
            for mv in legal_moves {
                let next = board.make_move_new(*mv);
                let score = evaluate_board_material(&next, ai_color);
                if score > best_score {
                    best_score = score;
                    best_move = *mv;
                }
            }
            best_move
        }
    }
}

fn minimax_score(board: &Board, depth: u8, ai_color: Color, mut alpha: i32, mut beta: i32) -> i32 {
    if depth == 0 || board.status() != BoardStatus::Ongoing {
        return evaluate_board_material(board, ai_color);
    }

    let moves: Vec<ChessMove> = MoveGen::new_legal(board).collect();
    if moves.is_empty() {
        return evaluate_board_material(board, ai_color);
    }

    let maximizing = board.side_to_move() == ai_color;
    if maximizing {
        let mut best = i32::MIN;
        for mv in moves {
            let next = board.make_move_new(mv);
            let score = minimax_score(&next, depth.saturating_sub(1), ai_color, alpha, beta);
            best = best.max(score);
            alpha = alpha.max(best);
            if beta <= alpha {
                break;
            }
        }
        best
    } else {
        let mut best = i32::MAX;
        for mv in moves {
            let next = board.make_move_new(mv);
            let score = minimax_score(&next, depth.saturating_sub(1), ai_color, alpha, beta);
            best = best.min(score);
            beta = beta.min(best);
            if beta <= alpha {
                break;
            }
        }
        best
    }
}

fn evaluate_board_material(board: &Board, ai_color: Color) -> i32 {
    match board.status() {
        BoardStatus::Checkmate => {
            return if board.side_to_move() == ai_color {
                -100_000
            } else {
                100_000
            };
        }
        BoardStatus::Stalemate => return 0,
        BoardStatus::Ongoing => {}
    }

    let opponent = opposite_color(ai_color);
    let mut score = 0_i32;
    for piece in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
    ] {
        let value = piece_value(piece);
        let ai_count = (board.pieces(piece) & board.color_combined(ai_color)).popcnt() as i32;
        let opp_count = (board.pieces(piece) & board.color_combined(opponent)).popcnt() as i32;
        score += (ai_count - opp_count) * value;
    }
    score
}

fn piece_value(piece: Piece) -> i32 {
    match piece {
        Piece::Pawn => 100,
        Piece::Knight => 320,
        Piece::Bishop => 330,
        Piece::Rook => 500,
        Piece::Queen => 900,
        Piece::King => 0,
    }
}

fn clear_chess_reset_requests(chess: &mut ChessState) {
    chess.reset_requested_white = false;
    chess.reset_requested_black = false;
}

fn connect_four_has_winning_line(
    board: &[u8; CONNECT_FOUR_CELLS],
    row: usize,
    col: usize,
    token: u8,
) -> bool {
    const DIRECTIONS: [(isize, isize); 4] = [(1, 0), (0, 1), (1, 1), (1, -1)];
    for (dx, dy) in DIRECTIONS {
        let mut count = 1;
        count += connect_four_count_direction(board, row, col, token, dx, dy);
        count += connect_four_count_direction(board, row, col, token, -dx, -dy);
        if count >= 4 {
            return true;
        }
    }
    false
}

fn connect_four_count_direction(
    board: &[u8; CONNECT_FOUR_CELLS],
    row: usize,
    col: usize,
    token: u8,
    dx: isize,
    dy: isize,
) -> i32 {
    let mut steps = 0_i32;
    let mut x = col as isize + dx;
    let mut y = row as isize + dy;

    while x >= 0 && x < CONNECT_FOUR_COLS as isize && y >= 0 && y < CONNECT_FOUR_ROWS as isize {
        let idx = y as usize * CONNECT_FOUR_COLS + x as usize;
        if board[idx] != token {
            break;
        }
        steps += 1;
        x += dx;
        y += dy;
    }
    steps
}

fn battleship_color_for_user(state: &BattleshipState, user_id: &str) -> Option<&'static str> {
    if state.blue_user_id.as_deref() == Some(user_id) {
        Some("blue")
    } else if state.red_user_id.as_deref() == Some(user_id) {
        Some("red")
    } else {
        None
    }
}

fn clear_battleship_reset_requests(state: &mut BattleshipState) {
    state.reset_requested_blue = false;
    state.reset_requested_red = false;
}

fn count_ship_cells(ships: &[u8; BATTLESHIP_BOARD_CELLS]) -> u16 {
    ships.iter().filter(|cell| **cell != 0).count() as u16
}

pub(crate) fn placed_battleship_ship_ids(ships: &[u8; BATTLESHIP_BOARD_CELLS]) -> Vec<u8> {
    let mut ids: Vec<u8> = (1..=(BATTLESHIP_SHIP_SIZES.len() as u8))
        .filter(|ship_id| ships.iter().any(|cell| *cell == *ship_id))
        .collect();
    ids.sort_unstable();
    ids
}

fn battleship_ship_size(ship_id: u8) -> Option<usize> {
    BATTLESHIP_SHIP_SIZES
        .get(ship_id.saturating_sub(1) as usize)
        .map(|size| *size as usize)
}

fn generate_battleship_ships(seed: u64) -> Result<[u8; BATTLESHIP_BOARD_CELLS], String> {
    let mut rng = StdRng::seed_from_u64(seed);

    for _ in 0..256 {
        let mut board = [0_u8; BATTLESHIP_BOARD_CELLS];
        let mut placed_all = true;
        for (ship_offset, size) in BATTLESHIP_SHIP_SIZES.iter().enumerate() {
            let ship_id = (ship_offset as u8) + 1;
            let mut placed_ship = false;
            for _ in 0..512 {
                let horizontal = rng.gen_bool(0.5);
                let max_x = if horizontal {
                    BATTLESHIP_BOARD_SIZE - (*size as usize)
                } else {
                    BATTLESHIP_BOARD_SIZE - 1
                };
                let max_y = if horizontal {
                    BATTLESHIP_BOARD_SIZE - 1
                } else {
                    BATTLESHIP_BOARD_SIZE - (*size as usize)
                };
                let x = rng.gen_range(0..=max_x);
                let y = rng.gen_range(0..=max_y);

                let mut valid = true;
                for i in 0..(*size as usize) {
                    let px = if horizontal { x + i } else { x };
                    let py = if horizontal { y } else { y + i };
                    let idx = py * BATTLESHIP_BOARD_SIZE + px;
                    if board[idx] != 0 {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    continue;
                }

                for i in 0..(*size as usize) {
                    let px = if horizontal { x + i } else { x };
                    let py = if horizontal { y } else { y + i };
                    let idx = py * BATTLESHIP_BOARD_SIZE + px;
                    board[idx] = ship_id;
                }
                placed_ship = true;
                break;
            }

            if !placed_ship {
                placed_all = false;
                break;
            }
        }

        if placed_all {
            return Ok(board);
        }
    }

    Err("failed to generate a valid battleship board layout".to_string())
}

fn is_battleship_ship_sunk(
    ships: &[u8; BATTLESHIP_BOARD_CELLS],
    shots: &[u8; BATTLESHIP_BOARD_CELLS],
    ship_id: u8,
) -> bool {
    if ship_id == 0 {
        return false;
    }
    for (idx, cell_ship_id) in ships.iter().enumerate() {
        if *cell_ship_id == ship_id && shots[idx] != 2 {
            return false;
        }
    }
    true
}

fn select_battleship_ai_target(
    shots: &[u8; BATTLESHIP_BOARD_CELLS],
    ai_color: BattleshipColor,
    difficulty: &str,
) -> Option<(usize, usize)> {
    let untargeted = battleship_untargeted_cells(shots);
    if untargeted.is_empty() {
        return None;
    }

    if difficulty == "easy" {
        let mut rng = StdRng::seed_from_u64(
            chrono::Utc::now().timestamp_millis().unsigned_abs()
                ^ match ai_color {
                    BattleshipColor::Blue => 0xB100u64,
                    BattleshipColor::Red => 0xB200u64,
                },
        );
        return untargeted.get(rng.gen_range(0..untargeted.len())).copied();
    }

    let adjacent_targets = battleship_adjacent_targets_from_hits(shots);
    if !adjacent_targets.is_empty() {
        if difficulty == "hard" {
            return adjacent_targets
                .into_iter()
                .max_by_key(|(x, y)| battleship_target_score(shots, *x, *y))
                .or_else(|| untargeted.first().copied());
        }
        return adjacent_targets.first().copied();
    }

    if difficulty == "hard" {
        let parity_targets: Vec<(usize, usize)> = untargeted
            .iter()
            .copied()
            .filter(|(x, y)| (x + y) % 2 == 0)
            .collect();
        if !parity_targets.is_empty() {
            let mut rng = StdRng::seed_from_u64(
                chrono::Utc::now().timestamp_millis().unsigned_abs()
                    ^ match ai_color {
                        BattleshipColor::Blue => 0xB300u64,
                        BattleshipColor::Red => 0xB400u64,
                    },
            );
            return parity_targets
                .get(rng.gen_range(0..parity_targets.len()))
                .copied();
        }
    }

    untargeted.first().copied()
}

fn battleship_untargeted_cells(shots: &[u8; BATTLESHIP_BOARD_CELLS]) -> Vec<(usize, usize)> {
    let mut cells = Vec::new();
    for y in 0..BATTLESHIP_BOARD_SIZE {
        for x in 0..BATTLESHIP_BOARD_SIZE {
            let idx = y * BATTLESHIP_BOARD_SIZE + x;
            if shots[idx] == 0 {
                cells.push((x, y));
            }
        }
    }
    cells
}

fn battleship_adjacent_targets_from_hits(
    shots: &[u8; BATTLESHIP_BOARD_CELLS],
) -> Vec<(usize, usize)> {
    let mut results = Vec::new();
    let mut seen = [false; BATTLESHIP_BOARD_CELLS];

    for y in 0..BATTLESHIP_BOARD_SIZE {
        for x in 0..BATTLESHIP_BOARD_SIZE {
            let idx = y * BATTLESHIP_BOARD_SIZE + x;
            if shots[idx] != 2 {
                continue;
            }

            for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
                let next_x = x as i32 + dx;
                let next_y = y as i32 + dy;
                if next_x < 0
                    || next_x >= BATTLESHIP_BOARD_SIZE as i32
                    || next_y < 0
                    || next_y >= BATTLESHIP_BOARD_SIZE as i32
                {
                    continue;
                }
                let next_x = next_x as usize;
                let next_y = next_y as usize;
                let next_idx = next_y * BATTLESHIP_BOARD_SIZE + next_x;
                if shots[next_idx] != 0 || seen[next_idx] {
                    continue;
                }
                seen[next_idx] = true;
                results.push((next_x, next_y));
            }
        }
    }

    results
}

fn battleship_target_score(shots: &[u8; BATTLESHIP_BOARD_CELLS], x: usize, y: usize) -> i32 {
    let mut score = 0_i32;
    for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
        let next_x = x as i32 + dx;
        let next_y = y as i32 + dy;
        if next_x < 0
            || next_x >= BATTLESHIP_BOARD_SIZE as i32
            || next_y < 0
            || next_y >= BATTLESHIP_BOARD_SIZE as i32
        {
            continue;
        }
        let next_idx = next_y as usize * BATTLESHIP_BOARD_SIZE + next_x as usize;
        if shots[next_idx] == 2 {
            score += 10;
        } else if shots[next_idx] == 0 {
            score += 1;
        }
    }

    // Bias slightly toward the center when otherwise equal.
    let center = (BATTLESHIP_BOARD_SIZE as i32 - 1) / 2;
    score - ((x as i32 - center).abs() + (y as i32 - center).abs())
}

fn validate_chess_seat_change(
    seat_name: &str,
    current: Option<&str>,
    next: Option<&str>,
    actor_user_id: &str,
) -> Result<(), String> {
    if current == next {
        return Ok(());
    }

    let Some(current_player_id) = current else {
        return Ok(());
    };

    if current_player_id != actor_user_id {
        return Err(format!(
            "only the current {seat_name} player can leave that color"
        ));
    }

    if next.is_some() {
        return Err(format!(
            "{seat_name} seat is occupied; current player must clear it before reassignment"
        ));
    }

    Ok(())
}

fn chess_status(board: &Board) -> (String, Option<String>) {
    match board.status() {
        BoardStatus::Ongoing => ("active".to_string(), None),
        BoardStatus::Stalemate => ("stalemate".to_string(), None),
        BoardStatus::Checkmate => {
            let winner = match board.side_to_move() {
                Color::White => "black",
                Color::Black => "white",
            };
            ("checkmate".to_string(), Some(winner.to_string()))
        }
    }
}

fn parse_square(raw: &str) -> Result<Square, String> {
    Square::from_str(raw.trim().to_ascii_lowercase().as_str())
        .map_err(|_| format!("invalid square '{raw}'"))
}

fn parse_promotion_piece(raw: Option<&str>) -> Result<Option<Piece>, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let piece = match raw.to_ascii_lowercase().as_str() {
        "q" | "queen" => Piece::Queen,
        "r" | "rook" => Piece::Rook,
        "b" | "bishop" => Piece::Bishop,
        "n" | "knight" => Piece::Knight,
        _ => {
            return Err(format!(
                "invalid promotion piece '{raw}', expected one of: q, r, b, n"
            ));
        }
    };
    Ok(Some(piece))
}

fn piece_to_promotion(piece: Piece) -> String {
    match piece {
        Piece::Queen => "q",
        Piece::Rook => "r",
        Piece::Bishop => "b",
        Piece::Knight => "n",
        _ => "q",
    }
    .to_string()
}

fn square_to_string(square: Square) -> String {
    square.to_string().to_ascii_lowercase()
}

#[derive(Default)]
pub struct WatchPartyManager {
    rooms: RwLock<HashMap<String, Arc<RoomRuntime>>>,
}

impl WatchPartyManager {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get_or_create_runtime(
        &self,
        room_id: &str,
        item_id: &str,
        room_mode: &str,
        audio_source: Option<&str>,
        audio_library_id: Option<&str>,
        audio_track_ids: Option<Vec<String>>,
        youtube_video_id: Option<String>,
        web_url: Option<String>,
        create_state: Option<CreateState>,
    ) -> Arc<RoomRuntime> {
        let eviction_candidates = {
            let rooms = self.rooms.read().await;
            if let Some(existing) = rooms.get(room_id) {
                existing.touch_activity().await;
                return existing.clone();
            }

            if rooms.len() >= MAX_ACTIVE_ROOMS {
                rooms
                    .iter()
                    .filter(|(key, _)| key.as_str() != room_id)
                    .map(|(key, room)| (key.clone(), room.clone()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };

        let mut evict_key: Option<String> = None;
        let mut evict_activity_ts = i64::MAX;
        for (key, room) in &eviction_candidates {
            let activity_ts = room.last_activity_ts_ms().await;
            if activity_ts < evict_activity_ts {
                evict_activity_ts = activity_ts;
                evict_key = Some(key.clone());
            }
        }

        let mut rooms = self.rooms.write().await;
        if rooms.len() >= MAX_ACTIVE_ROOMS {
            if let Some(key) = evict_key {
                rooms.remove(&key);
            }
        }

        rooms
            .entry(room_id.to_string())
            .or_insert_with(|| {
                let runtime = if room_mode == "audio" {
                    RoomRuntime::new_audio(
                        room_id.to_string(),
                        item_id.to_string(),
                        audio_source.unwrap_or("online").to_string(),
                        audio_library_id.map(str::to_string),
                        audio_track_ids.unwrap_or_default(),
                    )
                } else if room_mode == "youtube" {
                    RoomRuntime::new_youtube(room_id.to_string(), youtube_video_id)
                } else if room_mode == "web" {
                    RoomRuntime::new_web(
                        room_id.to_string(),
                        web_url.unwrap_or_else(|| "https://www.mozilla.org/".to_string()),
                    )
                } else if room_mode == "create" {
                    RoomRuntime::new_create(room_id.to_string(), create_state.unwrap_or_default())
                } else if room_mode == "play" {
                    RoomRuntime::new_play(room_id.to_string())
                } else {
                    RoomRuntime::new_video(room_id.to_string(), item_id.to_string())
                };
                Arc::new(runtime)
            })
            .clone()
    }

    pub async fn get_runtime(&self, room_id: &str) -> Option<Arc<RoomRuntime>> {
        let rooms = self.rooms.read().await;
        rooms.get(room_id).cloned()
    }

    pub async fn remove_runtime(&self, room_id: &str) {
        let mut rooms = self.rooms.write().await;
        rooms.remove(room_id);
    }

    pub async fn cleanup_empty_lobbies(&self, db: &rustfin_db::DbPool, room_audio_root: &Path) {
        let cutoff_ts = chrono::Utc::now().timestamp() - EMPTY_ROOM_TTL_SECONDS;
        let candidate_room_ids =
            match rustfin_db::repo::watch_party::list_purgeable_room_ids_updated_before(
                db, cutoff_ts,
            )
            .await
            {
                Ok(ids) => ids,
                Err(err) => {
                    tracing::warn!(error = %err, "watch party empty-lobby cleanup query failed");
                    return;
                }
            };

        for room_id in candidate_room_ids {
            let runtime = self.get_runtime(&room_id).await;
            if let Some(runtime) = runtime.as_ref() {
                let connected_count = runtime.connected_user_ids.read().await.len();
                if connected_count > 0 {
                    continue;
                }
            }

            match rustfin_db::repo::watch_party::delete_room(db, &room_id).await {
                Ok(true) => {
                    if let Some(runtime) = runtime {
                        let _ = runtime.tx.send(ServerMessage::RoomEnded);
                    }
                    self.remove_runtime(&room_id).await;
                    if let Err(err) = remove_room_audio_files(room_audio_root, &room_id).await {
                        tracing::warn!(
                            room_id = %room_id,
                            error = %err,
                            "failed to purge watch-party room audio files"
                        );
                    }
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!(
                        room_id = %room_id,
                        error = %err,
                        "watch party cleanup failed to purge room"
                    );
                }
            }
        }
    }
}

async fn remove_room_audio_files(room_audio_root: &Path, room_id: &str) -> std::io::Result<()> {
    let room_dir = room_audio_root.join(room_id);
    match tokio::fs::metadata(&room_dir).await {
        Ok(meta) => {
            if meta.is_dir() {
                tokio::fs::remove_dir_all(room_dir).await?;
            } else {
                tokio::fs::remove_file(room_dir).await?;
            }
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_audio_runtime(track_ids: Vec<String>) -> RoomRuntime {
        RoomRuntime::new_audio(
            "room-test".to_string(),
            "item-test".to_string(),
            "library".to_string(),
            Some("library-1".to_string()),
            track_ids,
        )
    }

    #[tokio::test]
    async fn set_audio_shuffle_preserves_playback_progress() {
        let runtime = build_audio_runtime(vec!["track-a".to_string()]);
        let queue_lock = runtime.audio_queue.as_ref().expect("audio queue");
        let base_now_ms = chrono::Utc::now().timestamp_millis();

        {
            let mut queue = queue_lock.write().await;
            queue.playing = true;
            queue.position_ms = 12_000;
            queue.updated_ts_ms = base_now_ms - 3_000;
        }

        let updated = runtime
            .set_audio_shuffle_enabled(true)
            .await
            .expect("shuffle update");

        assert!(updated.shuffle_enabled);
        assert!(
            updated.position_ms >= 15_000,
            "expected projected position >= 15000ms, got {}",
            updated.position_ms
        );
    }

    #[tokio::test]
    async fn set_audio_repeat_preserves_playback_progress() {
        let runtime = build_audio_runtime(vec!["track-a".to_string()]);
        let queue_lock = runtime.audio_queue.as_ref().expect("audio queue");
        let base_now_ms = chrono::Utc::now().timestamp_millis();

        {
            let mut queue = queue_lock.write().await;
            queue.playing = true;
            queue.position_ms = 7_500;
            queue.updated_ts_ms = base_now_ms - 2_000;
        }

        let updated = runtime
            .set_audio_repeat_mode(AudioRepeatMode::Queue)
            .await
            .expect("repeat update");

        assert_eq!(updated.repeat_mode, AudioRepeatMode::Queue);
        assert!(
            updated.position_ms >= 9_500,
            "expected projected position >= 9500ms, got {}",
            updated.position_ms
        );
    }

    #[test]
    fn connect_four_detects_winning_line() {
        let mut board = [0_u8; CONNECT_FOUR_CELLS];
        board[5 * CONNECT_FOUR_COLS + 1] = 1;
        board[4 * CONNECT_FOUR_COLS + 1] = 1;
        board[3 * CONNECT_FOUR_COLS + 1] = 1;
        board[2 * CONNECT_FOUR_COLS + 1] = 1;

        assert!(connect_four_has_winning_line(&board, 2, 1, 1));
    }

    #[tokio::test]
    async fn connect_four_requires_assigned_player_for_current_turn() {
        let runtime = RoomRuntime::new_play("room-connect-four".to_string());
        runtime
            .set_play_game("connect_four".to_string())
            .await
            .expect("activate connect four");
        runtime
            .set_connect_four_players("admin", Some("user-red".to_string()), None)
            .await
            .expect("assign red seat");

        let err = runtime
            .apply_connect_four_drop("user-red", 0)
            .await
            .expect("first red move should succeed");
        assert!(err.is_some());

        let second_err = runtime
            .apply_connect_four_drop("user-red", 1)
            .await
            .expect_err("blue turn without assigned blue player should be rejected");
        assert_eq!(second_err, "no blue player is assigned for this turn");
    }

    #[tokio::test]
    async fn connect_four_ai_configuration_assigns_human_and_ai_sides() {
        let runtime = RoomRuntime::new_play("room-connect-four".to_string());
        runtime
            .set_play_game("connect_four".to_string())
            .await
            .expect("activate connect four");

        let snapshot = runtime
            .configure_connect_four_ai(
                "user-blue",
                true,
                Some("hard".to_string()),
                Some("blue".to_string()),
            )
            .await
            .expect("configure connect four ai")
            .expect("play state snapshot");

        assert!(snapshot.connect_four.ai_enabled);
        assert_eq!(snapshot.connect_four.ai_difficulty, "hard");
        assert_eq!(snapshot.connect_four.ai_color.as_deref(), Some("red"));
        assert_eq!(snapshot.connect_four.red_user_id, None);
        assert_eq!(
            snapshot.connect_four.yellow_user_id.as_deref(),
            Some("user-blue")
        );
    }

    #[tokio::test]
    async fn connect_four_ai_can_take_opening_move() {
        let runtime = RoomRuntime::new_play("room-connect-four".to_string());
        runtime
            .set_play_game("connect_four".to_string())
            .await
            .expect("activate connect four");
        runtime
            .configure_connect_four_ai(
                "user-blue",
                true,
                Some("medium".to_string()),
                Some("blue".to_string()),
            )
            .await
            .expect("configure connect four ai");

        let moved = runtime
            .apply_connect_four_ai_move_if_needed()
            .await
            .expect("apply connect four ai move");
        assert!(moved);

        let play_state = runtime.snapshot_play_state().await.expect("play state");
        let red_count = play_state
            .connect_four
            .board
            .iter()
            .filter(|cell| **cell == 1)
            .count();
        let blue_count = play_state
            .connect_four
            .board
            .iter()
            .filter(|cell| **cell == 2)
            .count();

        assert_eq!(red_count, 1, "expected exactly one AI disc on the board");
        assert_eq!(blue_count, 0, "human should not have moved yet");
        assert_eq!(play_state.connect_four.turn, "yellow");
        assert_eq!(play_state.connect_four.status, "active");
        assert_eq!(play_state.connect_four.ai_color.as_deref(), Some("red"));
        assert!(play_state.connect_four.last_move_col.is_some());
        assert!(play_state.connect_four.last_move_row.is_some());
    }

    #[tokio::test]
    async fn battleship_ai_configuration_assigns_human_and_ai_sides() {
        let runtime = RoomRuntime::new_play("room-battleship".to_string());
        runtime
            .set_play_game("battleship".to_string())
            .await
            .expect("activate battleship");

        let snapshot = runtime
            .configure_battleship_ai(
                "user-blue",
                true,
                Some("hard".to_string()),
                Some("blue".to_string()),
            )
            .await
            .expect("configure battleship ai")
            .expect("play state snapshot");

        assert!(snapshot.battleship.ai_enabled);
        assert_eq!(snapshot.battleship.ai_difficulty, "hard");
        assert_eq!(snapshot.battleship.ai_color.as_deref(), Some("red"));
        assert_eq!(
            snapshot.battleship.blue_user_id.as_deref(),
            Some("user-blue")
        );
        assert_eq!(snapshot.battleship.red_user_id, None);
    }

    #[tokio::test]
    async fn battleship_manual_ship_placement_updates_board() {
        let runtime = RoomRuntime::new_play("room-battleship-manual".to_string());
        runtime
            .set_play_game("battleship".to_string())
            .await
            .expect("activate battleship");
        runtime
            .set_battleship_players(
                "admin",
                Some("user-blue".to_string()),
                Some("user-red".to_string()),
            )
            .await
            .expect("assign players");

        let placed = runtime
            .battleship_place_ship("user-blue", 1, 0, 0, "horizontal")
            .await
            .expect("place carrier")
            .expect("play state");

        assert_eq!(placed.battleship.phase, "setup");
        assert!(!placed.battleship.blue_ready);
        assert_eq!(placed.battleship.remaining_ship_cells_blue, 5);
        assert_eq!(count_ship_cells(&placed.battleship.blue_ships), 5);
        assert_eq!(placed.battleship.blue_ships[0], 1);
        assert_eq!(placed.battleship.blue_ships[4], 1);
        assert_eq!(placed.battleship.blue_ships[5], 0);
    }

    #[tokio::test]
    async fn battleship_ai_can_prepare_and_take_turn() {
        let runtime = RoomRuntime::new_play("room-battleship".to_string());
        runtime
            .set_play_game("battleship".to_string())
            .await
            .expect("activate battleship");
        runtime
            .configure_battleship_ai(
                "user-blue",
                true,
                Some("medium".to_string()),
                Some("blue".to_string()),
            )
            .await
            .expect("configure battleship ai");

        let prepared = runtime
            .apply_battleship_ai_action_if_needed()
            .await
            .expect("apply battleship ai setup");
        assert!(prepared);

        let after_ai_setup = runtime.snapshot_play_state().await.expect("play state");
        assert_eq!(
            count_ship_cells(&after_ai_setup.battleship.red_ships),
            BATTLESHIP_TOTAL_SHIP_CELLS
        );
        assert!(after_ai_setup.battleship.red_ready);
        assert!(!after_ai_setup.battleship.blue_ready);
        assert_eq!(after_ai_setup.battleship.phase, "setup");

        runtime
            .battleship_auto_place("user-blue")
            .await
            .expect("human auto place");
        runtime
            .battleship_set_ready("user-blue", true)
            .await
            .expect("human ready");

        let active_state = runtime
            .snapshot_play_state()
            .await
            .expect("active play state");
        assert_eq!(active_state.battleship.phase, "active");
        assert_eq!(active_state.battleship.status, "active");

        if active_state.battleship.turn_color == "blue" {
            runtime
                .battleship_fire("user-blue", 0, 0)
                .await
                .expect("human opening shot");
        }

        let moved = runtime
            .apply_battleship_ai_action_if_needed()
            .await
            .expect("apply battleship ai move");
        assert!(moved);

        let final_state = runtime
            .snapshot_play_state()
            .await
            .expect("final play state");
        assert_eq!(final_state.battleship.ai_color.as_deref(), Some("red"));
        assert_eq!(
            final_state
                .battleship
                .last_shot
                .as_ref()
                .map(|shot| shot.by_color.as_str()),
            Some("red")
        );
        assert!(
            final_state
                .battleship
                .blue_shots
                .iter()
                .any(|cell| *cell != 0),
            "expected AI to target at least one blue cell"
        );
    }

    #[test]
    fn battleship_layout_places_expected_ships() {
        let ships = generate_battleship_ships(42).expect("layout");
        assert_eq!(count_ship_cells(&ships), BATTLESHIP_TOTAL_SHIP_CELLS);

        let mut lengths: Vec<usize> = (1_u8..=5)
            .map(|ship_id| ships.iter().filter(|cell| **cell == ship_id).count())
            .collect();
        lengths.sort_unstable();

        let mut expected: Vec<usize> = BATTLESHIP_SHIP_SIZES
            .iter()
            .map(|size| *size as usize)
            .collect();
        expected.sort_unstable();
        assert_eq!(lengths, expected);
    }
}
