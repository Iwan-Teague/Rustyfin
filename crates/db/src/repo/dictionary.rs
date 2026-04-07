use crate::repo::users;
use crate::{DbError, DbPool};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

fn now_ts() -> i64 {
    Utc::now().timestamp()
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn slug_tail_from_user_id(user_id: &str) -> String {
    user_id.chars().take(8).collect()
}

fn normalize_name_for_search(
    display_name: &str,
    canonical_name: &str,
    summary: Option<&str>,
) -> String {
    [
        display_name.trim(),
        canonical_name.trim(),
        summary.unwrap_or("").trim(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

fn summarize_markdown(body: &str) -> String {
    const MAX_LEN: usize = 240;
    let collapsed = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.chars().count() <= MAX_LEN {
        collapsed
    } else {
        format!("{}...", collapsed.chars().take(MAX_LEN).collect::<String>())
    }
}

fn relation_pair_key(from_person_id: &str, to_person_id: &str) -> String {
    if from_person_id <= to_person_id {
        format!("{from_person_id}:{to_person_id}")
    } else {
        format!("{to_person_id}:{from_person_id}")
    }
}

fn relation_group_key(
    from_person_id: &str,
    to_person_id: &str,
    relation_type: &str,
    inverse_relation_type: &str,
) -> String {
    let pair_key = relation_pair_key(from_person_id, to_person_id);
    let (left, right) = if relation_type <= inverse_relation_type {
        (relation_type, inverse_relation_type)
    } else {
        (inverse_relation_type, relation_type)
    };
    format!("{pair_key}|{left}|{right}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    FamilyShared,
    FriendsPrivate,
    WorkPrivate,
    Custom,
}

impl WorkspaceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FamilyShared => "family_shared",
            Self::FriendsPrivate => "friends_private",
            Self::WorkPrivate => "work_private",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "family_shared" => Some(Self::FamilyShared),
            "friends_private" => Some(Self::FriendsPrivate),
            "work_private" => Some(Self::WorkPrivate),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRole {
    Owner,
    Editor,
    Viewer,
}

impl WorkspaceRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Editor => "editor",
            Self::Viewer => "viewer",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "editor" => Some(Self::Editor),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }

    pub const fn can_write(self) -> bool {
        matches!(self, Self::Owner | Self::Editor)
    }

    pub const fn can_manage(self) -> bool {
        matches!(self, Self::Owner)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeNodeKind {
    Root,
    Group,
    Person,
    Shortcut,
}

impl TreeNodeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Group => "group",
            Self::Person => "person",
            Self::Shortcut => "shortcut",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    Person,
    TreeNode,
    Workspace,
}

impl SubjectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::TreeNode => "tree_node",
            Self::Workspace => "workspace",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactValueType {
    Text,
    Int,
    Bool,
    Date,
    Json,
}

impl FactValueType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Date => "date",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationSourceKind {
    Manual,
    Ai,
    Import,
    System,
}

impl MutationSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Ai => "ai",
            Self::Import => "import",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionarySpaceRow {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub owner_user_id: String,
    pub is_default: bool,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type DictionarySpaceTuple = (
    String,
    String,
    String,
    Option<String>,
    String,
    bool,
    i64,
    i64,
);

fn map_dictionary_space_row(
    (id, slug, title, description, owner_user_id, is_default, created_ts, updated_ts): DictionarySpaceTuple,
) -> DictionarySpaceRow {
    DictionarySpaceRow {
        id,
        slug,
        title,
        description,
        owner_user_id,
        is_default,
        created_ts,
        updated_ts,
    }
}

const DICTIONARY_SPACE_COLUMNS: &str =
    "id, slug, title, description, owner_user_id, is_default, created_ts, updated_ts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryWorkspaceRow {
    pub id: String,
    pub space_id: String,
    pub slug: String,
    pub title: String,
    pub workspace_kind: String,
    pub owner_user_id: Option<String>,
    pub is_system_seeded: bool,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type DictionaryWorkspaceTuple = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    bool,
    i64,
    i64,
);

fn map_dictionary_workspace_row(
    (
        id,
        space_id,
        slug,
        title,
        workspace_kind,
        owner_user_id,
        is_system_seeded,
        created_ts,
        updated_ts,
    ): DictionaryWorkspaceTuple,
) -> DictionaryWorkspaceRow {
    DictionaryWorkspaceRow {
        id,
        space_id,
        slug,
        title,
        workspace_kind,
        owner_user_id,
        is_system_seeded,
        created_ts,
        updated_ts,
    }
}

const DICTIONARY_WORKSPACE_COLUMNS: &str = "id, space_id, slug, title, workspace_kind, owner_user_id, is_system_seeded, created_ts, updated_ts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryWorkspaceMemberRow {
    pub workspace_id: String,
    pub user_id: String,
    pub role: String,
    pub added_by_user_id: Option<String>,
    pub created_ts: i64,
}

type DictionaryWorkspaceMemberTuple = (String, String, String, Option<String>, i64);

fn map_dictionary_workspace_member_row(
    (workspace_id, user_id, role, added_by_user_id, created_ts): DictionaryWorkspaceMemberTuple,
) -> DictionaryWorkspaceMemberRow {
    DictionaryWorkspaceMemberRow {
        workspace_id,
        user_id,
        role,
        added_by_user_id,
        created_ts,
    }
}

const DICTIONARY_WORKSPACE_MEMBER_COLUMNS: &str =
    "workspace_id, user_id, role, added_by_user_id, created_ts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryPersonRow {
    pub id: String,
    pub space_id: String,
    pub canonical_name: String,
    pub display_name: String,
    pub summary: Option<String>,
    pub primary_photo_path: Option<String>,
    pub primary_photo_content_type: Option<String>,
    pub search_text: String,
    pub created_by_user_id: String,
    pub archived_ts: Option<i64>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type DictionaryPersonTuple = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<i64>,
    i64,
    i64,
);

fn map_dictionary_person_row(
    (
        id,
        space_id,
        canonical_name,
        display_name,
        summary,
        primary_photo_path,
        primary_photo_content_type,
        search_text,
        created_by_user_id,
        archived_ts,
        created_ts,
        updated_ts,
    ): DictionaryPersonTuple,
) -> DictionaryPersonRow {
    DictionaryPersonRow {
        id,
        space_id,
        canonical_name,
        display_name,
        summary,
        primary_photo_path,
        primary_photo_content_type,
        search_text,
        created_by_user_id,
        archived_ts,
        created_ts,
        updated_ts,
    }
}

const DICTIONARY_PERSON_COLUMNS: &str = "id, space_id, canonical_name, display_name, summary, \
    primary_photo_path, primary_photo_content_type, search_text, created_by_user_id, archived_ts, created_ts, updated_ts";

fn dictionary_person_row_from_pg_row(
    row: &sqlx::postgres::PgRow,
) -> Result<DictionaryPersonRow, sqlx::Error> {
    Ok(DictionaryPersonRow {
        id: row.try_get("id")?,
        space_id: row.try_get("space_id")?,
        canonical_name: row.try_get("canonical_name")?,
        display_name: row.try_get("display_name")?,
        summary: row.try_get("summary")?,
        primary_photo_path: row.try_get("primary_photo_path")?,
        primary_photo_content_type: row.try_get("primary_photo_content_type")?,
        search_text: row.try_get("search_text")?,
        created_by_user_id: row.try_get("created_by_user_id")?,
        archived_ts: row.try_get("archived_ts")?,
        created_ts: row.try_get("created_ts")?,
        updated_ts: row.try_get("updated_ts")?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryPersonAliasRow {
    pub id: String,
    pub person_id: String,
    pub alias: String,
    pub alias_kind: String,
    pub created_by_user_id: String,
    pub created_ts: i64,
}

type DictionaryPersonAliasTuple = (String, String, String, String, String, i64);

fn map_dictionary_person_alias_row(
    (id, person_id, alias, alias_kind, created_by_user_id, created_ts): DictionaryPersonAliasTuple,
) -> DictionaryPersonAliasRow {
    DictionaryPersonAliasRow {
        id,
        person_id,
        alias,
        alias_kind,
        created_by_user_id,
        created_ts,
    }
}

const DICTIONARY_PERSON_ALIAS_COLUMNS: &str =
    "id, person_id, alias, alias_kind, created_by_user_id, created_ts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryTreeNodeRow {
    pub id: String,
    pub workspace_id: String,
    pub parent_node_id: Option<String>,
    pub node_kind: String,
    pub title: String,
    pub person_id: Option<String>,
    pub sort_order: i32,
    pub icon_name: Option<String>,
    pub note: Option<String>,
    pub is_system_seeded: bool,
    pub created_by_user_id: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type DictionaryTreeNodeTuple = (
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    i32,
    Option<String>,
    Option<String>,
    bool,
    String,
    i64,
    i64,
);

fn map_dictionary_tree_node_row(
    (
        id,
        workspace_id,
        parent_node_id,
        node_kind,
        title,
        person_id,
        sort_order,
        icon_name,
        note,
        is_system_seeded,
        created_by_user_id,
        created_ts,
        updated_ts,
    ): DictionaryTreeNodeTuple,
) -> DictionaryTreeNodeRow {
    DictionaryTreeNodeRow {
        id,
        workspace_id,
        parent_node_id,
        node_kind,
        title,
        person_id,
        sort_order,
        icon_name,
        note,
        is_system_seeded,
        created_by_user_id,
        created_ts,
        updated_ts,
    }
}

const DICTIONARY_TREE_NODE_COLUMNS: &str = "id, workspace_id, parent_node_id, node_kind, title, \
    person_id, sort_order, icon_name, note, is_system_seeded, created_by_user_id, created_ts, updated_ts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryRelationRow {
    pub id: String,
    pub workspace_id: String,
    pub from_person_id: String,
    pub to_person_id: String,
    pub relation_type: String,
    pub pair_key: String,
    pub relation_group_key: String,
    pub direction: String,
    pub status: String,
    pub source_kind: String,
    pub source_user_id: Option<String>,
    pub source_note: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type DictionaryRelationTuple = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

fn map_dictionary_relation_row(
    (
        id,
        workspace_id,
        from_person_id,
        to_person_id,
        relation_type,
        pair_key,
        relation_group_key,
        direction,
        status,
        source_kind,
        source_user_id,
        source_note,
        created_ts,
        updated_ts,
    ): DictionaryRelationTuple,
) -> DictionaryRelationRow {
    DictionaryRelationRow {
        id,
        workspace_id,
        from_person_id,
        to_person_id,
        relation_type,
        pair_key,
        relation_group_key,
        direction,
        status,
        source_kind,
        source_user_id,
        source_note,
        created_ts,
        updated_ts,
    }
}

const DICTIONARY_RELATION_COLUMNS: &str = "id, workspace_id, from_person_id, to_person_id, relation_type, \
    pair_key, relation_group_key, direction, status, source_kind, source_user_id, source_note, created_ts, updated_ts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryFactRow {
    pub id: String,
    pub workspace_id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub fact_key: String,
    pub value_type: String,
    pub value_text: Option<String>,
    pub value_int: Option<i64>,
    pub value_bool: Option<bool>,
    pub value_date: Option<String>,
    pub value_json: Option<Value>,
    pub unit: Option<String>,
    pub confidence: Option<f64>,
    pub status: String,
    pub source_kind: String,
    pub source_user_id: Option<String>,
    pub source_note: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

const DICTIONARY_FACT_COLUMNS: &str = "id, workspace_id, subject_kind, subject_id, fact_key, value_type, \
    value_text, value_int, value_bool, value_date, value_json, unit, confidence, status, \
    source_kind, source_user_id, source_note, created_ts, updated_ts";

fn dictionary_fact_row_from_pg_row(
    row: &sqlx::postgres::PgRow,
) -> Result<DictionaryFactRow, sqlx::Error> {
    Ok(DictionaryFactRow {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        subject_kind: row.try_get("subject_kind")?,
        subject_id: row.try_get("subject_id")?,
        fact_key: row.try_get("fact_key")?,
        value_type: row.try_get("value_type")?,
        value_text: row.try_get("value_text")?,
        value_int: row.try_get("value_int")?,
        value_bool: row.try_get("value_bool")?,
        value_date: row.try_get("value_date")?,
        value_json: row.try_get("value_json")?,
        unit: row.try_get("unit")?,
        confidence: row.try_get("confidence")?,
        status: row.try_get("status")?,
        source_kind: row.try_get("source_kind")?,
        source_user_id: row.try_get("source_user_id")?,
        source_note: row.try_get("source_note")?,
        created_ts: row.try_get("created_ts")?,
        updated_ts: row.try_get("updated_ts")?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryDocumentRow {
    pub id: String,
    pub workspace_id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub title: String,
    pub markdown_body: String,
    pub summary: String,
    pub last_edited_by_user_id: Option<String>,
    pub last_edited_source_kind: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type DictionaryDocumentTuple = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    i64,
    i64,
);

fn map_dictionary_document_row(
    (
        id,
        workspace_id,
        subject_kind,
        subject_id,
        title,
        markdown_body,
        summary,
        last_edited_by_user_id,
        last_edited_source_kind,
        created_ts,
        updated_ts,
    ): DictionaryDocumentTuple,
) -> DictionaryDocumentRow {
    DictionaryDocumentRow {
        id,
        workspace_id,
        subject_kind,
        subject_id,
        title,
        markdown_body,
        summary,
        last_edited_by_user_id,
        last_edited_source_kind,
        created_ts,
        updated_ts,
    }
}

const DICTIONARY_DOCUMENT_COLUMNS: &str = "id, workspace_id, subject_kind, subject_id, title, markdown_body, summary, \
    last_edited_by_user_id, last_edited_source_kind, created_ts, updated_ts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryAccountLinkRow {
    pub user_id: String,
    pub space_id: String,
    pub person_id: String,
    pub family_workspace_id: Option<String>,
    pub friends_workspace_id: Option<String>,
    pub work_workspace_id: Option<String>,
    pub created_by_user_id: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type DictionaryAccountLinkTuple = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
);

fn map_dictionary_account_link_row(
    (
        user_id,
        space_id,
        person_id,
        family_workspace_id,
        friends_workspace_id,
        work_workspace_id,
        created_by_user_id,
        created_ts,
        updated_ts,
    ): DictionaryAccountLinkTuple,
) -> DictionaryAccountLinkRow {
    DictionaryAccountLinkRow {
        user_id,
        space_id,
        person_id,
        family_workspace_id,
        friends_workspace_id,
        work_workspace_id,
        created_by_user_id,
        created_ts,
        updated_ts,
    }
}

const DICTIONARY_ACCOUNT_LINK_COLUMNS: &str = "user_id, space_id, person_id, family_workspace_id, \
    friends_workspace_id, work_workspace_id, created_by_user_id, created_ts, updated_ts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapWorkspacesResult {
    pub family_workspace: DictionaryWorkspaceRow,
    pub friends_workspace: DictionaryWorkspaceRow,
    pub work_workspace: DictionaryWorkspaceRow,
}

#[derive(Debug, Clone)]
pub struct CreateWorkspaceInput {
    pub space_id: String,
    pub slug: String,
    pub title: String,
    pub workspace_kind: WorkspaceKind,
    pub owner_user_id: Option<String>,
    pub is_system_seeded: bool,
}

#[derive(Debug, Clone)]
pub struct CreateTreeNodeInput {
    pub workspace_id: String,
    pub parent_node_id: Option<String>,
    pub node_kind: TreeNodeKind,
    pub title: String,
    pub person_id: Option<String>,
    pub sort_order: i32,
    pub icon_name: Option<String>,
    pub note: Option<String>,
    pub is_system_seeded: bool,
    pub created_by_user_id: String,
}

#[derive(Debug, Clone)]
pub struct CreatePersonInput {
    pub space_id: String,
    pub canonical_name: String,
    pub display_name: String,
    pub summary: Option<String>,
    pub created_by_user_id: String,
}

#[derive(Debug, Clone)]
pub struct UpdatePersonInput {
    pub person_id: String,
    pub display_name: String,
    pub canonical_name: String,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchVisiblePeopleParams {
    pub workspace_id: String,
    pub query: String,
    pub limit: i64,
}

#[derive(Debug, Clone)]
pub struct RelationPairInput {
    pub workspace_id: String,
    pub from_person_id: String,
    pub to_person_id: String,
    pub relation_type: String,
    pub inverse_relation_type: String,
    pub source_kind: MutationSourceKind,
    pub source_user_id: Option<String>,
    pub source_note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpsertFactInput {
    pub workspace_id: String,
    pub subject_kind: SubjectKind,
    pub subject_id: String,
    pub fact_key: String,
    pub value_type: FactValueType,
    pub value_text: Option<String>,
    pub value_int: Option<i64>,
    pub value_bool: Option<bool>,
    pub value_date: Option<String>,
    pub value_json: Option<Value>,
    pub unit: Option<String>,
    pub confidence: Option<f64>,
    pub status: String,
    pub source_kind: MutationSourceKind,
    pub source_user_id: Option<String>,
    pub source_note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SaveDocumentInput {
    pub workspace_id: String,
    pub subject_kind: SubjectKind,
    pub subject_id: String,
    pub title: String,
    pub markdown_body: String,
    pub edited_by_user_id: Option<String>,
    pub edit_source_kind: MutationSourceKind,
    pub edit_note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpsertAccountLinkInput {
    pub user_id: String,
    pub space_id: String,
    pub person_id: String,
    pub family_workspace_id: Option<String>,
    pub friends_workspace_id: Option<String>,
    pub work_workspace_id: Option<String>,
    pub created_by_user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryResolvedRelationRow {
    pub relation_id: String,
    pub relation_group_key: String,
    pub relation_type: String,
    pub direction: String,
    pub other_person: DictionaryPersonRow,
}

pub async fn get_default_space_for_owner(
    pool: &DbPool,
    owner_user_id: &str,
) -> Result<Option<DictionarySpaceRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_SPACE_COLUMNS}
         FROM dictionary_space
         WHERE owner_user_id = $1 AND is_default = TRUE
         LIMIT 1"
    );
    let row: Option<DictionarySpaceTuple> = sqlx::query_as(&sql)
        .bind(owner_user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_space_row))
}

pub async fn ensure_default_household_space(
    pool: &DbPool,
    owner_user_id: &str,
) -> Result<DictionarySpaceRow, DbError> {
    if let Some(existing) = get_default_space_for_owner(pool, owner_user_id).await? {
        return Ok(existing);
    }

    let now = now_ts();
    let id = new_id();
    let sql = format!(
        "INSERT INTO dictionary_space ({DICTIONARY_SPACE_COLUMNS})
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING {DICTIONARY_SPACE_COLUMNS}"
    );
    let row: DictionarySpaceTuple = sqlx::query_as(&sql)
        .bind(&id)
        .bind("household")
        .bind("Household")
        .bind(Some("Default Rustyfin Human Dictionary space".to_string()))
        .bind(owner_user_id)
        .bind(true)
        .bind(now)
        .bind(now)
        .fetch_one(pool)
        .await?;
    Ok(map_dictionary_space_row(row))
}

pub async fn find_workspace_by_id(
    pool: &DbPool,
    workspace_id: &str,
) -> Result<Option<DictionaryWorkspaceRow>, sqlx::Error> {
    let sql =
        format!("SELECT {DICTIONARY_WORKSPACE_COLUMNS} FROM dictionary_workspace WHERE id = $1");
    let row: Option<DictionaryWorkspaceTuple> = sqlx::query_as(&sql)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_workspace_row))
}

pub async fn find_workspace_by_slug(
    pool: &DbPool,
    space_id: &str,
    slug: &str,
) -> Result<Option<DictionaryWorkspaceRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_WORKSPACE_COLUMNS}
         FROM dictionary_workspace
         WHERE space_id = $1 AND slug = $2"
    );
    let row: Option<DictionaryWorkspaceTuple> = sqlx::query_as(&sql)
        .bind(space_id)
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_workspace_row))
}

