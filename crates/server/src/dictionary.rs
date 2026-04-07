use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::dictionary_hardening_helpers::{
    AttachNodeMode, WorkspaceMemberRole, WorkspaceMembershipMutationError, decide_attach_node_mode,
    validate_workspace_membership_change,
};
use crate::error::AppError;
use crate::state::AppState;
use rustfin_db::repo::dictionary::{
    self, BootstrapWorkspacesResult, CreatePersonInput, CreateTreeNodeInput, CreateWorkspaceInput,
    DictionaryAccountLinkRow, DictionaryDocumentRow, DictionaryFactRow, DictionaryPersonAliasRow,
    DictionaryPersonRow, DictionaryResolvedRelationRow, DictionaryTreeNodeRow,
    DictionaryWorkspaceMemberRow, DictionaryWorkspaceMemberWithUserRow, DictionaryWorkspaceRow,
    FactValueType, MutationSourceKind, RelationPairInput, SaveDocumentInput,
    SearchVisiblePeopleParams, SubjectKind, TreeNodeKind, UpdatePersonInput,
    UpsertAccountLinkInput, UpsertFactInput, WorkspaceKind, WorkspaceRole,
};
use rustfin_db::repo::users;

const MAX_PERSON_NAME_CHARS: usize = 120;
const MAX_PERSON_SUMMARY_CHARS: usize = 320;
const MAX_WORKSPACE_TITLE_CHARS: usize = 80;
const MAX_WORKSPACE_SLUG_CHARS: usize = 80;
const MAX_ALIAS_CHARS: usize = 80;
const MAX_DOCUMENT_TITLE_CHARS: usize = 120;
const MAX_DOCUMENT_BODY_CHARS: usize = 50_000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/bootstrap", post(bootstrap_dictionary))
        .route("/workspaces", get(list_workspaces).post(create_workspace))
        .route("/tree", get(get_tree_by_root))
        .route("/link-account", post(post_link_account))
        .route(
            "/account-link/me",
            get(get_my_account_link).put(put_my_account_link),
        )
        .route("/workspaces/{workspace_id}/tree", get(get_workspace_tree))
        .route(
            "/workspaces/{workspace_id}/members",
            get(list_workspace_members).post(upsert_workspace_member),
        )
        .route(
            "/workspaces/{workspace_id}/members/{user_id}",
            delete(delete_workspace_member),
        )
        .route(
            "/workspaces/{workspace_id}/people",
            get(list_or_search_workspace_people).post(create_workspace_person),
        )
        .route(
            "/workspaces/{workspace_id}/people/attach",
            post(attach_existing_workspace_person),
        )
        .route(
            "/workspaces/{workspace_id}/people/{person_id}",
            get(get_workspace_person_bundle)
                .patch(patch_workspace_person)
                .delete(delete_workspace_person),
        )
        .route(
            "/workspaces/{workspace_id}/people/{person_id}/facts/{fact_key}",
            put(upsert_workspace_person_fact),
        )
        .route(
            "/workspaces/{workspace_id}/people/{person_id}/document",
            get(get_workspace_person_document).patch(save_workspace_person_document),
        )
        .route(
            "/workspaces/{workspace_id}/relationships",
            get(list_workspace_relationships).post(upsert_workspace_relation_pair),
        )
        .route(
            "/workspaces/{workspace_id}/relationships/{relation_id}",
            patch(patch_workspace_relation_pair).delete(delete_workspace_relation_pair),
        )
}

