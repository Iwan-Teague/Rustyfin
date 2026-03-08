use std::collections::HashSet;

use rustfin_core::error::ApiError;

use crate::error::AppError;
use crate::state::AppState;

use super::manager::{PresenceMemberSnapshot, RoomRuntime};
use super::protocol::PresenceMember;

const PRESENCE_MEMBER_CACHE_TTL_MS: i64 = 3_000;

pub(crate) async fn build_presence_members(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
    connected: &HashSet<String>,
) -> Result<Vec<PresenceMember>, AppError> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    if let Some(cached) = runtime
        .get_presence_members_cache(now_ms, PRESENCE_MEMBER_CACHE_TTL_MS)
        .await
    {
        return Ok(presence_members_from_snapshot(cached, connected));
    }

    let members = rustfin_db::repo::watch_party::list_members_with_usernames(&state.db, room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let snapshots: Vec<PresenceMemberSnapshot> = members
        .into_iter()
        .map(|member| PresenceMemberSnapshot {
            user_id: member.user_id,
            username: member.username,
            role: member.role,
            status: member.status,
        })
        .collect();

    runtime
        .set_presence_members_cache(snapshots.clone(), now_ms)
        .await;
    Ok(presence_members_from_snapshot(snapshots, connected))
}

fn presence_members_from_snapshot(
    snapshots: Vec<PresenceMemberSnapshot>,
    connected: &HashSet<String>,
) -> Vec<PresenceMember> {
    snapshots
        .into_iter()
        .filter(|member| member.status != "declined" && member.status != "left")
        .map(|member| PresenceMember {
            connected: connected.contains(&member.user_id) && member.status == "joined",
            user_id: member.user_id,
            username: member.username,
            role: member.role,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_snapshot_filters_and_marks_connected() {
        let snapshots = vec![
            PresenceMemberSnapshot {
                user_id: "u-joined-online".to_string(),
                username: "online".to_string(),
                role: "member".to_string(),
                status: "joined".to_string(),
            },
            PresenceMemberSnapshot {
                user_id: "u-joined-offline".to_string(),
                username: "offline".to_string(),
                role: "admin".to_string(),
                status: "joined".to_string(),
            },
            PresenceMemberSnapshot {
                user_id: "u-invited".to_string(),
                username: "invited".to_string(),
                role: "member".to_string(),
                status: "invited".to_string(),
            },
            PresenceMemberSnapshot {
                user_id: "u-left".to_string(),
                username: "left".to_string(),
                role: "member".to_string(),
                status: "left".to_string(),
            },
            PresenceMemberSnapshot {
                user_id: "u-declined".to_string(),
                username: "declined".to_string(),
                role: "member".to_string(),
                status: "declined".to_string(),
            },
        ];

        let connected: HashSet<String> = ["u-joined-online", "u-invited"]
            .iter()
            .map(|value| value.to_string())
            .collect();

        let members = presence_members_from_snapshot(snapshots, &connected);
        assert_eq!(members.len(), 3);

        let joined_online = members
            .iter()
            .find(|member| member.user_id == "u-joined-online")
            .expect("joined online member");
        assert!(joined_online.connected);

        let joined_offline = members
            .iter()
            .find(|member| member.user_id == "u-joined-offline")
            .expect("joined offline member");
        assert!(!joined_offline.connected);

        let invited = members
            .iter()
            .find(|member| member.user_id == "u-invited")
            .expect("invited member");
        assert!(!invited.connected);
    }
}