pub async fn create_workspace(
    pool: &DbPool,
    input: &CreateWorkspaceInput,
) -> Result<DictionaryWorkspaceRow, DbError> {
    let now = now_ts();
    let id = new_id();
    let sql = format!(
        "INSERT INTO dictionary_workspace ({DICTIONARY_WORKSPACE_COLUMNS})
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING {DICTIONARY_WORKSPACE_COLUMNS}"
    );
    let row: DictionaryWorkspaceTuple = sqlx::query_as(&sql)
        .bind(&id)
        .bind(&input.space_id)
        .bind(&input.slug)
        .bind(&input.title)
        .bind(input.workspace_kind.as_str())
        .bind(&input.owner_user_id)
        .bind(input.is_system_seeded)
        .bind(now)
        .bind(now)
        .fetch_one(pool)
        .await?;
    Ok(map_dictionary_workspace_row(row))
}

pub async fn ensure_workspace_member(
    pool: &DbPool,
    workspace_id: &str,
    user_id: &str,
    role: WorkspaceRole,
    added_by_user_id: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO dictionary_workspace_member (workspace_id, user_id, role, added_by_user_id, created_ts)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (workspace_id, user_id)
         DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role.as_str())
    .bind(added_by_user_id)
    .bind(now_ts())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_workspace_member(
    pool: &DbPool,
    workspace_id: &str,
    user_id: &str,
) -> Result<Option<DictionaryWorkspaceMemberRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_WORKSPACE_MEMBER_COLUMNS}
         FROM dictionary_workspace_member
         WHERE workspace_id = $1 AND user_id = $2"
    );
    let row: Option<DictionaryWorkspaceMemberTuple> = sqlx::query_as(&sql)
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_workspace_member_row))
}

