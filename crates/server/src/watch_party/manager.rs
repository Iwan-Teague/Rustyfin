use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};

use super::protocol::ServerMessage;

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

pub struct RoomRuntime {
    pub room_id: String,
    pub item_id: String,
    pub state: RwLock<PlaybackState>,
    pub connected_user_ids: RwLock<HashSet<String>>,
    pub tx: broadcast::Sender<ServerMessage>,
}

impl RoomRuntime {
    pub fn new(room_id: String, item_id: String) -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            room_id,
            item_id,
            state: RwLock::new(PlaybackState::default()),
            connected_user_ids: RwLock::new(HashSet::new()),
            tx,
        }
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
                return existing.clone();
            }
        }

        let mut rooms = self.rooms.write().await;
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
