#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachNodeMode {
    Person,
    Shortcut,
}

pub fn decide_attach_node_mode(existing_visible_nodes: usize, as_shortcut: bool) -> AttachNodeMode {
    if as_shortcut || existing_visible_nodes > 0 {
        AttachNodeMode::Shortcut
    } else {
        AttachNodeMode::Person
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSelection<'a> {
    pub field_name: &'a str,
    pub workspace_id: &'a str,
    pub workspace_space_id: &'a str,
    pub person_visible_in_workspace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountLinkSelectionError {
    MissingRequiredWorkspace(&'static str),
    CrossSpace { field_name: String },
    PersonNotVisible { field_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMemberRole<'a> {
    pub user_id: &'a str,
    pub role: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceMembershipMutationError {
    LastOwner,
}

pub fn validate_account_link_workspace_selections(
    person_space_id: &str,
    family_workspace: Option<WorkspaceSelection<'_>>,
    friends_workspace: Option<WorkspaceSelection<'_>>,
    work_workspace: Option<WorkspaceSelection<'_>>,
) -> Result<(), AccountLinkSelectionError> {
    let family = family_workspace.ok_or(AccountLinkSelectionError::MissingRequiredWorkspace(
        "family_workspace_id",
    ))?;

    for selection in [Some(family), friends_workspace, work_workspace]
        .into_iter()
        .flatten()
    {
        if selection.workspace_space_id != person_space_id {
            return Err(AccountLinkSelectionError::CrossSpace {
                field_name: selection.field_name.to_string(),
            });
        }
        if !selection.person_visible_in_workspace {
            return Err(AccountLinkSelectionError::PersonNotVisible {
                field_name: selection.field_name.to_string(),
            });
        }
    }

    Ok(())
}

pub fn validate_workspace_membership_change(
    members: &[WorkspaceMemberRole<'_>],
    target_user_id: &str,
    next_role: Option<&str>,
) -> Result<(), WorkspaceMembershipMutationError> {
    let owner_count = members
        .iter()
        .filter(|member| member.role == "owner")
        .count();
    let Some(target) = members
        .iter()
        .find(|member| member.user_id == target_user_id)
    else {
        return Ok(());
    };

    let removing_or_demoting_owner = target.role == "owner" && next_role != Some("owner");
    if removing_or_demoting_owner && owner_count <= 1 {
        return Err(WorkspaceMembershipMutationError::LastOwner);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AccountLinkSelectionError, AttachNodeMode, WorkspaceMemberRole,
        WorkspaceMembershipMutationError, WorkspaceSelection, decide_attach_node_mode,
        validate_account_link_workspace_selections, validate_workspace_membership_change,
    };

    #[test]
    fn attach_mode_uses_person_for_first_visible_placement() {
        assert_eq!(decide_attach_node_mode(0, false), AttachNodeMode::Person);
    }

    #[test]
    fn attach_mode_uses_shortcut_when_person_already_visible() {
        assert_eq!(decide_attach_node_mode(1, false), AttachNodeMode::Shortcut);
    }

    #[test]
    fn attach_mode_respects_explicit_shortcut_request() {
        assert_eq!(decide_attach_node_mode(0, true), AttachNodeMode::Shortcut);
    }

    #[test]
    fn account_link_validation_requires_family_workspace() {
        let result = validate_account_link_workspace_selections("space-a", None, None, None);
        assert_eq!(
            result,
            Err(AccountLinkSelectionError::MissingRequiredWorkspace(
                "family_workspace_id"
            ))
        );
    }

    #[test]
    fn account_link_validation_rejects_cross_space_workspace() {
        let result = validate_account_link_workspace_selections(
            "space-a",
            Some(WorkspaceSelection {
                field_name: "family_workspace_id",
                workspace_id: "ws-family",
                workspace_space_id: "space-a",
                person_visible_in_workspace: true,
            }),
            Some(WorkspaceSelection {
                field_name: "friends_workspace_id",
                workspace_id: "ws-friends",
                workspace_space_id: "space-b",
                person_visible_in_workspace: true,
            }),
            None,
        );

        assert_eq!(
            result,
            Err(AccountLinkSelectionError::CrossSpace {
                field_name: "friends_workspace_id".to_string(),
            })
        );
    }

    #[test]
    fn account_link_validation_rejects_non_visible_workspace() {
        let result = validate_account_link_workspace_selections(
            "space-a",
            Some(WorkspaceSelection {
                field_name: "family_workspace_id",
                workspace_id: "ws-family",
                workspace_space_id: "space-a",
                person_visible_in_workspace: true,
            }),
            None,
            Some(WorkspaceSelection {
                field_name: "work_workspace_id",
                workspace_id: "ws-work",
                workspace_space_id: "space-a",
                person_visible_in_workspace: false,
            }),
        );

        assert_eq!(
            result,
            Err(AccountLinkSelectionError::PersonNotVisible {
                field_name: "work_workspace_id".to_string(),
            })
        );
    }

    #[test]
    fn account_link_validation_accepts_all_valid_workspaces() {
        let result = validate_account_link_workspace_selections(
            "space-a",
            Some(WorkspaceSelection {
                field_name: "family_workspace_id",
                workspace_id: "ws-family",
                workspace_space_id: "space-a",
                person_visible_in_workspace: true,
            }),
            Some(WorkspaceSelection {
                field_name: "friends_workspace_id",
                workspace_id: "ws-friends",
                workspace_space_id: "space-a",
                person_visible_in_workspace: true,
            }),
            Some(WorkspaceSelection {
                field_name: "work_workspace_id",
                workspace_id: "ws-work",
                workspace_space_id: "space-a",
                person_visible_in_workspace: true,
            }),
        );

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn workspace_membership_change_blocks_removing_last_owner() {
        let result = validate_workspace_membership_change(
            &[WorkspaceMemberRole {
                user_id: "user-1",
                role: "owner",
            }],
            "user-1",
            None,
        );

        assert_eq!(result, Err(WorkspaceMembershipMutationError::LastOwner));
    }

    #[test]
    fn workspace_membership_change_blocks_demoting_last_owner() {
        let result = validate_workspace_membership_change(
            &[WorkspaceMemberRole {
                user_id: "user-1",
                role: "owner",
            }],
            "user-1",
            Some("editor"),
        );

        assert_eq!(result, Err(WorkspaceMembershipMutationError::LastOwner));
    }

    #[test]
    fn workspace_membership_change_allows_removing_owner_when_another_owner_exists() {
        let result = validate_workspace_membership_change(
            &[
                WorkspaceMemberRole {
                    user_id: "user-1",
                    role: "owner",
                },
                WorkspaceMemberRole {
                    user_id: "user-2",
                    role: "owner",
                },
            ],
            "user-1",
            None,
        );

        assert_eq!(result, Ok(()));
    }
}