pub async fn user_can_access_workspace(
    pool: &DbPool,
    workspace_id: &str,
    user_id: &str,
) -> Result<bool, sqlx::Error> {
    Ok(get_workspace_member(pool, workspace_id, user_id)
        .await?
        .is_some())
}

pub async fn list_visible_workspaces(
    pool: &DbPool,
    user_id: &str,
) -> Result<Vec<DictionaryWorkspaceRow>, sqlx::Error> {
    let sql = format!(
        "SELECT w.{cols}
         FROM dictionary_workspace w
         INNER JOIN dictionary_workspace_member m ON m.workspace_id = w.id
         WHERE m.user_id = $1
         ORDER BY
             CASE w.workspace_kind
                 WHEN 'family_shared' THEN 0
                 WHEN 'friends_private' THEN 1
                 WHEN 'work_private' THEN 2
                 ELSE 3
             END,
             lower(w.title),
             w.created_ts",
        cols = DICTIONARY_WORKSPACE_COLUMNS
    );
    let rows: Vec<DictionaryWorkspaceTuple> =
        sqlx::query_as(&sql).bind(user_id).fetch_all(pool).await?;
    Ok(rows.into_iter().map(map_dictionary_workspace_row).collect())
}

pub async fn ensure_default_workspaces_for_user(
    pool: &DbPool,
    space_id: &str,
    user_id: &str,
) -> Result<BootstrapWorkspacesResult, DbError> {
    let user = users::find_by_id(pool, user_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    let family = match find_workspace_by_slug(pool, space_id, "family-shared").await? {
        Some(existing) => existing,
        None => {
            create_workspace(
                pool,
                &CreateWorkspaceInput {
                    space_id: space_id.to_string(),
                    slug: "family-shared".to_string(),
                    title: "Family".to_string(),
                    workspace_kind: WorkspaceKind::FamilyShared,
                    owner_user_id: Some(user_id.to_string()),
                    is_system_seeded: true,
                },
            )
            .await?
        }
    };
    ensure_workspace_member(
        pool,
        &family.id,
        user_id,
        WorkspaceRole::Owner,
        Some(user_id),
    )
    .await?;

    let friends_slug = format!("friends-{}", slug_tail_from_user_id(user_id));
    let friends = match find_workspace_by_slug(pool, space_id, &friends_slug).await? {
        Some(existing) => existing,
        None => {
            create_workspace(
                pool,
                &CreateWorkspaceInput {
                    space_id: space_id.to_string(),
                    slug: friends_slug,
                    title: format!("{}'s Friends", user.display_name),
                    workspace_kind: WorkspaceKind::FriendsPrivate,
                    owner_user_id: Some(user_id.to_string()),
                    is_system_seeded: true,
                },
            )
            .await?
        }
    };
    ensure_workspace_member(
        pool,
        &friends.id,
        user_id,
        WorkspaceRole::Owner,
        Some(user_id),
    )
    .await?;

    let work_slug = format!("work-{}", slug_tail_from_user_id(user_id));
    let work = match find_workspace_by_slug(pool, space_id, &work_slug).await? {
        Some(existing) => existing,
        None => {
            create_workspace(
                pool,
                &CreateWorkspaceInput {
                    space_id: space_id.to_string(),
                    slug: work_slug,
                    title: format!("{}'s Work", user.display_name),
                    workspace_kind: WorkspaceKind::WorkPrivate,
                    owner_user_id: Some(user_id.to_string()),
                    is_system_seeded: true,
                },
            )
            .await?
        }
    };
    ensure_workspace_member(pool, &work.id, user_id, WorkspaceRole::Owner, Some(user_id)).await?;

    Ok(BootstrapWorkspacesResult {
        family_workspace: family,
        friends_workspace: friends,
        work_workspace: work,
    })
}

pub async fn find_tree_node_by_id(
    pool: &DbPool,
    node_id: &str,
) -> Result<Option<DictionaryTreeNodeRow>, sqlx::Error> {
    let sql =
        format!("SELECT {DICTIONARY_TREE_NODE_COLUMNS} FROM dictionary_tree_node WHERE id = $1");
    let row: Option<DictionaryTreeNodeTuple> = sqlx::query_as(&sql)
        .bind(node_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_tree_node_row))
}

