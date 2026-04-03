use crate::DbPool;
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct AiMemoryItemRow {
    pub id: String,
    pub memory_key: String,
    pub user_id: String,
    pub memory_type: String,
    pub topic_key: Option<String>,
    pub title: String,
    pub content: String,
    pub search_text: String,
    pub weight: f64,
    pub metadata_json: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

pub struct UpsertAiMemoryItemParams<'a> {
    pub user_id: &'a str,
    pub memory_key: &'a str,
    pub memory_type: &'a str,
    pub topic_key: Option<&'a str>,
    pub title: &'a str,
    pub content: &'a str,
    pub search_text: &'a str,
    pub weight: f64,
    pub metadata_json: &'a str,
}

#[derive(Debug, Clone)]
pub struct AiMemoryItemHit {
    pub row: AiMemoryItemRow,
    pub rank: f64,
}

#[derive(Debug, Clone)]
pub struct AiRetrievalChunkRow {
    pub id: String,
    pub chunk_key: String,
    pub source_kind: String,
    pub source_id: String,
    pub source_sub_id: Option<String>,
    pub owner_user_id: Option<String>,
    pub access_scope: String,
    pub access_key: Option<String>,
    pub topic_key: Option<String>,
    pub title: String,
    pub excerpt: String,
    pub search_text: String,
    pub score_boost: f64,
    pub metadata_json: String,
    pub source_ts: i64,
    pub updated_ts: i64,
}

pub struct UpsertAiRetrievalChunkParams<'a> {
    pub chunk_key: &'a str,
    pub source_kind: &'a str,
    pub source_id: &'a str,
    pub source_sub_id: Option<&'a str>,
    pub owner_user_id: Option<&'a str>,
    pub access_scope: &'a str,
    pub access_key: Option<&'a str>,
    pub topic_key: Option<&'a str>,
    pub title: &'a str,
    pub excerpt: &'a str,
    pub search_text: &'a str,
    pub score_boost: f64,
    pub metadata_json: &'a str,
    pub source_ts: i64,
}

#[derive(Debug, Clone)]
pub struct AiRetrievalChunkHit {
    pub row: AiRetrievalChunkRow,
    pub rank: f64,
}

#[derive(Debug, Clone)]
pub struct AiEntityNodeRow {
    pub id: String,
    pub node_key: String,
    pub owner_user_id: Option<String>,
    pub conversation_id: Option<String>,
    pub turn_id: Option<String>,
    pub entity_kind: String,
    pub label: String,
    pub identifier: Option<String>,
    pub topic_key: Option<String>,
    pub source_chunk_id: Option<String>,
    pub access_scope: String,
    pub access_key: Option<String>,
    pub ordinal: i64,
    pub metadata_json: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

pub struct UpsertAiEntityNodeParams<'a> {
    pub node_key: &'a str,
    pub owner_user_id: Option<&'a str>,
    pub conversation_id: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub entity_kind: &'a str,
    pub label: &'a str,
    pub identifier: Option<&'a str>,
    pub topic_key: Option<&'a str>,
    pub source_chunk_id: Option<&'a str>,
    pub access_scope: &'a str,
    pub access_key: Option<&'a str>,
    pub ordinal: i64,
    pub metadata_json: &'a str,
}

#[derive(Debug, Clone)]
pub struct AiEntityNodeHit {
    pub row: AiEntityNodeRow,
    pub rank: f64,
}

#[derive(Debug, Clone)]
pub struct AiEntityEdgeRow {
    pub id: String,
    pub edge_key: String,
    pub from_node_key: String,
    pub to_node_key: String,
    pub relation: String,
    pub weight: f64,
    pub created_ts: i64,
}

pub struct UpsertAiEntityEdgeParams<'a> {
    pub edge_key: &'a str,
    pub from_node_key: &'a str,
    pub to_node_key: &'a str,
    pub relation: &'a str,
    pub weight: f64,
}