#[derive(Debug, Serialize)]
pub struct BootstrapDictionaryResponse {
    pub workspaces: Vec<DictionaryWorkspaceRow>,
    pub seeded: BootstrapWorkspacesResult,
    pub account_link: Option<DictionaryAccountLinkRow>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceTreeResponse {
    pub workspace: DictionaryWorkspaceRow,
    pub nodes: Vec<DictionaryTreeNodeRow>,
}

#[derive(Debug, Serialize)]
pub struct PersonBundleResponse {
    pub workspace: DictionaryWorkspaceRow,
    pub person: DictionaryPersonRow,
    pub aliases: Vec<DictionaryPersonAliasRow>,
    pub nodes: Vec<DictionaryTreeNodeRow>,
    pub facts: Vec<DictionaryFactRow>,
    pub relations: Vec<DictionaryResolvedRelationRow>,
    pub document: Option<DictionaryDocumentRow>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub title: String,
    #[serde(default)]
    pub slug: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TreeRootQuery {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchPeopleQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePersonRequest {
    pub display_name: String,
    #[serde(default)]
    pub canonical_name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub parent_node_id: Option<String>,
    #[serde(default)]
    pub node_title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePersonRequest {
    pub display_name: String,
    #[serde(default)]
    pub canonical_name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub aliases_to_add: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertPersonFactRequest {
    pub value_type: String,
    #[serde(default)]
    pub value_text: Option<String>,
    #[serde(default)]
    pub value_int: Option<i64>,
    #[serde(default)]
    pub value_bool: Option<bool>,
    #[serde(default)]
    pub value_date: Option<String>,
    #[serde(default)]
    pub value_json: Option<serde_json::Value>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub source_note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SavePersonDocumentRequest {
    pub title: String,
    pub markdown_body: String,
    #[serde(default)]
    pub edit_note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RelationshipQuery {
    pub person_id: String,
}

#[derive(Debug, Deserialize)]
pub struct UpsertRelationPairRequest {
    pub from_person_id: String,
    pub to_person_id: String,
    pub relation_type: String,
    pub inverse_relation_type: String,
    #[serde(default)]
    pub source_note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PutMyAccountLinkRequest {
    pub person_id: String,
    #[serde(default)]
    pub family_workspace_id: Option<String>,
    #[serde(default)]
    pub friends_workspace_id: Option<String>,
    #[serde(default)]
    pub work_workspace_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertWorkspaceMemberRequest {
    pub login_username: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct AttachExistingPersonRequest {
    pub person_id: String,
    #[serde(default)]
    pub parent_node_id: Option<String>,
    #[serde(default)]
    pub node_title: Option<String>,
    #[serde(default)]
    pub as_shortcut: bool,
}

#[derive(Debug, Serialize)]
pub struct DictionaryWorkspaceMemberView {
    pub workspace_id: String,
    pub user_id: String,
    pub login_username: String,
    pub display_name: String,
    pub role: String,
    pub added_by_user_id: Option<String>,
    pub created_ts: i64,
}

fn normalize_person_name(raw: &str) -> Result<String, AppError> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("person name must not be empty".into()).into());
    }
    if trimmed.chars().count() > MAX_PERSON_NAME_CHARS {
        return Err(ApiError::BadRequest(format!(
            "person name must be at most {MAX_PERSON_NAME_CHARS} characters"
        ))
        .into());
    }
    Ok(trimmed.to_string())
}

fn normalize_optional_summary(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(value) = raw else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_PERSON_SUMMARY_CHARS {
        return Err(ApiError::BadRequest(format!(
            "summary must be at most {MAX_PERSON_SUMMARY_CHARS} characters"
        ))
        .into());
    }
    Ok(Some(trimmed.to_string()))
}

fn normalize_alias(raw: &str) -> Result<String, AppError> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("alias must not be empty".into()).into());
    }
    if trimmed.chars().count() > MAX_ALIAS_CHARS {
        return Err(ApiError::BadRequest(format!(
            "alias must be at most {MAX_ALIAS_CHARS} characters"
        ))
        .into());
    }
    Ok(trimmed.to_string())
}

fn normalize_relation_type(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim().to_ascii_lowercase().replace('-', "_");
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("relation type must not be empty".into()).into());
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(ApiError::BadRequest(
            "relation type must use lowercase letters, digits, and underscore".into(),
        )
        .into());
    }
    Ok(trimmed)
}

fn normalize_fact_key(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("fact key must not be empty".into()).into());
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '.')
    {
        return Err(ApiError::BadRequest(
            "fact key must use lowercase letters, digits, underscore, or dot".into(),
        )
        .into());
    }
    Ok(trimmed)
}

fn parse_fact_value_type(raw: &str) -> Result<FactValueType, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "text" => Ok(FactValueType::Text),
        "int" => Ok(FactValueType::Int),
        "bool" => Ok(FactValueType::Bool),
        "date" => Ok(FactValueType::Date),
        "json" => Ok(FactValueType::Json),
        _ => Err(ApiError::BadRequest(
            "value_type must be one of: text, int, bool, date, json".into(),
        )
        .into()),
    }
}

fn dictionary_workspace_member_view_from_row(
    row: DictionaryWorkspaceMemberWithUserRow,
) -> DictionaryWorkspaceMemberView {
    DictionaryWorkspaceMemberView {
        workspace_id: row.workspace_id,
        user_id: row.user_id,
        login_username: row.login_username,
        display_name: row.display_name,
        role: row.role,
        added_by_user_id: row.added_by_user_id,
        created_ts: row.created_ts,
    }
}

fn validate_document_title(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("document title must not be empty".into()).into());
    }
    if trimmed.chars().count() > MAX_DOCUMENT_TITLE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "document title must be at most {MAX_DOCUMENT_TITLE_CHARS} characters"
        ))
        .into());
    }
    Ok(trimmed.to_string())
}

fn validate_document_body(markdown_body: &str) -> Result<(), AppError> {
    if markdown_body.chars().count() > MAX_DOCUMENT_BODY_CHARS {
        return Err(ApiError::BadRequest(format!(
            "document body must be at most {MAX_DOCUMENT_BODY_CHARS} characters"
        ))
        .into());
    }
    Ok(())
}

fn normalize_workspace_title(raw: &str) -> Result<String, AppError> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("workspace title must not be empty".into()).into());
    }
    if trimmed.chars().count() > MAX_WORKSPACE_TITLE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "workspace title must be at most {MAX_WORKSPACE_TITLE_CHARS} characters"
        ))
        .into());
    }
    Ok(trimmed.to_string())
}

fn normalize_workspace_slug(title: &str, slug: Option<&str>) -> Result<String, AppError> {
    let source = slug.unwrap_or(title).trim().to_ascii_lowercase();
    let mut out = String::new();
    let mut last_dash = false;
    for ch in source.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch)
        } else if matches!(ch, ' ' | '_' | '-' | '/' | '.') {
            Some('-')
        } else {
            None
        };
        let Some(normalized) = normalized else {
            continue;
        };
        if normalized == '-' {
            if out.is_empty() || last_dash {
                continue;
            }
            last_dash = true;
            out.push('-');
        } else {
            last_dash = false;
            out.push(normalized);
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("workspace slug must not be empty".into()).into());
    }
    if trimmed.chars().count() > MAX_WORKSPACE_SLUG_CHARS {
        return Err(ApiError::BadRequest(format!(
            "workspace slug must be at most {MAX_WORKSPACE_SLUG_CHARS} characters"
        ))
        .into());
    }
    Ok(trimmed)
}