pub async fn list_tree_nodes(
    pool: &DbPool,
    workspace_id: &str,
) -> Result<Vec<DictionaryTreeNodeRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_TREE_NODE_COLUMNS}
         FROM dictionary_tree_node
         WHERE workspace_id = $1
         ORDER BY parent_node_id NULLS FIRST, sort_order, created_ts, id"
    );
    let rows: Vec<DictionaryTreeNodeTuple> = sqlx::query_as(&sql)
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_dictionary_tree_node_row).collect())
}

pub async fn list_tree_nodes_for_person(
    pool: &DbPool,
    workspace_id: &str,
    person_id: &str,
) -> Result<Vec<DictionaryTreeNodeRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_TREE_NODE_COLUMNS}
         FROM dictionary_tree_node
         WHERE workspace_id = $1 AND person_id = $2
         ORDER BY sort_order, created_ts, id"
    );
    let rows: Vec<DictionaryTreeNodeTuple> = sqlx::query_as(&sql)
        .bind(workspace_id)
        .bind(person_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_dictionary_tree_node_row).collect())
}

pub async fn create_tree_node(
    pool: &DbPool,
    input: &CreateTreeNodeInput,
) -> Result<DictionaryTreeNodeRow, DbError> {
    let now = now_ts();
    let id = new_id();
    let sql = format!(
        "INSERT INTO dictionary_tree_node ({DICTIONARY_TREE_NODE_COLUMNS})
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         RETURNING {DICTIONARY_TREE_NODE_COLUMNS}"
    );
    let row: DictionaryTreeNodeTuple = sqlx::query_as(&sql)
        .bind(&id)
        .bind(&input.workspace_id)
        .bind(&input.parent_node_id)
        .bind(input.node_kind.as_str())
        .bind(&input.title)
        .bind(&input.person_id)
        .bind(input.sort_order)
        .bind(&input.icon_name)
        .bind(&input.note)
        .bind(input.is_system_seeded)
        .bind(&input.created_by_user_id)
        .bind(now)
        .bind(now)
        .fetch_one(pool)
        .await?;
    Ok(map_dictionary_tree_node_row(row))
}

