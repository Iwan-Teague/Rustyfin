#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Auth { token: String },
    Play { position_ms: u64 },
    Pause { position_ms: u64 },
    Seek { position_ms: u64 },
    Ping,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PresenceMember {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub connected: bool,
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
    Presence {
        user_id: String,
        connected: bool,
    },
    Error {
        message: String,
    },
    Pong,
}