fn root_kind_to_workspace_kind(root: &str) -> Option<WorkspaceKind> {
    match root.trim().to_ascii_lowercase().as_str() {
        "family" => Some(WorkspaceKind::FamilyShared),
        "friends" => Some(WorkspaceKind::FriendsPrivate),
        "work" => Some(WorkspaceKind::WorkPrivate),
        _ => None,
    }
}

async fn ensure_default_space(
    state: &AppState,
    auth: &AuthUser,
) -> Result<rustfin_db::repo::dictionary::DictionarySpaceRow, AppError> {
    dictionary::ensure_default_household_space(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")).into())
}

async fn ensure_workspace_role(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: &str,
) -> Result<(DictionaryWorkspaceRow, DictionaryWorkspaceMemberRow), AppError> {
    let workspace = dictionary::find_workspace_by_id(&state.db, workspace_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("dictionary workspace not found".into()))?;

    let membership = dictionary::get_workspace_member(&state.db, workspace_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::Forbidden("dictionary workspace access denied".into()))?;

    Ok((workspace, membership))
}

async fn ensure_workspace_read_access(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: &str,
) -> Result<DictionaryWorkspaceRow, AppError> {
    ensure_workspace_role(state, auth, workspace_id)
        .await
        .map(|(workspace, _)| workspace)
}

async fn ensure_workspace_write_access(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: &str,
) -> Result<DictionaryWorkspaceRow, AppError> {
    let (workspace, membership) = ensure_workspace_role(state, auth, workspace_id).await?;
    let role = WorkspaceRole::from_str(&membership.role)
        .ok_or_else(|| ApiError::Internal("invalid dictionary membership role".into()))?;
    if !role.can_write() {
        return Err(ApiError::Forbidden(
            "dictionary workspace is read-only for this account".into(),
        )
        .into());
    }
    Ok(workspace)
}

fn parse_workspace_role_strict(role: &str) -> Result<WorkspaceRole, AppError> {
    WorkspaceRole::from_str(role)
        .ok_or_else(|| ApiError::Internal("invalid dictionary membership role".into()).into())
}

async fn ensure_workspace_manage_access(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: &str,
) -> Result<DictionaryWorkspaceRow, AppError> {
    let (workspace, membership) = ensure_workspace_role(state, auth, workspace_id).await?;
    let role = parse_workspace_role_strict(&membership.role)?;
    if !role.can_manage() {
        return Err(
            ApiError::Forbidden("dictionary workspace membership is owner-managed".into()).into(),
        );
    }
    Ok(workspace)
}

async fn ensure_tree_parent_in_workspace(
    state: &AppState,
    workspace_id: &str,
    parent_node_id: &str,
) -> Result<(), AppError> {
    let parent = dictionary::find_tree_node_by_id(&state.db, parent_node_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::BadRequest("parent node not found".into()))?;
    if parent.workspace_id != workspace_id {
        return Err(ApiError::BadRequest("parent node is not in this workspace".into()).into());
    }
    Ok(())
}

async fn ensure_person_visible_in_workspace(
    state: &AppState,
    workspace_id: &str,
    person_id: &str,
) -> Result<DictionaryPersonRow, AppError> {
    let person = dictionary::find_person_by_id(&state.db, person_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("person not found".into()))?;
    let nodes = dictionary::list_tree_nodes_for_person(&state.db, workspace_id, person_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    if nodes.is_empty() {
        return Err(ApiError::Forbidden("person is not visible in this workspace".into()).into());
    }
    Ok(person)
}

async fn ensure_people_share_workspace(
    state: &AppState,
    workspace_id: &str,
    person_ids: &[&str],
) -> Result<(), AppError> {
    for person_id in person_ids {
        let _ = ensure_person_visible_in_workspace(state, workspace_id, person_id).await?;
    }
    Ok(())
}

async fn default_parent_node_id(
    state: &AppState,
    workspace_id: &str,
) -> Result<Option<String>, AppError> {
    let nodes = dictionary::list_tree_nodes(&state.db, workspace_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    if let Some(node) = nodes.iter().find(|node| node.node_kind == "group") {
        return Ok(Some(node.id.clone()));
    }
    if let Some(node) = nodes.iter().find(|node| node.node_kind == "root") {
        return Ok(Some(node.id.clone()));
    }
    Ok(None)
}

fn normalize_login_username(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("login_username must not be empty".into()).into());
    }
    if trimmed.len() > 64 {
        return Err(ApiError::BadRequest("login_username is too long".into()).into());
    }
    Ok(trimmed)
}

fn parse_requested_member_role(raw: &str) -> Result<WorkspaceRole, AppError> {
    WorkspaceRole::from_str(raw.trim().to_ascii_lowercase().as_str()).ok_or_else(|| {
        ApiError::BadRequest("role must be one of: owner, editor, viewer".into()).into()
    })
}

async fn validate_selected_link_workspace(
    state: &AppState,
    auth: &AuthUser,
    person: &DictionaryPersonRow,
    workspace_id: &str,
    field_name: &'static str,
) -> Result<String, AppError> {
    let workspace = dictionary::find_workspace_by_id(&state.db, workspace_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| {
            ApiError::BadRequest(format!(
                "{field_name} must reference an existing dictionary workspace"
            ))
        })?;

    let can_read = dictionary::user_can_access_workspace(&state.db, workspace_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    if !can_read {
        return Err(ApiError::Forbidden(format!(
            "{field_name} must reference a dictionary workspace you can read"
        ))
        .into());
    }

    if workspace.space_id != person.space_id {
        return Err(ApiError::BadRequest(format!(
            "{field_name} must belong to the same dictionary space as the linked person"
        ))
        .into());
    }

    let _ = ensure_person_visible_in_workspace(state, &workspace.id, &person.id)
        .await
        .map_err(|_| {
            ApiError::BadRequest(format!(
                "linked person must be visible in {field_name} before the account link can be saved"
            ))
        })?;

    Ok(workspace.id)
}

async fn ensure_workspace_seeded(
    state: &AppState,
    auth: &AuthUser,
    workspace: &DictionaryWorkspaceRow,
) -> Result<(), AppError> {
    let existing = dictionary::list_tree_nodes(&state.db, &workspace.id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    if !existing.is_empty() {
        return Ok(());
    }

    let root_title = match workspace.workspace_kind.as_str() {
        "family_shared" => "Family",
        "friends_private" => "Friends",
        "work_private" => "Work",
        _ => &workspace.title,
    };
    let root = dictionary::create_tree_node(
        &state.db,
        &CreateTreeNodeInput {
            workspace_id: workspace.id.clone(),
            parent_node_id: None,
            node_kind: TreeNodeKind::Root,
            title: root_title.to_string(),
            person_id: None,
            sort_order: 0,
            icon_name: None,
            note: None,
            is_system_seeded: true,
            created_by_user_id: auth.user_id.clone(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    let group_titles = match workspace.workspace_kind.as_str() {
        "family_shared" => vec!["Immediate", "Extended", "Household"],
        "friends_private" => vec!["Close Friends", "Social", "Acquaintances"],
        "work_private" => vec!["Team", "Leadership", "External"],
        _ => vec!["People"],
    };

    for (index, title) in group_titles.into_iter().enumerate() {
        dictionary::create_tree_node(
            &state.db,
            &CreateTreeNodeInput {
                workspace_id: workspace.id.clone(),
                parent_node_id: Some(root.id.clone()),
                node_kind: TreeNodeKind::Group,
                title: title.to_string(),
                person_id: None,
                sort_order: index as i32,
                icon_name: None,
                note: None,
                is_system_seeded: true,
                created_by_user_id: auth.user_id.clone(),
            },
        )
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    }

    Ok(())
}

async fn build_person_bundle(
    state: &AppState,
    workspace: DictionaryWorkspaceRow,
    person_id: &str,
) -> Result<PersonBundleResponse, AppError> {
    let person = ensure_person_visible_in_workspace(state, &workspace.id, person_id).await?;
    let aliases = dictionary::list_person_aliases(&state.db, person_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let nodes = dictionary::list_tree_nodes_for_person(&state.db, &workspace.id, person_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let facts = dictionary::list_facts_for_subject(
        &state.db,
        &workspace.id,
        SubjectKind::Person,
        person_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let relations =
        dictionary::list_resolved_relations_for_person(&state.db, &workspace.id, person_id)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let document = dictionary::get_document_for_subject(
        &state.db,
        &workspace.id,
        SubjectKind::Person,
        person_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    Ok(PersonBundleResponse {
        workspace,
        person,
        aliases,
        nodes,
        facts,
        relations,
        document,
    })
}

async fn resolve_tree_workspace(
    state: &AppState,
    auth: &AuthUser,
    query: &TreeRootQuery,
) -> Result<DictionaryWorkspaceRow, AppError> {
    if let Some(workspace_id) = query.workspace_id.as_deref() {
        return ensure_workspace_read_access(state, auth, workspace_id).await;
    }

    let workspaces = dictionary::list_visible_workspaces(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let Some(root_kind) = query.root.as_deref().and_then(root_kind_to_workspace_kind) else {
        return workspaces.into_iter().next().ok_or_else(|| {
            ApiError::NotFound("no dictionary workspaces are available".into()).into()
        });
    };

    workspaces
        .into_iter()
        .find(|workspace| WorkspaceKind::from_str(&workspace.workspace_kind) == Some(root_kind))
        .ok_or_else(|| {
            ApiError::NotFound("dictionary workspace not found for that root".into()).into()
        })
}

async fn bootstrap_dictionary(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<BootstrapDictionaryResponse>, AppError> {
    let space = ensure_default_space(&state, &auth).await?;
    let seeded =
        dictionary::ensure_default_workspaces_for_user(&state.db, &space.id, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    for workspace in [
        &seeded.family_workspace,
        &seeded.friends_workspace,
        &seeded.work_workspace,
    ] {
        ensure_workspace_seeded(&state, &auth, workspace).await?;
    }

    let workspaces = dictionary::list_visible_workspaces(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let account_link = dictionary::get_account_link(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    Ok(Json(BootstrapDictionaryResponse {
        workspaces,
        seeded,
        account_link,
    }))
}

async fn list_workspaces(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<DictionaryWorkspaceRow>>, AppError> {
    let space = ensure_default_space(&state, &auth).await?;
    let seeded =
        dictionary::ensure_default_workspaces_for_user(&state.db, &space.id, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    for workspace in [
        &seeded.family_workspace,
        &seeded.friends_workspace,
        &seeded.work_workspace,
    ] {
        ensure_workspace_seeded(&state, &auth, workspace).await?;
    }
    let workspaces = dictionary::list_visible_workspaces(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(workspaces))
}

async fn create_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Result<Json<DictionaryWorkspaceRow>, AppError> {
    let title = normalize_workspace_title(&req.title)?;
    let slug = normalize_workspace_slug(&title, req.slug.as_deref())?;
    let space = ensure_default_space(&state, &auth).await?;

    if dictionary::find_workspace_by_slug(&state.db, &space.id, &slug)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .is_some()
    {
        return Err(ApiError::Conflict(
            "a dictionary workspace with that slug already exists".into(),
        )
        .into());
    }

    let workspace = dictionary::create_workspace(
        &state.db,
        &CreateWorkspaceInput {
            space_id: space.id,
            slug,
            title,
            workspace_kind: WorkspaceKind::Custom,
            owner_user_id: Some(auth.user_id.clone()),
            is_system_seeded: false,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    dictionary::ensure_workspace_member(
        &state.db,
        &workspace.id,
        &auth.user_id,
        WorkspaceRole::Owner,
        Some(&auth.user_id),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    ensure_workspace_seeded(&state, &auth, &workspace).await?;

    Ok(Json(workspace))
}

async fn get_tree_by_root(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<TreeRootQuery>,
) -> Result<Json<WorkspaceTreeResponse>, AppError> {
    let workspace = resolve_tree_workspace(&state, &auth, &query).await?;
    ensure_workspace_seeded(&state, &auth, &workspace).await?;
    let nodes = dictionary::list_tree_nodes(&state.db, &workspace.id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(WorkspaceTreeResponse { workspace, nodes }))
}

async fn post_link_account(
    state: State<AppState>,
    auth: AuthUser,
    payload: Json<PutMyAccountLinkRequest>,
) -> Result<Json<DictionaryAccountLinkRow>, AppError> {
    put_my_account_link(state, auth, payload).await
}

async fn get_my_account_link(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Option<DictionaryAccountLinkRow>>, AppError> {
    let row = dictionary::get_account_link(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(row))
}

async fn put_my_account_link(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<PutMyAccountLinkRequest>,
) -> Result<Json<DictionaryAccountLinkRow>, AppError> {
    let person_id = req.person_id.trim();
    if person_id.is_empty() {
        return Err(ApiError::BadRequest("person_id must not be empty".into()).into());
    }

    let person = dictionary::find_person_by_id(&state.db, person_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("linked person not found".into()))?;

    let family_workspace_id = req
        .family_workspace_id
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("family_workspace_id is required".into()))?;

    let family_workspace_id = validate_selected_link_workspace(
        &state,
        &auth,
        &person,
        family_workspace_id,
        "family_workspace_id",
    )
    .await?;

    let friends_workspace_id = match req.friends_workspace_id.as_deref() {
        Some(workspace_id) => Some(
            validate_selected_link_workspace(
                &state,
                &auth,
                &person,
                workspace_id,
                "friends_workspace_id",
            )
            .await?,
        ),
        None => None,
    };

    let work_workspace_id = match req.work_workspace_id.as_deref() {
        Some(workspace_id) => Some(
            validate_selected_link_workspace(
                &state,
                &auth,
                &person,
                workspace_id,
                "work_workspace_id",
            )
            .await?,
        ),
        None => None,
    };

    let row = dictionary::upsert_account_link(
        &state.db,
        &UpsertAccountLinkInput {
            user_id: auth.user_id.clone(),
            space_id: person.space_id.clone(),
            person_id: person.id.clone(),
            family_workspace_id: Some(family_workspace_id),
            friends_workspace_id,
            work_workspace_id,
            created_by_user_id: auth.user_id.clone(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    Ok(Json(row))
}

async fn list_workspace_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<String>,
) -> Result<Json<Vec<DictionaryWorkspaceMemberView>>, AppError> {
    let workspace = ensure_workspace_manage_access(&state, &auth, &workspace_id).await?;
    let rows = dictionary::list_workspace_members_with_users(&state.db, &workspace.id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(
        rows.into_iter()
            .map(dictionary_workspace_member_view_from_row)
            .collect(),
    ))
}

async fn upsert_workspace_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<String>,
    Json(req): Json<UpsertWorkspaceMemberRequest>,
) -> Result<Json<Vec<DictionaryWorkspaceMemberView>>, AppError> {
    let workspace = ensure_workspace_manage_access(&state, &auth, &workspace_id).await?;
    let login_username = normalize_login_username(&req.login_username)?;
    let role = parse_requested_member_role(&req.role)?;

    let target_user = users::find_by_username(&state.db, &login_username)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("dictionary member user not found".into()))?;

    let current_rows = dictionary::list_workspace_members_with_users(&state.db, &workspace.id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let target_existing = current_rows
        .iter()
        .find(|row| row.user_id == target_user.id)
        .cloned();

    if let Some(existing) = target_existing
        && let Err(WorkspaceMembershipMutationError::LastOwner) =
            validate_workspace_membership_change(
                &current_rows
                    .iter()
                    .map(|row| WorkspaceMemberRole {
                        user_id: row.user_id.as_str(),
                        role: row.role.as_str(),
                    })
                    .collect::<Vec<_>>(),
                &existing.user_id,
                Some(role.as_str()),
            )
    {
        return Err(ApiError::Conflict("cannot demote the last workspace owner".into()).into());
    }

    dictionary::ensure_workspace_member(
        &state.db,
        &workspace.id,
        &target_user.id,
        role,
        Some(&auth.user_id),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    let rows = dictionary::list_workspace_members_with_users(&state.db, &workspace.id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(
        rows.into_iter()
            .map(dictionary_workspace_member_view_from_row)
            .collect(),
    ))
}

async fn delete_workspace_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let workspace = ensure_workspace_manage_access(&state, &auth, &workspace_id).await?;
    let current_rows = dictionary::list_workspace_members_with_users(&state.db, &workspace.id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let target = current_rows
        .iter()
        .find(|row| row.user_id == user_id)
        .ok_or_else(|| ApiError::NotFound("workspace member not found".into()))?;
    if let Err(WorkspaceMembershipMutationError::LastOwner) = validate_workspace_membership_change(
        &current_rows
            .iter()
            .map(|row| WorkspaceMemberRole {
                user_id: row.user_id.as_str(),
                role: row.role.as_str(),
            })
            .collect::<Vec<_>>(),
        &target.user_id,
        None,
    ) {
        return Err(ApiError::Conflict("cannot remove the last workspace owner".into()).into());
    }
    let deleted = dictionary::delete_workspace_member(&state.db, &workspace.id, &user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

async fn get_workspace_tree(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<String>,
) -> Result<Json<WorkspaceTreeResponse>, AppError> {
    let workspace = ensure_workspace_read_access(&state, &auth, &workspace_id).await?;
    ensure_workspace_seeded(&state, &auth, &workspace).await?;
    let nodes = dictionary::list_tree_nodes(&state.db, &workspace.id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(WorkspaceTreeResponse { workspace, nodes }))
}

async fn list_or_search_workspace_people(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<String>,
    Query(query): Query<SearchPeopleQuery>,
) -> Result<Json<Vec<DictionaryPersonRow>>, AppError> {
    let workspace = ensure_workspace_read_access(&state, &auth, &workspace_id).await?;
    let rows = if let Some(q) = query.q.as_deref().filter(|value| !value.trim().is_empty()) {
        dictionary::search_visible_people(
            &state.db,
            &SearchVisiblePeopleParams {
                workspace_id: workspace.id.clone(),
                query: q.to_string(),
                limit: query.limit.unwrap_or(12),
            },
        )
        .await
    } else {
        dictionary::list_visible_people(&state.db, &workspace.id, query.limit.unwrap_or(200)).await
    }
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(rows))
}

async fn create_workspace_person(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<String>,
    Json(req): Json<CreatePersonRequest>,
) -> Result<Json<PersonBundleResponse>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    let display_name = normalize_person_name(&req.display_name)?;
    let canonical_name =
        normalize_person_name(req.canonical_name.as_deref().unwrap_or(&display_name))?;
    let summary = normalize_optional_summary(req.summary.as_deref())?;

    if let Some(parent_node_id) = req.parent_node_id.as_deref() {
        ensure_tree_parent_in_workspace(&state, &workspace.id, parent_node_id).await?;
    }

    let person = dictionary::create_person(
        &state.db,
        &CreatePersonInput {
            space_id: workspace.space_id.clone(),
            canonical_name,
            display_name: display_name.clone(),
            summary,
            created_by_user_id: auth.user_id.clone(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    for alias in &req.aliases {
        let normalized = normalize_alias(alias)?;
        dictionary::add_person_alias(&state.db, &person.id, &normalized, "custom", &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    }

    let parent_node_id = match req.parent_node_id {
        Some(parent) => Some(parent),
        None => default_parent_node_id(&state, &workspace.id).await?,
    };
    dictionary::create_tree_node(
        &state.db,
        &CreateTreeNodeInput {
            workspace_id: workspace.id.clone(),
            parent_node_id,
            node_kind: TreeNodeKind::Person,
            title: req.node_title.unwrap_or_else(|| display_name.clone()),
            person_id: Some(person.id.clone()),
            sort_order: 0,
            icon_name: None,
            note: None,
            is_system_seeded: false,
            created_by_user_id: auth.user_id.clone(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    Ok(Json(
        build_person_bundle(&state, workspace, &person.id).await?,
    ))
}

async fn attach_existing_workspace_person(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<String>,
    Json(req): Json<AttachExistingPersonRequest>,
) -> Result<Json<PersonBundleResponse>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    let person_id = req.person_id.trim();
    if person_id.is_empty() {
        return Err(ApiError::BadRequest("person_id must not be empty".into()).into());
    }

    let person = dictionary::find_person_by_id(&state.db, person_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("person not found".into()))?;
    if person.space_id != workspace.space_id {
        return Err(ApiError::BadRequest(
            "person must belong to the same dictionary space as the target workspace".into(),
        )
        .into());
    }

    if let Some(parent_node_id) = req.parent_node_id.as_deref() {
        ensure_tree_parent_in_workspace(&state, &workspace.id, parent_node_id).await?;
    }

    let existing_nodes =
        dictionary::list_tree_nodes_for_person(&state.db, &workspace.id, &person.id)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    let parent_node_id = match req.parent_node_id {
        Some(parent) => Some(parent),
        None => default_parent_node_id(&state, &workspace.id).await?,
    };
    let node_kind = match decide_attach_node_mode(existing_nodes.len(), req.as_shortcut) {
        AttachNodeMode::Person => TreeNodeKind::Person,
        AttachNodeMode::Shortcut => TreeNodeKind::Shortcut,
    };
    let title = match req.node_title {
        Some(raw) if !raw.trim().is_empty() => raw.trim().to_string(),
        _ => person.display_name.clone(),
    };

    dictionary::attach_existing_person_to_workspace(
        &state.db,
        &CreateTreeNodeInput {
            workspace_id: workspace.id.clone(),
            parent_node_id,
            node_kind,
            title,
            person_id: Some(person.id.clone()),
            sort_order: 0,
            icon_name: None,
            note: None,
            is_system_seeded: false,
            created_by_user_id: auth.user_id.clone(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    Ok(Json(
        build_person_bundle(&state, workspace, &person.id).await?,
    ))
}

async fn get_workspace_person_bundle(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, person_id)): Path<(String, String)>,
) -> Result<Json<PersonBundleResponse>, AppError> {
    let workspace = ensure_workspace_read_access(&state, &auth, &workspace_id).await?;
    Ok(Json(
        build_person_bundle(&state, workspace, &person_id).await?,
    ))
}

async fn patch_workspace_person(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, person_id)): Path<(String, String)>,
    Json(req): Json<UpdatePersonRequest>,
) -> Result<Json<PersonBundleResponse>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    let _ = ensure_person_visible_in_workspace(&state, &workspace.id, &person_id).await?;

    let display_name = normalize_person_name(&req.display_name)?;
    let canonical_name =
        normalize_person_name(req.canonical_name.as_deref().unwrap_or(&display_name))?;
    let summary = normalize_optional_summary(req.summary.as_deref())?;

    let updated = dictionary::update_person(
        &state.db,
        &UpdatePersonInput {
            person_id: person_id.clone(),
            display_name,
            canonical_name,
            summary,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
    .ok_or_else(|| ApiError::NotFound("person not found".into()))?;

    for alias in &req.aliases_to_add {
        let normalized = normalize_alias(alias)?;
        dictionary::add_person_alias(&state.db, &updated.id, &normalized, "custom", &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    }

    Ok(Json(
        build_person_bundle(&state, workspace, &updated.id).await?,
    ))
}

async fn delete_workspace_person(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, person_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    let _ = ensure_person_visible_in_workspace(&state, &workspace.id, &person_id).await?;

    if let Some(account_link) = dictionary::get_account_link(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        && account_link.person_id == person_id
    {
        return Err(ApiError::Conflict(
            "unlink your Rustyfin account from this person before deleting them from the dictionary".into(),
        )
        .into());
    }

    let deleted = dictionary::archive_person_from_workspace(&state.db, &workspace.id, &person_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

async fn upsert_workspace_person_fact(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, person_id, fact_key)): Path<(String, String, String)>,
    Json(req): Json<UpsertPersonFactRequest>,
) -> Result<Json<DictionaryFactRow>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    let _ = ensure_person_visible_in_workspace(&state, &workspace.id, &person_id).await?;
    let fact_key = normalize_fact_key(&fact_key)?;
    let value_type = parse_fact_value_type(&req.value_type)?;

    let row = dictionary::upsert_fact(
        &state.db,
        &UpsertFactInput {
            workspace_id: workspace.id,
            subject_kind: SubjectKind::Person,
            subject_id: person_id,
            fact_key,
            value_type,
            value_text: req.value_text.clone(),
            value_int: req.value_int,
            value_bool: req.value_bool,
            value_date: req.value_date.clone(),
            value_json: req.value_json.clone(),
            unit: req.unit.clone(),
            confidence: req.confidence,
            status: "confirmed".to_string(),
            source_kind: MutationSourceKind::Manual,
            source_user_id: Some(auth.user_id),
            source_note: req.source_note.clone(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    Ok(Json(row))
}

async fn get_workspace_person_document(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, person_id)): Path<(String, String)>,
) -> Result<Json<Option<DictionaryDocumentRow>>, AppError> {
    let workspace = ensure_workspace_read_access(&state, &auth, &workspace_id).await?;
    let _ = ensure_person_visible_in_workspace(&state, &workspace.id, &person_id).await?;
    let document = dictionary::get_document_for_subject(
        &state.db,
        &workspace.id,
        SubjectKind::Person,
        &person_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(document))
}

async fn save_workspace_person_document(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, person_id)): Path<(String, String)>,
    Json(req): Json<SavePersonDocumentRequest>,
) -> Result<Json<DictionaryDocumentRow>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    let _ = ensure_person_visible_in_workspace(&state, &workspace.id, &person_id).await?;
    let title = validate_document_title(&req.title)?;
    validate_document_body(&req.markdown_body)?;

    let document = dictionary::save_document(
        &state.db,
        &SaveDocumentInput {
            workspace_id: workspace.id,
            subject_kind: SubjectKind::Person,
            subject_id: person_id,
            title,
            markdown_body: req.markdown_body,
            edited_by_user_id: Some(auth.user_id),
            edit_source_kind: MutationSourceKind::Manual,
            edit_note: req.edit_note,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    Ok(Json(document))
}

async fn list_workspace_relationships(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<String>,
    Query(query): Query<RelationshipQuery>,
) -> Result<Json<Vec<DictionaryResolvedRelationRow>>, AppError> {
    let workspace = ensure_workspace_read_access(&state, &auth, &workspace_id).await?;
    let _ = ensure_person_visible_in_workspace(&state, &workspace.id, &query.person_id).await?;
    let relations =
        dictionary::list_resolved_relations_for_person(&state.db, &workspace.id, &query.person_id)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(relations))
}

async fn upsert_workspace_relation_pair(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<String>,
    Json(req): Json<UpsertRelationPairRequest>,
) -> Result<Json<Vec<DictionaryResolvedRelationRow>>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    ensure_people_share_workspace(
        &state,
        &workspace.id,
        &[&req.from_person_id, &req.to_person_id],
    )
    .await?;
    let relation_type = normalize_relation_type(&req.relation_type)?;
    let inverse_relation_type = normalize_relation_type(&req.inverse_relation_type)?;

    dictionary::upsert_relation_pair(
        &state.db,
        &RelationPairInput {
            workspace_id: workspace.id.clone(),
            from_person_id: req.from_person_id.clone(),
            to_person_id: req.to_person_id.clone(),
            relation_type,
            inverse_relation_type,
            source_kind: MutationSourceKind::Manual,
            source_user_id: Some(auth.user_id),
            source_note: req.source_note,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    let relations = dictionary::list_resolved_relations_for_person(
        &state.db,
        &workspace.id,
        &req.from_person_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(relations))
}

async fn patch_workspace_relation_pair(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, relation_id)): Path<(String, String)>,
    Json(req): Json<UpsertRelationPairRequest>,
) -> Result<Json<Vec<DictionaryResolvedRelationRow>>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    let existing = dictionary::find_relation_by_id(&state.db, &relation_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("relationship not found".into()))?;
    if existing.workspace_id != workspace.id {
        return Err(ApiError::Forbidden("relationship is not in this workspace".into()).into());
    }

    let _ =
        dictionary::delete_relation_group(&state.db, &workspace.id, &existing.relation_group_key)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;

    upsert_workspace_relation_pair(State(state), auth, Path(workspace_id), Json(req)).await
}

async fn delete_workspace_relation_pair(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, relation_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let workspace = ensure_workspace_write_access(&state, &auth, &workspace_id).await?;
    let existing = dictionary::find_relation_by_id(&state.db, &relation_id)
        .await
        .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("relationship not found".into()))?;
    if existing.workspace_id != workspace.id {
        return Err(ApiError::Forbidden("relationship is not in this workspace".into()).into());
    }
    let deleted =
        dictionary::delete_relation_group(&state.db, &workspace.id, &existing.relation_group_key)
            .await
            .map_err(|e| ApiError::Internal(format!("dictionary db error: {e}")))?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_fact_key, normalize_person_name, normalize_relation_type,
        normalize_workspace_slug, root_kind_to_workspace_kind, router,
    };
    use crate::state::RustyVaultRuntimeState;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn person_name_normalization_collapses_whitespace() {
        let name = normalize_person_name("  Mary   Anne  ").expect("valid name");
        assert_eq!(name, "Mary Anne");
    }

    #[test]
    fn relation_type_normalization_rewrites_dashes() {
        let relation = normalize_relation_type("co-worker").expect("valid relation");
        assert_eq!(relation, "co_worker");
    }

    #[test]
    fn fact_key_rejects_uppercase_and_spaces() {
        assert!(normalize_fact_key("birthday date").is_err());
        assert_eq!(
            normalize_fact_key("Birthday").expect("valid fact key"),
            "birthday"
        );
    }

    #[test]
    fn workspace_slug_uses_title_when_slug_missing() {
        let slug = normalize_workspace_slug("Dublin Family", None).expect("valid slug");
        assert_eq!(slug, "dublin-family");
    }

    #[test]
    fn root_kind_mapping_matches_seeded_workspace_kinds() {
        assert_eq!(
            root_kind_to_workspace_kind("family").map(|kind| kind.as_str()),
            Some("family_shared")
        );
        assert_eq!(
            root_kind_to_workspace_kind("friends").map(|kind| kind.as_str()),
            Some("friends_private")
        );
        assert_eq!(
            root_kind_to_workspace_kind("work").map(|kind| kind.as_str()),
            Some("work_private")
        );
        assert!(root_kind_to_workspace_kind("unknown").is_none());
    }

    fn test_state() -> crate::state::AppState {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@127.0.0.1/rustfin_test")
            .expect("lazy postgres pool");
        let tc_config = rustfin_transcoder::TranscoderConfig {
            transcode_dir: std::env::temp_dir().join("rustyfin-dictionary-route-tests"),
            max_concurrent: 1,
            ..Default::default()
        };
        let ffmpeg_path = tc_config.ffmpeg_path.clone();
        let ffprobe_path = tc_config.ffprobe_path.clone();
        let transcoder = Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));
        let (events_tx, _) = tokio::sync::broadcast::channel(8);

        crate::state::AppState {
            db: pool,
            rustyvault: RustyVaultRuntimeState::available(),
            jwt_secret: "test-secret".to_string(),
            http: reqwest::Client::builder().build().expect("http client"),
            runtime_metrics: crate::runtime_metrics::RuntimeMetrics::new(),
            tmdb_agent_url: "http://127.0.0.1:8100".to_string(),
            tmdb_agent_token: None,
            youtube_agent_url: "http://127.0.0.1:8101".to_string(),
            youtube_agent_token: None,
            transcription_agent_url: "http://127.0.0.1:8102".to_string(),
            transcription_agent_token: None,
            servers_agent_url: None,
            servers_agent_token: None,
            model_dir: Arc::new(tokio::sync::RwLock::new(
                std::env::temp_dir().join("rustyfin-dictionary-models"),
            )),
            engine: Arc::new(tokio::sync::Mutex::new(crate::ai::EngineState::default())),
            transcoder,
            ffmpeg_path,
            ffprobe_path,
            transcoder_hw_accel: None,
            transcoder_hw_accel_required: false,
            cache_dir: std::env::temp_dir().join("rustyfin-dictionary-cache"),
            watch_party_audio_dir: std::env::temp_dir().join("rustyfin-dictionary-audio"),
            events: events_tx,
            watch_party: Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
            channel_manager: Arc::new(crate::channels::manager::ChannelManager::new()),
        }
    }

    #[tokio::test]
    async fn dictionary_routes_require_authentication() {
        let app = router().with_state(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/workspaces")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
