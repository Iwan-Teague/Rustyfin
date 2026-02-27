use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMsg {
    Auth {
        token: String,
    },
    JoinVoice {
        channel_id: String,
    },
    LeaveVoice {
        channel_id: String,
    },
    RtcOffer {
        to_user_id: String,
        channel_id: String,
        sdp: String,
    },
    RtcAnswer {
        to_user_id: String,
        channel_id: String,
        sdp: String,
    },
    RtcIce {
        to_user_id: String,
        channel_id: String,
        candidate: String,
    },
    SendMessage {
        channel_id: String,
        content: String,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelEvent {
    Hello {
        channels: Vec<ChannelInfo>,
        voice_presence: HashMap<String, Vec<UserInfo>>,
        voice_active_since_ts: HashMap<String, i64>,
        #[serde(default)]
        voice_transcriptions: HashMap<String, VoiceTranscriptionStateInfo>,
    },
    VoicePresence {
        channel_id: String,
        user_id: String,
        username: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        avatar_url: Option<String>,
        joined: bool,
        active_since_ts: Option<i64>,
    },
    VoiceJoined {
        channel_id: String,
        existing_members: Vec<UserInfo>,
    },
    VoiceTranscriptionState {
        channel_id: String,
        state: VoiceTranscriptionStateInfo,
    },
    RtcOffer {
        from_user_id: String,
        channel_id: String,
        sdp: String,
    },
    RtcAnswer {
        from_user_id: String,
        channel_id: String,
        sdp: String,
    },
    RtcIce {
        from_user_id: String,
        channel_id: String,
        candidate: String,
    },
    NewMessage {
        msg: MessageInfo,
    },
    ChannelCreated {
        channel: ChannelInfo,
    },
    ChannelUpdated {
        channel: ChannelInfo,
    },
    ChannelDeleted {
        channel_id: String,
    },
    MessageDeleted {
        message_id: String,
        channel_id: String,
    },
    Pong,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub position: i64,
    pub is_private: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub user_id: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageInfo {
    pub id: String,
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<MessageAttachmentInfo>,
    pub created_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttachmentInfo {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub download_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTranscriptionStateInfo {
    pub status: String,
    pub session_id: Option<String>,
    pub started_by_username: Option<String>,
    pub started_ts: Option<i64>,
    pub ended_ts: Option<i64>,
    pub output_available: bool,
    pub message: Option<String>,
}
