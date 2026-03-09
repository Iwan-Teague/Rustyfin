use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{RwLock, broadcast, mpsc};

use super::protocol::{ChannelEvent, UserInfo};

pub struct ChannelManager {
    broadcast: broadcast::Sender<Arc<ChannelEvent>>,
    user_senders: RwLock<HashMap<String, mpsc::Sender<Arc<ChannelEvent>>>>,
    /// channel_id → Vec<(user_id, username, avatar_url)> in join order
    voice: RwLock<HashMap<String, Vec<(String, String, Option<String>)>>>,
    /// channel_id -> unix timestamp (seconds) when the current active voice session began.
    voice_active_since_ts: RwLock<HashMap<String, i64>>,
}

pub struct JoinVoiceResult {
    pub existing_members: Vec<UserInfo>,
    pub active_since_ts: i64,
}

pub struct LeaveVoiceResult {
    pub channel_id: String,
    pub active_since_ts: Option<i64>,
}

impl ChannelManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            broadcast: tx,
            user_senders: RwLock::new(HashMap::new()),
            voice: RwLock::new(HashMap::new()),
            voice_active_since_ts: RwLock::new(HashMap::new()),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<ChannelEvent>> {
        self.broadcast.subscribe()
    }

    pub async fn register_user(&self, user_id: &str, tx: mpsc::Sender<Arc<ChannelEvent>>) {
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
        let sender = {
            let senders = self.user_senders.read().await;
            senders.get(user_id).cloned()
        };
        let Some(sender) = sender else {
            return false;
        };

        match sender.try_send(Arc::new(event)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => {
                let mut senders = self.user_senders.write().await;
                senders.remove(user_id);
                false
            }
        }
    }

    /// Adds user to voice channel. Returns list of existing members before this join.
    pub async fn join_voice(
        &self,
        channel_id: &str,
        user_id: &str,
        username: &str,
        avatar_url: Option<&str>,
    ) -> JoinVoiceResult {
        let mut voice = self.voice.write().await;
        let members = voice.entry(channel_id.to_string()).or_default();

        let existing: Vec<UserInfo> = members
            .iter()
            .filter(|(uid, _, _)| uid != user_id)
            .map(|(uid, uname, avatar)| UserInfo {
                user_id: uid.clone(),
                username: uname.clone(),
                avatar_url: avatar.clone(),
            })
            .collect();

        // Remove if already present then re-add (handles re-join)
        members.retain(|(uid, _, _)| uid != user_id);
        let was_empty = members.is_empty();
        members.push((
            user_id.to_string(),
            username.to_string(),
            avatar_url.map(str::to_string),
        ));

        drop(voice);

        let now = chrono::Utc::now().timestamp();
        let mut active = self.voice_active_since_ts.write().await;
        let active_since_ts = if was_empty {
            active.insert(channel_id.to_string(), now);
            now
        } else {
            active.get(channel_id).copied().unwrap_or_else(|| {
                active.insert(channel_id.to_string(), now);
                now
            })
        };

        JoinVoiceResult {
            existing_members: existing,
            active_since_ts,
        }
    }

    pub async fn leave_voice(&self, channel_id: &str, user_id: &str) -> Option<i64> {
        let mut voice = self.voice.write().await;
        let mut still_active = false;
        if let Some(members) = voice.get_mut(channel_id) {
            members.retain(|(uid, _, _)| uid != user_id);
            if members.is_empty() {
                voice.remove(channel_id);
            } else {
                still_active = true;
            }
        }
        drop(voice);

        let mut active = self.voice_active_since_ts.write().await;
        if still_active {
            active.get(channel_id).copied().or_else(|| {
                let now = chrono::Utc::now().timestamp();
                active.insert(channel_id.to_string(), now);
                Some(now)
            })
        } else {
            active.remove(channel_id);
            None
        }
    }

    /// Removes user from all voice channels. Returns list of channel_ids they were in.
    pub async fn leave_all_voice(&self, user_id: &str) -> Vec<LeaveVoiceResult> {
        let mut voice = self.voice.write().await;
        let mut left_channel_ids = Vec::new();
        let mut still_active_channels = HashSet::new();
        for (channel_id, members) in voice.iter_mut() {
            let before = members.len();
            members.retain(|(uid, _, _)| uid != user_id);
            if members.len() < before {
                left_channel_ids.push(channel_id.clone());
                if !members.is_empty() {
                    still_active_channels.insert(channel_id.clone());
                }
            }
        }
        // Clean up empty channels
        voice.retain(|_, members| !members.is_empty());
        drop(voice);

        let mut active = self.voice_active_since_ts.write().await;
        let mut results = Vec::with_capacity(left_channel_ids.len());
        for channel_id in left_channel_ids {
            if still_active_channels.contains(&channel_id) {
                let active_since_ts = active.get(&channel_id).copied().or_else(|| {
                    let now = chrono::Utc::now().timestamp();
                    active.insert(channel_id.clone(), now);
                    Some(now)
                });
                results.push(LeaveVoiceResult {
                    channel_id,
                    active_since_ts,
                });
            } else {
                active.remove(&channel_id);
                results.push(LeaveVoiceResult {
                    channel_id,
                    active_since_ts: None,
                });
            }
        }

        results
    }

    /// Removes a user from every voice channel except the target one.
    pub async fn leave_other_voice(
        &self,
        user_id: &str,
        keep_channel_id: &str,
    ) -> Vec<LeaveVoiceResult> {
        let mut voice = self.voice.write().await;
        let mut left_channel_ids = Vec::new();
        let mut still_active_channels = HashSet::new();

        for (channel_id, members) in voice.iter_mut() {
            if channel_id == keep_channel_id {
                continue;
            }
            let before = members.len();
            members.retain(|(uid, _, _)| uid != user_id);
            if members.len() < before {
                left_channel_ids.push(channel_id.clone());
                if !members.is_empty() {
                    still_active_channels.insert(channel_id.clone());
                }
            }
        }

        voice.retain(|_, members| !members.is_empty());
        drop(voice);

        let mut active = self.voice_active_since_ts.write().await;
        let mut results = Vec::with_capacity(left_channel_ids.len());
        for channel_id in left_channel_ids {
            if still_active_channels.contains(&channel_id) {
                let active_since_ts = active.get(&channel_id).copied().or_else(|| {
                    let now = chrono::Utc::now().timestamp();
                    active.insert(channel_id.clone(), now);
                    Some(now)
                });
                results.push(LeaveVoiceResult {
                    channel_id,
                    active_since_ts,
                });
            } else {
                active.remove(&channel_id);
                results.push(LeaveVoiceResult {
                    channel_id,
                    active_since_ts: None,
                });
            }
        }

        results
    }

    pub async fn voice_channel_has_user(&self, channel_id: &str, user_id: &str) -> bool {
        let voice = self.voice.read().await;
        voice
            .get(channel_id)
            .map(|members| members.iter().any(|(uid, _, _)| uid == user_id))
            .unwrap_or(false)
    }

    pub async fn voice_channel_has_pair(
        &self,
        channel_id: &str,
        first_user_id: &str,
        second_user_id: &str,
    ) -> bool {
        let voice = self.voice.read().await;
        let Some(members) = voice.get(channel_id) else {
            return false;
        };
        let mut first_found = false;
        let mut second_found = false;
        for (user_id, _, _) in members {
            if user_id == first_user_id {
                first_found = true;
            }
            if user_id == second_user_id {
                second_found = true;
            }
            if first_found && second_found {
                return true;
            }
        }
        false
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
                        .map(|(uid, uname, avatar)| UserInfo {
                            user_id: uid.clone(),
                            username: uname.clone(),
                            avatar_url: avatar.clone(),
                        })
                        .collect(),
                )
            })
            .collect()
    }

    pub async fn voice_active_since_snapshot(&self) -> HashMap<String, i64> {
        self.voice_active_since_ts.read().await.clone()
    }
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn leave_other_voice_keeps_target_channel_membership() {
        let manager = ChannelManager::new();
        manager.join_voice("voice-a", "u-1", "alpha", None).await;
        manager.join_voice("voice-b", "u-1", "alpha", None).await;

        let left = manager.leave_other_voice("u-1", "voice-b").await;
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].channel_id, "voice-a");
        assert!(!manager.voice_channel_has_user("voice-a", "u-1").await);
        assert!(manager.voice_channel_has_user("voice-b", "u-1").await);
    }

    #[tokio::test]
    async fn send_to_user_evicts_full_personal_queue() {
        let manager = ChannelManager::new();
        let (tx, mut rx) = mpsc::channel(1);
        manager.register_user("u-1", tx).await;

        assert!(
            manager
                .send_to_user(
                    "u-1",
                    ChannelEvent::Error {
                        message: "first".to_string(),
                    },
                )
                .await
        );
        assert!(
            !manager
                .send_to_user(
                    "u-1",
                    ChannelEvent::Error {
                        message: "second".to_string(),
                    },
                )
                .await
        );
        assert!(matches!(rx.recv().await, Some(_)));
        assert!(rx.recv().await.is_none());
    }
}