pub async fn find_person_by_id(
    pool: &DbPool,
    person_id: &str,
) -> Result<Option<DictionaryPersonRow>, sqlx::Error> {
    let sql = format!("SELECT {DICTIONARY_PERSON_COLUMNS} FROM dictionary_person WHERE id = $1");
    let row: Option<DictionaryPersonTuple> = sqlx::query_as(&sql)
        .bind(person_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_person_row))
}

pub async fn create_person(
    pool: &DbPool,
    input: &CreatePersonInput,
) -> Result<DictionaryPersonRow, DbError> {
    let now = now_ts();
    let id = new_id();
    let search_text = normalize_name_for_search(
        &input.display_name,
        &input.canonical_name,
        input.summary.as_deref(),
    );
    let sql = format!(
        "INSERT INTO dictionary_person ({DICTIONARY_PERSON_COLUMNS})
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING {DICTIONARY_PERSON_COLUMNS}"
    );
    let row: DictionaryPersonTuple = sqlx::query_as(&sql)
        .bind(&id)
        .bind(&input.space_id)
        .bind(&input.canonical_name)
        .bind(&input.display_name)
        .bind(&input.summary)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(&search_text)
        .bind(&input.created_by_user_id)
        .bind(Option::<i64>::None)
        .bind(now)
        .bind(now)
        .fetch_one(pool)
        .await?;
    Ok(map_dictionary_person_row(row))
}

pub async fn update_person(
    pool: &DbPool,
    input: &UpdatePersonInput,
) -> Result<Option<DictionaryPersonRow>, DbError> {
    let now = now_ts();
    let search_text = normalize_name_for_search(
        &input.display_name,
        &input.canonical_name,
        input.summary.as_deref(),
    );
    let sql = format!(
        "UPDATE dictionary_person
         SET display_name = $1,
             canonical_name = $2,
             summary = $3,
             search_text = $4,
             updated_ts = $5
         WHERE id = $6
         RETURNING {DICTIONARY_PERSON_COLUMNS}"
    );
    let row: Option<DictionaryPersonTuple> = sqlx::query_as(&sql)
        .bind(&input.display_name)
        .bind(&input.canonical_name)
        .bind(&input.summary)
        .bind(&search_text)
        .bind(now)
        .bind(&input.person_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_person_row))
}

