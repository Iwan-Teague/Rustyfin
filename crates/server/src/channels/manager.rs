use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast, mpsc};

use super::protocol::{ChannelEvent, UserInfo};

pub struct ChannelManager {
    broadcast: broadcast::Sender<Arc<ChannelEvent>>,
    user_senders: RwLock<HashMap<String, mpsc::UnboundedSender<Arc<ChannelEvent>>>>,
    /// channel_id → Vec<(user_id, username)> in join order
    voice: RwLock<HashMap<String, Vec<(String, String)>>>,
}

impl ChannelManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            broadcast: tx,
            user_senders: RwLock::new(HashMap::new()),
            voice: RwLock::new(HashMap::new()),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<ChannelEvent>> {
        self.broadcast.subscribe()
    }

    pub async fn register_user(
        &self,
        user_id: &str,
        tx: mpsc::UnboundedSender<Arc<ChannelEvent>>,
    ) {
        let mut senders = self.user_senders.write().await;
        senders.insert(user_id.to_string(), tx);
    }

    pub async fn unregister_user(&self, user_id: &str) {
        let mut senders = self.user_senders.write().await;
        senders.remove(user_id);
    }

    pub fn broadcast(&self, event: ChannelEvent) {
        let _ = self.broadcast.send(Arc::new(event));
    }

    pub async fn send_to_user(&self, user_id: &str, event: ChannelEvent) -> bool {
        let senders = self.user_senders.read().await;
        if let Some(tx) = senders.get(user_id) {
            tx.send(Arc::new(event)).is_ok()
        } else {
            false
        }
    }

    /// Adds user to voice channel. Returns list of existing members before this join.
    pub async fn join_voice(
        &self,
        channel_id: &str,
        user_id: &str,
        username: &str,
    ) -> Vec<UserInfo> {
        let mut voice = self.voice.write().await;
        let members = voice.entry(channel_id.to_string()).or_default();

        let existing: Vec<UserInfo> = members
            .iter()
            .filter(|(uid, _)| uid != user_id)
            .map(|(uid, uname)| UserInfo {
                user_id: uid.clone(),
                username: uname.clone(),
            })
            .collect();

        // Remove if already present then re-add (handles re-join)
        members.retain(|(uid, _)| uid != user_id);
        members.push((user_id.to_string(), username.to_string()));

        existing
    }

    pub async fn leave_voice(&self, channel_id: &str, user_id: &str) {
        let mut voice = self.voice.write().await;
        if let Some(members) = voice.get_mut(channel_id) {
            members.retain(|(uid, _)| uid != user_id);
            if members.is_empty() {
                voice.remove(channel_id);
            }
        }
    }

    /// Removes user from all voice channels. Returns list of channel_ids they were in.
    pub async fn leave_all_voice(&self, user_id: &str) -> Vec<String> {
        let mut voice = self.voice.write().await;
        let mut left = Vec::new();
        for (channel_id, members) in voice.iter_mut() {
            let before = members.len();
            members.retain(|(uid, _)| uid != user_id);
            if members.len() < before {
                left.push(channel_id.clone());
            }
        }
        // Clean up empty channels
        voice.retain(|_, members| !members.is_empty());
        left
    }

    pub async fn voice_snapshot(&self) -> HashMap<String, Vec<UserInfo>> {
        let voice = self.voice.read().await;
        voice
            .iter()
            .map(|(channel_id, members)| {
                (
                    channel_id.clone(),
                    members
                        .iter()
                        .map(|(uid, uname)| UserInfo {
                            user_id: uid.clone(),
                            username: uname.clone(),
                        })
                        .collect(),
                )
            })
            .collect()
    }
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new()
    }
}
