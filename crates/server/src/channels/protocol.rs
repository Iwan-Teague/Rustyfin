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

impl ChannelEvent {
    /// Returns the channel id this event pertains to, if any.
    ///
    /// Used by the per-socket broadcast fan-out to decide whether an event belongs
    /// to a private (admin-only) channel and must therefore be withheld from
    /// non-admin sockets. Events that are not scoped to a single channel (`Hello`,
    /// `Pong`, `Error`) return `None` and are always delivered.
    pub fn channel_id(&self) -> Option<&str> {
        match self {
            ChannelEvent::VoicePresence { channel_id, .. }
            | ChannelEvent::VoiceJoined { channel_id, .. }
            | ChannelEvent::VoiceTranscriptionState { channel_id, .. }
            | ChannelEvent::RtcOffer { channel_id, .. }
            | ChannelEvent::RtcAnswer { channel_id, .. }
            | ChannelEvent::RtcIce { channel_id, .. }
            | ChannelEvent::ChannelDeleted { channel_id }
            | ChannelEvent::MessageDeleted { channel_id, .. } => Some(channel_id),
            ChannelEvent::NewMessage { msg } => Some(&msg.channel_id),
            ChannelEvent::ChannelCreated { channel } | ChannelEvent::ChannelUpdated { channel } => {
                Some(&channel.id)
            }
            ChannelEvent::Hello { .. } | ChannelEvent::Pong | ChannelEvent::Error { .. } => None,
        }
    }
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
