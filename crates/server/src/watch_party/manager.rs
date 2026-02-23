use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};

use super::protocol::ServerMessage;

const MAX_ACTIVE_ROOMS: usize = 512;

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
    pub updated_ts_ms: i64,
}

impl Default for AudioQueueState {
    fn default() -> Self {
        Self {
            track_ids: Vec::new(),
            current_index: 0,
            position_ms: 0,
            playing: false,
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

pub struct RoomRuntime {
    pub room_id: String,
    pub item_id: String,
    pub room_mode: String,
    pub audio_library_id: Option<String>,
    pub state: RwLock<PlaybackState>,
    pub audio_queue: Option<RwLock<AudioQueueState>>,
    pub youtube_video_id: RwLock<Option<String>>,
    pub connected_user_ids: RwLock<HashSet<String>>,
    pub tx: broadcast::Sender<ServerMessage>,
    pub last_activity_ts_ms: RwLock<i64>,
}

impl RoomRuntime {
    pub fn new(room_id: String, item_id: String) -> Self {
        Self::new_video(room_id, item_id)
    }

    pub fn new_video(room_id: String, item_id: String) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            room_id,
            item_id,
            room_mode: "video".to_string(),
            audio_library_id: None,
            state: RwLock::new(PlaybackState::default()),
            audio_queue: None,
            youtube_video_id: RwLock::new(None),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
        }
    }

    pub fn new_audio(
        room_id: String,
        item_id: String,
        audio_library_id: String,
        track_ids: Vec<String>,
    ) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            room_id,
            item_id,
            room_mode: "audio".to_string(),
            audio_library_id: Some(audio_library_id),
            state: RwLock::new(PlaybackState::default()),
            audio_queue: Some(RwLock::new(AudioQueueState {
                track_ids,
                ..AudioQueueState::default()
            })),
            youtube_video_id: RwLock::new(None),
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
            audio_library_id: None,
            state: RwLock::new(PlaybackState::default()),
            audio_queue: None,
            youtube_video_id: RwLock::new(initial_video_id),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
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

    pub async fn snapshot_state(&self) -> PlaybackState {
        self.state.read().await.clone()
    }

    pub async fn snapshot_audio_queue(&self) -> Option<AudioQueueState> {
        let queue = self.audio_queue.as_ref()?;
        Some(queue.read().await.clone())
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
                    queue.current_index = (queue.current_index + 1) % len;
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

    pub async fn get_or_create_runtime(
        &self,
        room_id: &str,
        item_id: &str,
        room_mode: &str,
        audio_library_id: Option<&str>,
        audio_track_ids: Option<Vec<String>>,
        youtube_video_id: Option<String>,
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
                        audio_library_id.unwrap_or_default().to_string(),
                        audio_track_ids.unwrap_or_default(),
                    )
                } else if room_mode == "youtube" {
                    RoomRuntime::new_youtube(room_id.to_string(), youtube_video_id)
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
}
