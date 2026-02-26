use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use chess::{Board, BoardStatus, ChessMove, Color, MoveGen, Piece, Square};
use tokio::sync::{RwLock, broadcast};

use super::protocol::{AudioRepeatMode, CreateCanvasStroke, ServerMessage, YouTubeSearchEntry};

const MAX_ACTIVE_ROOMS: usize = 512;
const EMPTY_ROOM_TTL_SECONDS: i64 = 5 * 60;

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
            updated_ts_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayState {
    pub active_game: String,
    pub chess: ChessState,
    pub updated_ts_ms: i64,
}

impl Default for PlayState {
    fn default() -> Self {
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self {
            active_game: "chess".to_string(),
            chess: ChessState::default(),
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
        if normalized != "chess" {
            return Err("only chess is currently supported in play-together rooms".to_string());
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = state.write().await;
        guard.active_game = normalized;
        guard.updated_ts_ms = now_ms;
        guard.chess.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
    }

    pub async fn set_chess_players(
        &self,
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
        guard.chess.white_user_id = white;
        guard.chess.black_user_id = black;
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
        let mut next_chess = ChessState::default();
        next_chess.white_user_id = white_user_id;
        next_chess.black_user_id = black_user_id;
        next_chess.updated_ts_ms = now_ms;
        guard.active_game = "chess".to_string();
        guard.chess = next_chess;
        guard.updated_ts_ms = now_ms;
        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
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
        guard.chess.updated_ts_ms = now_ms;
        guard.updated_ts_ms = now_ms;

        let snapshot = guard.clone();
        drop(guard);
        self.touch_activity().await;
        Ok(Some(snapshot))
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
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "white",
        Color::Black => "black",
    }
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
        {
            let rooms = self.rooms.read().await;
            if let Some(existing) = rooms.get(room_id) {
                existing.touch_activity().await;
                return existing.clone();
            }
        }

        let mut rooms = self.rooms.write().await;
        if rooms.len() >= MAX_ACTIVE_ROOMS {
            let evict_key = rooms
                .iter()
                .find_map(|(key, room)| {
                    (key.as_str() != room_id).then(|| (key.clone(), room.clone()))
                })
                .map(|(key, _)| key);
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

    pub async fn cleanup_empty_lobbies(&self, db: &sqlx::SqlitePool, room_audio_root: &Path) {
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
}