pub async fn add_person_alias(
    pool: &DbPool,
    person_id: &str,
    alias: &str,
    alias_kind: &str,
    created_by_user_id: &str,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO dictionary_person_alias (id, person_id, alias, alias_kind, created_by_user_id, created_ts)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (person_id, lower(alias)) DO NOTHING",
    )
    .bind(new_id())
    .bind(person_id)
    .bind(alias)
    .bind(alias_kind)
    .bind(created_by_user_id)
    .bind(now_ts())
    .execute(pool)
    .await?;

    sqlx::query(
        "UPDATE dictionary_person
         SET search_text = trim(both ' ' from search_text || ' ' || $1),
             updated_ts = $2
         WHERE id = $3",
    )
    .bind(alias)
    .bind(now_ts())
    .bind(person_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_person_aliases(
    pool: &DbPool,
    person_id: &str,
) -> Result<Vec<DictionaryPersonAliasRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_PERSON_ALIAS_COLUMNS}
         FROM dictionary_person_alias
         WHERE person_id = $1
         ORDER BY lower(alias), created_ts, id"
    );
    let rows: Vec<DictionaryPersonAliasTuple> =
        sqlx::query_as(&sql).bind(person_id).fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(map_dictionary_person_alias_row)
        .collect())
}

pub async fn search_visible_people(
    pool: &DbPool,
    params: &SearchVisiblePeopleParams,
) -> Result<Vec<DictionaryPersonRow>, sqlx::Error> {
    let trimmed = params.query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let like = format!("%{}%", trimmed.to_ascii_lowercase());
    let sql = format!(
        "SELECT DISTINCT p.{cols}
         FROM dictionary_person p
         INNER JOIN dictionary_tree_node n ON n.person_id = p.id AND n.workspace_id = $1
         LEFT JOIN dictionary_person_alias a ON a.person_id = p.id
         WHERE p.archived_ts IS NULL
           AND (
                lower(p.display_name) LIKE $2
                OR lower(p.canonical_name) LIKE $2
                OR lower(coalesce(p.summary, '')) LIKE $2
                OR lower(coalesce(a.alias, '')) LIKE $2
                OR to_tsvector('simple', p.search_text) @@ plainto_tsquery('simple', $3)
           )
         ORDER BY lower(p.display_name), p.created_ts
         LIMIT $4",
        cols = DICTIONARY_PERSON_COLUMNS
    );
    let rows: Vec<DictionaryPersonTuple> = sqlx::query_as(&sql)
        .bind(&params.workspace_id)
        .bind(&like)
        .bind(trimmed)
        .bind(params.limit.clamp(1, 50))
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_dictionary_person_row).collect())
}

pub async fn list_visible_people(
    pool: &DbPool,
    workspace_id: &str,
    limit: i64,
) -> Result<Vec<DictionaryPersonRow>, sqlx::Error> {
    let sql = format!(
        "SELECT DISTINCT p.{cols}
         FROM dictionary_person p
         INNER JOIN dictionary_tree_node n ON n.person_id = p.id
         WHERE n.workspace_id = $1
           AND p.archived_ts IS NULL
         ORDER BY lower(p.display_name), p.created_ts
         LIMIT $2",
        cols = DICTIONARY_PERSON_COLUMNS
    );
    let rows: Vec<DictionaryPersonTuple> = sqlx::query_as(&sql)
        .bind(workspace_id)
        .bind(limit.clamp(1, 500))
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_dictionary_person_row).collect())
}

async fn upsert_single_relation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: &str,
    from_person_id: &str,
    to_person_id: &str,
    relation_type: &str,
    pair_key: &str,
    relation_group_key: &str,
    direction: &str,
    source_kind: MutationSourceKind,
    source_user_id: Option<&str>,
    source_note: Option<&str>,
) -> Result<DictionaryRelationRow, sqlx::Error> {
    let now = now_ts();
    let id = new_id();
    let sql = format!(
        "INSERT INTO dictionary_relation ({DICTIONARY_RELATION_COLUMNS})
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'confirmed', $9, $10, $11, $12, $13)
         ON CONFLICT (workspace_id, from_person_id, to_person_id, relation_type, direction)
         DO UPDATE SET
             pair_key = EXCLUDED.pair_key,
             relation_group_key = EXCLUDED.relation_group_key,
             status = 'confirmed',
             source_kind = EXCLUDED.source_kind,
             source_user_id = EXCLUDED.source_user_id,
             source_note = EXCLUDED.source_note,
             updated_ts = EXCLUDED.updated_ts
         RETURNING {DICTIONARY_RELATION_COLUMNS}"
    );
    let row: DictionaryRelationTuple = sqlx::query_as(&sql)
        .bind(&id)
        .bind(workspace_id)
        .bind(from_person_id)
        .bind(to_person_id)
        .bind(relation_type)
        .bind(pair_key)
        .bind(relation_group_key)
        .bind(direction)
        .bind(source_kind.as_str())
        .bind(source_user_id)
        .bind(source_note)
        .bind(now)
        .bind(now)
        .fetch_one(&mut **tx)
        .await?;
    Ok(map_dictionary_relation_row(row))
}

pub async fn upsert_relation_pair(
    pool: &DbPool,
    input: &RelationPairInput,
) -> Result<(DictionaryRelationRow, DictionaryRelationRow), DbError> {
    let mut tx = pool.begin().await?;
    let pair_key = relation_pair_key(&input.from_person_id, &input.to_person_id);
    let group_key = relation_group_key(
        &input.from_person_id,
        &input.to_person_id,
        &input.relation_type,
        &input.inverse_relation_type,
    );
    let forward = upsert_single_relation(
        &mut tx,
        &input.workspace_id,
        &input.from_person_id,
        &input.to_person_id,
        &input.relation_type,
        &pair_key,
        &group_key,
        "forward",
        input.source_kind,
        input.source_user_id.as_deref(),
        input.source_note.as_deref(),
    )
    .await?;
    let inverse = upsert_single_relation(
        &mut tx,
        &input.workspace_id,
        &input.to_person_id,
        &input.from_person_id,
        &input.inverse_relation_type,
        &pair_key,
        &group_key,
        "inverse",
        input.source_kind,
        input.source_user_id.as_deref(),
        input.source_note.as_deref(),
    )
    .await?;
    tx.commit().await?;
    Ok((forward, inverse))
}

