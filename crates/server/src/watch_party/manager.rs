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

pub struct RoomRuntime {
    pub room_id: String,
    pub item_id: String,
    pub state: RwLock<PlaybackState>,
    pub connected_user_ids: RwLock<HashSet<String>>,
    pub tx: broadcast::Sender<ServerMessage>,
    pub last_activity_ts_ms: RwLock<i64>,
}

impl RoomRuntime {
    pub fn new(room_id: String, item_id: String) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            room_id,
            item_id,
            state: RwLock::new(PlaybackState::default()),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
            last_activity_ts_ms: RwLock::new(chrono::Utc::now().timestamp_millis()),
        }
    }

    pub async fn snapshot_state(&self) -> PlaybackState {
        self.state.read().await.clone()
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

    pub async fn get_or_create_runtime(&self, room_id: &str, item_id: &str) -> Arc<RoomRuntime> {
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
            .or_insert_with(|| Arc::new(RoomRuntime::new(room_id.to_string(), item_id.to_string())))
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