fn map_memory_item_row(
    row: (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        f64,
        String,
        i64,
        i64,
    ),
) -> AiMemoryItemRow {
    AiMemoryItemRow {
        id: row.0,
        memory_key: row.1,
        user_id: row.2,
        memory_type: row.3,
        topic_key: row.4,
        title: row.5,
        content: row.6,
        search_text: row.7,
        weight: row.8,
        metadata_json: row.9,
        created_ts: row.10,
        updated_ts: row.11,
    }
}

fn map_retrieval_chunk_row(
    row: (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        f64,
        String,
        i64,
        i64,
    ),
) -> AiRetrievalChunkRow {
    AiRetrievalChunkRow {
        id: row.0,
        chunk_key: row.1,
        source_kind: row.2,
        source_id: row.3,
        source_sub_id: row.4,
        owner_user_id: row.5,
        access_scope: row.6,
        access_key: row.7,
        topic_key: row.8,
        title: row.9,
        excerpt: row.10,
        search_text: row.11,
        score_boost: row.12,
        metadata_json: row.13,
        source_ts: row.14,
        updated_ts: row.15,
    }
}

fn map_entity_node_row(
    row: (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        i64,
        String,
        i64,
        i64,
    ),
) -> AiEntityNodeRow {
    AiEntityNodeRow {
        id: row.0,
        node_key: row.1,
        owner_user_id: row.2,
        conversation_id: row.3,
        turn_id: row.4,
        entity_kind: row.5,
        label: row.6,
        identifier: row.7,
        topic_key: row.8,
        source_chunk_id: row.9,
        access_scope: row.10,
        access_key: row.11,
        ordinal: row.12,
        metadata_json: row.13,
        created_ts: row.14,
        updated_ts: row.15,
    }
}

fn map_entity_edge_row(row: (String, String, String, String, String, f64, i64)) -> AiEntityEdgeRow {
    AiEntityEdgeRow {
        id: row.0,
        edge_key: row.1,
        from_node_key: row.2,
        to_node_key: row.3,
        relation: row.4,
        weight: row.5,
        created_ts: row.6,
    }
}

fn memory_scope_clause(owner_user_id_column: &str) -> String {
    format!(
        "(access_scope = 'shared' OR (access_scope = 'admin' AND $1) OR (access_scope = 'user' AND {owner_user_id_column} = $2))"
    )
}

fn retrieval_scope_clause(
    is_admin: bool,
    allowed_library_ids: Option<&[String]>,
    owner_user_id_column: &str,
    access_key_column: &str,
    next_param: &mut usize,
) -> String {
    let mut clauses = vec![
        "access_scope = 'shared'".to_string(),
        format!("(access_scope = 'admin' AND ${})", *next_param),
        format!(
            "(access_scope = 'user' AND {owner_user_id_column} = ${})",
            *next_param + 1
        ),
    ];
    *next_param += 2;

    if is_admin {
        clauses.push("access_scope = 'library'".to_string());
    } else if let Some(library_ids) = allowed_library_ids {
        if library_ids.is_empty() {
            clauses.push("FALSE".to_string());
        } else {
            let placeholders = crate::repo::dollar_placeholders(*next_param, library_ids.len());
            *next_param += library_ids.len();
            clauses.push(format!(
                "(access_scope = 'library' AND {access_key_column} IN ({placeholders}))"
            ));
        }
    }

    format!("({})", clauses.join(" OR "))
}