pub async fn find_relation_by_id(
    pool: &DbPool,
    relation_id: &str,
) -> Result<Option<DictionaryRelationRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_RELATION_COLUMNS}
         FROM dictionary_relation
         WHERE id = $1"
    );
    let row: Option<DictionaryRelationTuple> = sqlx::query_as(&sql)
        .bind(relation_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_relation_row))
}

pub async fn list_relations_for_person(
    pool: &DbPool,
    workspace_id: &str,
    person_id: &str,
) -> Result<Vec<DictionaryRelationRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_RELATION_COLUMNS}
         FROM dictionary_relation
         WHERE workspace_id = $1
           AND (from_person_id = $2 OR to_person_id = $2)
           AND status = 'confirmed'
         ORDER BY lower(relation_type), created_ts, id"
    );
    let rows: Vec<DictionaryRelationTuple> = sqlx::query_as(&sql)
        .bind(workspace_id)
        .bind(person_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_dictionary_relation_row).collect())
}

pub async fn list_resolved_relations_for_person(
    pool: &DbPool,
    workspace_id: &str,
    person_id: &str,
) -> Result<Vec<DictionaryResolvedRelationRow>, sqlx::Error> {
    let sql = format!(
        "SELECT
            r.id, r.relation_group_key, r.relation_type, r.direction,
            p.{person_cols}
         FROM dictionary_relation r
         JOIN dictionary_person p
           ON p.id = CASE
                WHEN r.from_person_id = $2 THEN r.to_person_id
                ELSE r.from_person_id
              END
         WHERE r.workspace_id = $1
           AND (r.from_person_id = $2 OR r.to_person_id = $2)
           AND r.status = 'confirmed'
           AND p.archived_ts IS NULL
         ORDER BY lower(r.relation_type), lower(p.display_name), r.created_ts",
        person_cols = DICTIONARY_PERSON_COLUMNS
    );
    let rows = sqlx::query(&sql)
        .bind(workspace_id)
        .bind(person_id)
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            Ok(DictionaryResolvedRelationRow {
                relation_id: row.try_get("id")?,
                relation_group_key: row.try_get("relation_group_key")?,
                relation_type: row.try_get("relation_type")?,
                direction: row.try_get("direction")?,
                other_person: dictionary_person_row_from_pg_row(&row)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?)
}

pub async fn delete_relation_group(
    pool: &DbPool,
    workspace_id: &str,
    relation_group_key: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM dictionary_relation
         WHERE workspace_id = $1 AND relation_group_key = $2",
    )
    .bind(workspace_id)
    .bind(relation_group_key)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn upsert_fact(
    pool: &DbPool,
    input: &UpsertFactInput,
) -> Result<DictionaryFactRow, DbError> {
    let now = now_ts();
    let id = new_id();
    let sql = format!(
        "INSERT INTO dictionary_fact ({DICTIONARY_FACT_COLUMNS})
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
         ON CONFLICT (workspace_id, subject_kind, subject_id, fact_key)
         DO UPDATE SET
             value_type = EXCLUDED.value_type,
             value_text = EXCLUDED.value_text,
             value_int = EXCLUDED.value_int,
             value_bool = EXCLUDED.value_bool,
             value_date = EXCLUDED.value_date,
             value_json = EXCLUDED.value_json,
             unit = EXCLUDED.unit,
             confidence = EXCLUDED.confidence,
             status = EXCLUDED.status,
             source_kind = EXCLUDED.source_kind,
             source_user_id = EXCLUDED.source_user_id,
             source_note = EXCLUDED.source_note,
             updated_ts = EXCLUDED.updated_ts
         RETURNING {DICTIONARY_FACT_COLUMNS}"
    );
    let row = sqlx::query(&sql)
        .bind(&id)
        .bind(&input.workspace_id)
        .bind(input.subject_kind.as_str())
        .bind(&input.subject_id)
        .bind(&input.fact_key)
        .bind(input.value_type.as_str())
        .bind(&input.value_text)
        .bind(input.value_int)
        .bind(input.value_bool)
        .bind(&input.value_date)
        .bind(&input.value_json)
        .bind(&input.unit)
        .bind(input.confidence)
        .bind(&input.status)
        .bind(input.source_kind.as_str())
        .bind(&input.source_user_id)
        .bind(&input.source_note)
        .bind(now)
        .bind(now)
        .fetch_one(pool)
        .await?;
    Ok(dictionary_fact_row_from_pg_row(&row)?)
}

pub async fn list_facts_for_subject(
    pool: &DbPool,
    workspace_id: &str,
    subject_kind: SubjectKind,
    subject_id: &str,
) -> Result<Vec<DictionaryFactRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_FACT_COLUMNS}
         FROM dictionary_fact
         WHERE workspace_id = $1 AND subject_kind = $2 AND subject_id = $3
         ORDER BY fact_key, created_ts, id"
    );
    let rows = sqlx::query(&sql)
        .bind(workspace_id)
        .bind(subject_kind.as_str())
        .bind(subject_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| dictionary_fact_row_from_pg_row(&row))
        .collect()
}

pub async fn get_document_for_subject(
    pool: &DbPool,
    workspace_id: &str,
    subject_kind: SubjectKind,
    subject_id: &str,
) -> Result<Option<DictionaryDocumentRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_DOCUMENT_COLUMNS}
         FROM dictionary_document
         WHERE workspace_id = $1 AND subject_kind = $2 AND subject_id = $3"
    );
    let row: Option<DictionaryDocumentTuple> = sqlx::query_as(&sql)
        .bind(workspace_id)
        .bind(subject_kind.as_str())
        .bind(subject_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_document_row))
}

