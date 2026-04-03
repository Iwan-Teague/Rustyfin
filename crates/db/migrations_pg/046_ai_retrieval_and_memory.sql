CREATE TABLE IF NOT EXISTS ai_memory_item (
    id TEXT PRIMARY KEY,
    memory_key TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    memory_type TEXT NOT NULL,
    topic_key TEXT,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    search_text TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 1,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    UNIQUE (user_id, memory_key)
);

CREATE INDEX IF NOT EXISTS idx_ai_memory_item_user_updated
    ON ai_memory_item (user_id, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_memory_item_topic_key
    ON ai_memory_item (topic_key);

CREATE INDEX IF NOT EXISTS idx_ai_memory_item_search_vector
    ON ai_memory_item
    USING GIN (to_tsvector('simple', search_text));

CREATE TABLE IF NOT EXISTS ai_retrieval_chunk (
    id TEXT PRIMARY KEY,
    chunk_key TEXT NOT NULL UNIQUE,
    source_kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    source_sub_id TEXT,
    owner_user_id TEXT,
    access_scope TEXT NOT NULL,
    access_key TEXT,
    topic_key TEXT,
    title TEXT NOT NULL,
    excerpt TEXT NOT NULL,
    search_text TEXT NOT NULL,
    score_boost REAL NOT NULL DEFAULT 1,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    source_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_retrieval_chunk_scope_updated
    ON ai_retrieval_chunk (access_scope, access_key, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_retrieval_chunk_source_kind_ts
    ON ai_retrieval_chunk (source_kind, source_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_retrieval_chunk_topic_key
    ON ai_retrieval_chunk (topic_key);

CREATE INDEX IF NOT EXISTS idx_ai_retrieval_chunk_search_vector
    ON ai_retrieval_chunk
    USING GIN (to_tsvector('simple', search_text));

CREATE TABLE IF NOT EXISTS ai_entity_node (
    id TEXT PRIMARY KEY,
    node_key TEXT NOT NULL UNIQUE,
    owner_user_id TEXT,
    conversation_id TEXT,
    turn_id TEXT,
    entity_kind TEXT NOT NULL,
    label TEXT NOT NULL,
    identifier TEXT,
    topic_key TEXT,
    source_chunk_id TEXT,
    access_scope TEXT NOT NULL,
    access_key TEXT,
    ordinal BIGINT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_entity_node_owner_updated
    ON ai_entity_node (owner_user_id, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_entity_node_conversation_updated
    ON ai_entity_node (conversation_id, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_entity_node_topic_key
    ON ai_entity_node (topic_key);

CREATE INDEX IF NOT EXISTS idx_ai_entity_node_search_vector
    ON ai_entity_node
    USING GIN (to_tsvector('simple', COALESCE(label, '') || ' ' || COALESCE(identifier, '') || ' ' || COALESCE(topic_key, '')));

CREATE TABLE IF NOT EXISTS ai_entity_edge (
    id TEXT PRIMARY KEY,
    edge_key TEXT NOT NULL UNIQUE,
    from_node_key TEXT NOT NULL REFERENCES ai_entity_node(node_key) ON DELETE CASCADE,
    to_node_key TEXT NOT NULL REFERENCES ai_entity_node(node_key) ON DELETE CASCADE,
    relation TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 1,
    created_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_entity_edge_from_node
    ON ai_entity_edge (from_node_key, relation);

CREATE INDEX IF NOT EXISTS idx_ai_entity_edge_to_node
    ON ai_entity_edge (to_node_key, relation);

ALTER TABLE ai_conversation_turn
    ADD COLUMN IF NOT EXISTS grounding_chunks_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE ai_assistant_audit_event
    ADD COLUMN IF NOT EXISTS grounding_chunks_json TEXT NOT NULL DEFAULT '[]';