pub async fn upsert_memory_item(
    pool: &DbPool,
    params: UpsertAiMemoryItemParams<'_>,
) -> Result<AiMemoryItemRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let row: (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        f64,
        String,
        i64,
        i64,
    ) = sqlx::query_as(
        "INSERT INTO ai_memory_item (
            id, memory_key, user_id, memory_type, topic_key, title, content, search_text, weight,
            metadata_json, created_ts, updated_ts
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT (user_id, memory_key) DO UPDATE SET
            memory_type = EXCLUDED.memory_type,
            topic_key = EXCLUDED.topic_key,
            title = EXCLUDED.title,
            content = EXCLUDED.content,
            search_text = EXCLUDED.search_text,
            weight = EXCLUDED.weight,
            metadata_json = EXCLUDED.metadata_json,
            updated_ts = EXCLUDED.updated_ts
        RETURNING id, memory_key, user_id, memory_type, topic_key, title, content, search_text, weight, metadata_json, created_ts, updated_ts",
    )
    .bind(&id)
    .bind(params.memory_key)
    .bind(params.user_id)
    .bind(params.memory_type)
    .bind(params.topic_key)
    .bind(params.title)
    .bind(params.content)
    .bind(params.search_text)
    .bind(params.weight)
    .bind(params.metadata_json)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(map_memory_item_row(row))
}

pub async fn delete_memory_item(
    pool: &DbPool,
    user_id: &str,
    memory_key: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM ai_memory_item WHERE user_id = $1 AND memory_key = $2")
        .bind(user_id)
        .bind(memory_key)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_memory_items_for_user(
    pool: &DbPool,
    user_id: &str,
    limit: i64,
) -> Result<Vec<AiMemoryItemRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        f64,
        String,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT id, memory_key, user_id, memory_type, topic_key, title, content, search_text, weight, metadata_json, created_ts, updated_ts
         FROM ai_memory_item
         WHERE user_id = $1
         ORDER BY updated_ts DESC, memory_key ASC
         LIMIT $2",
    )
    .bind(user_id)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(map_memory_item_row).collect())
}

pub async fn search_memory_items_for_user(
    pool: &DbPool,
    user_id: &str,
    topic_key: Option<&str>,
    query: Option<&str>,
    limit: i64,
) -> Result<Vec<AiMemoryItemHit>, sqlx::Error> {
    let normalized_query = query.map(str::trim).filter(|value| !value.is_empty());
    let query_param_index = normalized_query.map(|_| 2 + usize::from(topic_key.is_some()));
    let mut sql = String::from(
        "SELECT id, memory_key, user_id, memory_type, topic_key, title, content, search_text, weight, metadata_json, created_ts, updated_ts",
    );
    if let Some(query_param_index) = query_param_index {
        sql.push_str(&format!(
            ", ts_rank_cd(to_tsvector('simple', search_text), websearch_to_tsquery('simple', ${query_param_index})) AS rank",
        ));
    } else {
        sql.push_str(", 0::double precision AS rank");
    }

    sql.push_str(" FROM ai_memory_item WHERE user_id = $1");

    if let Some(topic_key) = topic_key {
        sql.push_str(" AND topic_key = $2");
        let _ = topic_key;
    }

    if let Some(query_param_index) = query_param_index {
        sql.push_str(&format!(
            " AND to_tsvector('simple', search_text) @@ websearch_to_tsquery('simple', ${query_param_index})",
        ));
    }

    sql.push_str(" ORDER BY ");
    if normalized_query.is_some() {
        sql.push_str("rank DESC, ");
    }
    sql.push_str("weight DESC, updated_ts DESC, memory_key ASC LIMIT ");
    sql.push_str(&limit.clamp(1, 200).to_string());

    let mut query_builder = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            f64,
            String,
            i64,
            i64,
            f64,
        ),
    >(&sql)
    .bind(user_id);

    if let Some(topic_key) = topic_key {
        query_builder = query_builder.bind(topic_key);
    }
    if let Some(query) = normalized_query {
        query_builder = query_builder.bind(query);
    }

    let rows = query_builder.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|row| AiMemoryItemHit {
            row: map_memory_item_row((
                row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10,
                row.11,
            )),
            rank: row.12,
        })
        .collect())
}