pub async fn save_document(
    pool: &DbPool,
    input: &SaveDocumentInput,
) -> Result<DictionaryDocumentRow, DbError> {
    let mut tx = pool.begin().await?;
    let now = now_ts();
    let summary = summarize_markdown(&input.markdown_body);
    let document_id = new_id();
    let sql = format!(
        "INSERT INTO dictionary_document ({DICTIONARY_DOCUMENT_COLUMNS})
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (workspace_id, subject_kind, subject_id)
         DO UPDATE SET
             title = EXCLUDED.title,
             markdown_body = EXCLUDED.markdown_body,
             summary = EXCLUDED.summary,
             last_edited_by_user_id = EXCLUDED.last_edited_by_user_id,
             last_edited_source_kind = EXCLUDED.last_edited_source_kind,
             updated_ts = EXCLUDED.updated_ts
         RETURNING {DICTIONARY_DOCUMENT_COLUMNS}"
    );
    let document_tuple: DictionaryDocumentTuple = sqlx::query_as(&sql)
        .bind(&document_id)
        .bind(&input.workspace_id)
        .bind(input.subject_kind.as_str())
        .bind(&input.subject_id)
        .bind(&input.title)
        .bind(&input.markdown_body)
        .bind(&summary)
        .bind(&input.edited_by_user_id)
        .bind(input.edit_source_kind.as_str())
        .bind(now)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
    let document = map_dictionary_document_row(document_tuple);

    let revision_no: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(revision_no), 0) + 1
         FROM dictionary_document_revision
         WHERE document_id = $1",
    )
    .bind(&document.id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO dictionary_document_revision (
             id, document_id, revision_no, markdown_body, summary,
             edited_by_user_id, edit_source_kind, edit_note, diff_json, created_ts
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(new_id())
    .bind(&document.id)
    .bind(revision_no)
    .bind(&document.markdown_body)
    .bind(&document.summary)
    .bind(&input.edited_by_user_id)
    .bind(input.edit_source_kind.as_str())
    .bind(&input.edit_note)
    .bind(Option::<Value>::None)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(document)
}

pub async fn get_account_link(
    pool: &DbPool,
    user_id: &str,
) -> Result<Option<DictionaryAccountLinkRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {DICTIONARY_ACCOUNT_LINK_COLUMNS}
         FROM dictionary_account_link
         WHERE user_id = $1"
    );
    let row: Option<DictionaryAccountLinkTuple> = sqlx::query_as(&sql)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_dictionary_account_link_row))
}

pub async fn upsert_account_link(
    pool: &DbPool,
    input: &UpsertAccountLinkInput,
) -> Result<DictionaryAccountLinkRow, DbError> {
    let existing = get_account_link(pool, &input.user_id).await?;
    let now = now_ts();
    let created_ts = existing.as_ref().map(|row| row.created_ts).unwrap_or(now);
    let sql = format!(
        "INSERT INTO dictionary_account_link ({DICTIONARY_ACCOUNT_LINK_COLUMNS})
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (user_id)
         DO UPDATE SET
             space_id = EXCLUDED.space_id,
             person_id = EXCLUDED.person_id,
             family_workspace_id = EXCLUDED.family_workspace_id,
             friends_workspace_id = EXCLUDED.friends_workspace_id,
             work_workspace_id = EXCLUDED.work_workspace_id,
             created_by_user_id = EXCLUDED.created_by_user_id,
             updated_ts = EXCLUDED.updated_ts
         RETURNING {DICTIONARY_ACCOUNT_LINK_COLUMNS}"
    );
    let row: DictionaryAccountLinkTuple = sqlx::query_as(&sql)
        .bind(&input.user_id)
        .bind(&input.space_id)
        .bind(&input.person_id)
        .bind(&input.family_workspace_id)
        .bind(&input.friends_workspace_id)
        .bind(&input.work_workspace_id)
        .bind(&input.created_by_user_id)
        .bind(created_ts)
        .bind(now)
        .fetch_one(pool)
        .await?;
    Ok(map_dictionary_account_link_row(row))
}

pub async fn archive_person_from_workspace(
    pool: &DbPool,
    workspace_id: &str,
    person_id: &str,
) -> Result<bool, DbError> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "DELETE FROM dictionary_tree_node
         WHERE workspace_id = $1 AND person_id = $2",
    )
    .bind(workspace_id)
    .bind(person_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM dictionary_fact
         WHERE workspace_id = $1 AND subject_kind = 'person' AND subject_id = $2",
    )
    .bind(workspace_id)
    .bind(person_id)
    .execute(&mut *tx)
    .await?;

    let documents: Vec<(String,)> = sqlx::query_as(
        "SELECT id
         FROM dictionary_document
         WHERE workspace_id = $1 AND subject_kind = 'person' AND subject_id = $2",
    )
    .bind(workspace_id)
    .bind(person_id)
    .fetch_all(&mut *tx)
    .await?;

    for (document_id,) in documents {
        sqlx::query("DELETE FROM dictionary_document_revision WHERE document_id = $1")
            .bind(&document_id)
            .execute(&mut *tx)
            .await?;
    }

    sqlx::query(
        "DELETE FROM dictionary_document
         WHERE workspace_id = $1 AND subject_kind = 'person' AND subject_id = $2",
    )
    .bind(workspace_id)
    .bind(person_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM dictionary_relation
         WHERE workspace_id = $1 AND (from_person_id = $2 OR to_person_id = $2)",
    )
    .bind(workspace_id)
    .bind(person_id)
    .execute(&mut *tx)
    .await?;

    let remaining_tree_nodes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM dictionary_tree_node WHERE person_id = $1",
    )
    .bind(person_id)
    .fetch_one(&mut *tx)
    .await?;
    let linked_accounts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM dictionary_account_link WHERE person_id = $1",
    )
    .bind(person_id)
    .fetch_one(&mut *tx)
    .await?;

    let affected = if remaining_tree_nodes == 0 && linked_accounts == 0 {
        sqlx::query(
            "UPDATE dictionary_person
             SET archived_ts = $1, updated_ts = $1
             WHERE id = $2 AND archived_ts IS NULL",
        )
        .bind(now_ts())
        .bind(person_id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0
    } else {
        true
    };

    tx.commit().await?;
    Ok(affected)
}
