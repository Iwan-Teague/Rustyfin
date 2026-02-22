use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomPolicy {
    #[serde(default = "default_true")]
    pub allow_non_host_play_pause: bool,
    #[serde(default = "default_false")]
    pub allow_non_host_seek: bool,
    #[serde(default = "default_join_role")]
    pub default_join_role: String,
    #[serde(default)]
    pub invite_only: bool,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_join_role() -> String {
    "viewer".to_string()
}

impl Default for RoomPolicy {
    fn default() -> Self {
        Self {
            allow_non_host_play_pause: true,
            allow_non_host_seek: false,
            default_join_role: default_join_role(),
            invite_only: false,
        }
    }
}

pub fn can_play_pause(role: &str, policy: &RoomPolicy) -> bool {
    role == "host" || role == "controller" || (role == "viewer" && policy.allow_non_host_play_pause)
}

pub fn can_seek(role: &str, policy: &RoomPolicy) -> bool {
    role == "host" || role == "controller" || (role == "viewer" && policy.allow_non_host_seek)
}