pub async fn upsert_retrieval_chunk(
    pool: &DbPool,
    params: UpsertAiRetrievalChunkParams<'_>,
) -> Result<AiRetrievalChunkRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let row: (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        f64,
        String,
        i64,
        i64,
    ) = sqlx::query_as(
        "INSERT INTO ai_retrieval_chunk (
            id, chunk_key, source_kind, source_id, source_sub_id, owner_user_id, access_scope,
            access_key, topic_key, title, excerpt, search_text, score_boost, metadata_json,
            source_ts, updated_ts
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
        ON CONFLICT (chunk_key) DO UPDATE SET
            source_kind = EXCLUDED.source_kind,
            source_id = EXCLUDED.source_id,
            source_sub_id = EXCLUDED.source_sub_id,
            owner_user_id = EXCLUDED.owner_user_id,
            access_scope = EXCLUDED.access_scope,
            access_key = EXCLUDED.access_key,
            topic_key = EXCLUDED.topic_key,
            title = EXCLUDED.title,
            excerpt = EXCLUDED.excerpt,
            search_text = EXCLUDED.search_text,
            score_boost = EXCLUDED.score_boost,
            metadata_json = EXCLUDED.metadata_json,
            source_ts = EXCLUDED.source_ts,
            updated_ts = EXCLUDED.updated_ts
        RETURNING id, chunk_key, source_kind, source_id, source_sub_id, owner_user_id, access_scope,
                  access_key, topic_key, title, excerpt, search_text, score_boost, metadata_json,
                  source_ts, updated_ts",
    )
    .bind(&id)
    .bind(params.chunk_key)
    .bind(params.source_kind)
    .bind(params.source_id)
    .bind(params.source_sub_id)
    .bind(params.owner_user_id)
    .bind(params.access_scope)
    .bind(params.access_key)
    .bind(params.topic_key)
    .bind(params.title)
    .bind(params.excerpt)
    .bind(params.search_text)
    .bind(params.score_boost)
    .bind(params.metadata_json)
    .bind(params.source_ts)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(map_retrieval_chunk_row(row))
}

pub async fn delete_retrieval_chunks_by_source_kind(
    pool: &DbPool,
    source_kind: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM ai_retrieval_chunk WHERE source_kind = $1")
        .bind(source_kind)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn search_retrieval_chunks(
    pool: &DbPool,
    user_id: &str,
    is_admin: bool,
    allowed_library_ids: Option<&[String]>,
    topic_key: Option<&str>,
    query: Option<&str>,
    limit: i64,
) -> Result<Vec<AiRetrievalChunkHit>, sqlx::Error> {
    let normalized_query = query.map(str::trim).filter(|value| !value.is_empty());
    let mut next_param = 1usize;
    let scope_clause = retrieval_scope_clause(
        is_admin,
        allowed_library_ids,
        "owner_user_id",
        "access_key",
        &mut next_param,
    );
    let query_param_index = normalized_query.map(|_| next_param + usize::from(topic_key.is_some()));
    let mut sql = String::from(
        "SELECT id, chunk_key, source_kind, source_id, source_sub_id, owner_user_id, access_scope,
                access_key, topic_key, title, excerpt, search_text, score_boost, metadata_json,
                source_ts, updated_ts",
    );
    if let Some(query_param_index) = query_param_index {
        sql.push_str(&format!(
            ", ts_rank_cd(to_tsvector('simple', search_text), websearch_to_tsquery('simple', ${query_param_index})) AS rank",
        ));
    } else {
        sql.push_str(", 0::double precision AS rank");
    }
    sql.push_str(" FROM ai_retrieval_chunk WHERE ");
    sql.push_str(&scope_clause);

    if let Some(topic_key) = topic_key {
        sql.push_str(&format!(" AND topic_key = ${next_param}"));
        let _ = topic_key;
    }

    if let Some(query_param_index) = query_param_index {
        sql.push_str(&format!(
            " AND to_tsvector('simple', search_text) @@ websearch_to_tsquery('simple', ${query_param_index})",
        ));
    }

    sql.push_str(" ORDER BY ");
    if normalized_query.is_some() {
        sql.push_str("rank DESC, ");
    }
    sql.push_str("score_boost DESC, source_ts DESC, updated_ts DESC, chunk_key ASC LIMIT ");
    sql.push_str(&limit.clamp(1, 200).to_string());

    let mut query_builder = sqlx::query(&sql).bind(is_admin).bind(user_id);

    if let Some(library_ids) = allowed_library_ids {
        if !is_admin {
            for library_id in library_ids {
                query_builder = query_builder.bind(library_id);
            }
        }
    }
    if let Some(topic_key) = topic_key {
        query_builder = query_builder.bind(topic_key);
    }
    if let Some(query) = normalized_query {
        query_builder = query_builder.bind(query);
    }

    let rows = query_builder.fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            Ok(AiRetrievalChunkHit {
                row: AiRetrievalChunkRow {
                    id: row.try_get("id")?,
                    chunk_key: row.try_get("chunk_key")?,
                    source_kind: row.try_get("source_kind")?,
                    source_id: row.try_get("source_id")?,
                    source_sub_id: row.try_get("source_sub_id")?,
                    owner_user_id: row.try_get("owner_user_id")?,
                    access_scope: row.try_get("access_scope")?,
                    access_key: row.try_get("access_key")?,
                    topic_key: row.try_get("topic_key")?,
                    title: row.try_get("title")?,
                    excerpt: row.try_get("excerpt")?,
                    search_text: row.try_get("search_text")?,
                    score_boost: row.try_get("score_boost")?,
                    metadata_json: row.try_get("metadata_json")?,
                    source_ts: row.try_get("source_ts")?,
                    updated_ts: row.try_get("updated_ts")?,
                },
                rank: row.try_get("rank")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
}

pub async fn upsert_entity_node(
    pool: &DbPool,
    params: UpsertAiEntityNodeParams<'_>,
) -> Result<AiEntityNodeRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let row: (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        i64,
        String,
        i64,
        i64,
    ) = sqlx::query_as(
        "INSERT INTO ai_entity_node (
            id, node_key, owner_user_id, conversation_id, turn_id, entity_kind, label, identifier,
            topic_key, source_chunk_id, access_scope, access_key, ordinal, metadata_json,
            created_ts, updated_ts
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                  $9, $10, $11, $12, $13, $14, $15, $16)
        ON CONFLICT (node_key) DO UPDATE SET
            owner_user_id = EXCLUDED.owner_user_id,
            conversation_id = EXCLUDED.conversation_id,
            turn_id = EXCLUDED.turn_id,
            entity_kind = EXCLUDED.entity_kind,
            label = EXCLUDED.label,
            identifier = EXCLUDED.identifier,
            topic_key = EXCLUDED.topic_key,
            source_chunk_id = EXCLUDED.source_chunk_id,
            access_scope = EXCLUDED.access_scope,
            access_key = EXCLUDED.access_key,
            ordinal = EXCLUDED.ordinal,
            metadata_json = EXCLUDED.metadata_json,
            updated_ts = EXCLUDED.updated_ts
        RETURNING id, node_key, owner_user_id, conversation_id, turn_id, entity_kind, label, identifier,
                  topic_key, source_chunk_id, access_scope, access_key, ordinal, metadata_json,
                  created_ts, updated_ts",
    )
    .bind(&id)
    .bind(params.node_key)
    .bind(params.owner_user_id)
    .bind(params.conversation_id)
    .bind(params.turn_id)
    .bind(params.entity_kind)
    .bind(params.label)
    .bind(params.identifier)
    .bind(params.topic_key)
    .bind(params.source_chunk_id)
    .bind(params.access_scope)
    .bind(params.access_key)
    .bind(params.ordinal)
    .bind(params.metadata_json)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(map_entity_node_row(row))
}

pub async fn upsert_entity_edge(
    pool: &DbPool,
    params: UpsertAiEntityEdgeParams<'_>,
) -> Result<AiEntityEdgeRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let row: (String, String, String, String, String, f64, i64) = sqlx::query_as(
        "INSERT INTO ai_entity_edge (
            id, edge_key, from_node_key, to_node_key, relation, weight, created_ts
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (edge_key) DO UPDATE SET
            from_node_key = EXCLUDED.from_node_key,
            to_node_key = EXCLUDED.to_node_key,
            relation = EXCLUDED.relation,
            weight = EXCLUDED.weight
        RETURNING id, edge_key, from_node_key, to_node_key, relation, weight, created_ts",
    )
    .bind(&id)
    .bind(params.edge_key)
    .bind(params.from_node_key)
    .bind(params.to_node_key)
    .bind(params.relation)
    .bind(params.weight)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(map_entity_edge_row(row))
}

pub async fn delete_entity_nodes_for_turn(
    pool: &DbPool,
    turn_id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM ai_entity_node WHERE turn_id = $1")
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn search_entity_nodes_for_user(
    pool: &DbPool,
    user_id: &str,
    is_admin: bool,
    topic_key: Option<&str>,
    query: Option<&str>,
    limit: i64,
) -> Result<Vec<AiEntityNodeHit>, sqlx::Error> {
    let normalized_query = query.map(str::trim).filter(|value| !value.is_empty());
    let mut sql = String::from(
        "SELECT id, node_key, owner_user_id, conversation_id, turn_id, entity_kind, label, identifier,
                topic_key, source_chunk_id, access_scope, access_key, ordinal, metadata_json,
                created_ts, updated_ts",
    );
    let query_param_index = normalized_query.map(|_| 3 + usize::from(topic_key.is_some()));
    if let Some(query_param_index) = query_param_index {
        sql.push_str(&format!(
            ", ts_rank_cd(to_tsvector('simple', COALESCE(label, '') || ' ' || COALESCE(identifier, '') || ' ' || COALESCE(topic_key, '')), websearch_to_tsquery('simple', ${query_param_index})) AS rank",
        ));
    } else {
        sql.push_str(", 0::double precision AS rank");
    }
    sql.push_str(" FROM ai_entity_node WHERE ");

    let next_param = 3usize;
    sql.push_str(&memory_scope_clause("owner_user_id"));

    if let Some(topic_key) = topic_key {
        sql.push_str(&format!(" AND topic_key = ${next_param}"));
        let _ = topic_key;
    }

    if let Some(query_param_index) = query_param_index {
        sql.push_str(&format!(
            " AND to_tsvector('simple', COALESCE(label, '') || ' ' || COALESCE(identifier, '') || ' ' || COALESCE(topic_key, '')) @@ websearch_to_tsquery('simple', ${query_param_index})",
        ));
    }

    sql.push_str(" ORDER BY ");
    if normalized_query.is_some() {
        sql.push_str("rank DESC, ");
    }
    sql.push_str("updated_ts DESC, ordinal ASC, node_key ASC LIMIT ");
    sql.push_str(&limit.clamp(1, 200).to_string());

    let mut query_builder = sqlx::query(&sql).bind(is_admin).bind(user_id);

    if let Some(topic_key) = topic_key {
        query_builder = query_builder.bind(topic_key);
    }
    if let Some(query) = normalized_query {
        query_builder = query_builder.bind(query);
    }

    let rows = query_builder.fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            Ok(AiEntityNodeHit {
                row: AiEntityNodeRow {
                    id: row.try_get("id")?,
                    node_key: row.try_get("node_key")?,
                    owner_user_id: row.try_get("owner_user_id")?,
                    conversation_id: row.try_get("conversation_id")?,
                    turn_id: row.try_get("turn_id")?,
                    entity_kind: row.try_get("entity_kind")?,
                    label: row.try_get("label")?,
                    identifier: row.try_get("identifier")?,
                    topic_key: row.try_get("topic_key")?,
                    source_chunk_id: row.try_get("source_chunk_id")?,
                    access_scope: row.try_get("access_scope")?,
                    access_key: row.try_get("access_key")?,
                    ordinal: row.try_get("ordinal")?,
                    metadata_json: row.try_get("metadata_json")?,
                    created_ts: row.try_get("created_ts")?,
                    updated_ts: row.try_get("updated_ts")?,
                },
                rank: row.try_get("rank")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
}
