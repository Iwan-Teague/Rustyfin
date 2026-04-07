-- 055_dictionary_core.sql
-- Human Dictionary core schema for Rustyfin.
-- Notes:
--   * Uses BIGINT timestamps to match the repo's current direction.
--   * Uses workspace membership as the primary privacy boundary.
--   * Keeps canonical people at the space level while tree/document/fact views are workspace-scoped.

CREATE TABLE IF NOT EXISTS dictionary_space (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    owner_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE RESTRICT,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    CHECK (length(btrim(slug)) > 0),
    CHECK (length(btrim(title)) > 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_dictionary_space_owner_slug
    ON dictionary_space (owner_user_id, slug);

CREATE UNIQUE INDEX IF NOT EXISTS idx_dictionary_space_owner_default
    ON dictionary_space (owner_user_id)
    WHERE is_default = TRUE;

CREATE INDEX IF NOT EXISTS idx_dictionary_space_owner
    ON dictionary_space (owner_user_id, updated_ts DESC);

CREATE TABLE IF NOT EXISTS dictionary_workspace (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES dictionary_space(id) ON DELETE CASCADE,
    slug TEXT NOT NULL,
    title TEXT NOT NULL,
    workspace_kind TEXT NOT NULL CHECK (
        workspace_kind IN ('family_shared', 'friends_private', 'work_private', 'custom')
    ),
    owner_user_id TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    is_system_seeded BOOLEAN NOT NULL DEFAULT FALSE,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    CHECK (length(btrim(slug)) > 0),
    CHECK (length(btrim(title)) > 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_dictionary_workspace_space_slug
    ON dictionary_workspace (space_id, slug);

CREATE INDEX IF NOT EXISTS idx_dictionary_workspace_space_kind
    ON dictionary_workspace (space_id, workspace_kind, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_dictionary_workspace_owner_kind
    ON dictionary_workspace (owner_user_id, workspace_kind);

CREATE TABLE IF NOT EXISTS dictionary_workspace_member (
    workspace_id TEXT NOT NULL REFERENCES dictionary_workspace(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
    added_by_user_id TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    created_ts BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_dictionary_workspace_member_user
    ON dictionary_workspace_member (user_id, workspace_id);

CREATE TABLE IF NOT EXISTS dictionary_person (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES dictionary_space(id) ON DELETE CASCADE,
    canonical_name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    summary TEXT,
    primary_photo_path TEXT,
    primary_photo_content_type TEXT,
    search_text TEXT NOT NULL DEFAULT '',
    created_by_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE RESTRICT,
    archived_ts BIGINT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    CHECK (length(btrim(canonical_name)) > 0),
    CHECK (length(btrim(display_name)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_dictionary_person_space_archived
    ON dictionary_person (space_id, archived_ts, updated_ts DESC);

CREATE INDEX IF NOT EXISTS idx_dictionary_person_space_display_name_lower
    ON dictionary_person (space_id, lower(display_name));

CREATE INDEX IF NOT EXISTS idx_dictionary_person_search_tsv
    ON dictionary_person USING GIN (to_tsvector('simple', coalesce(search_text, '')));

CREATE TABLE IF NOT EXISTS dictionary_person_alias (
    id TEXT PRIMARY KEY,
    person_id TEXT NOT NULL REFERENCES dictionary_person(id) ON DELETE CASCADE,
    alias TEXT NOT NULL,
    alias_kind TEXT NOT NULL CHECK (
        alias_kind IN ('nickname', 'family_role', 'search_name', 'maiden_name', 'custom')
    ),
    created_by_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE RESTRICT,
    created_ts BIGINT NOT NULL,
    CHECK (length(btrim(alias)) > 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_dictionary_person_alias_person_lower
    ON dictionary_person_alias (person_id, lower(alias));

CREATE INDEX IF NOT EXISTS idx_dictionary_person_alias_lower
    ON dictionary_person_alias (lower(alias));

CREATE TABLE IF NOT EXISTS dictionary_tree_node (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES dictionary_workspace(id) ON DELETE CASCADE,
    parent_node_id TEXT REFERENCES dictionary_tree_node(id) ON DELETE CASCADE,
    node_kind TEXT NOT NULL CHECK (node_kind IN ('root', 'group', 'person', 'shortcut')),
    title TEXT NOT NULL,
    person_id TEXT REFERENCES dictionary_person(id) ON DELETE SET NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    icon_name TEXT,
    note TEXT,
    is_system_seeded BOOLEAN NOT NULL DEFAULT FALSE,
    created_by_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE RESTRICT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    CHECK (length(btrim(title)) > 0),
    CHECK (
        (node_kind IN ('person', 'shortcut') AND person_id IS NOT NULL)
        OR (node_kind IN ('root', 'group') AND person_id IS NULL)
    ),
    CHECK (
        (node_kind = 'root' AND parent_node_id IS NULL)
        OR (node_kind <> 'root')
    )
);

CREATE INDEX IF NOT EXISTS idx_dictionary_tree_node_workspace_parent_sort
    ON dictionary_tree_node (workspace_id, parent_node_id, sort_order, created_ts);

CREATE INDEX IF NOT EXISTS idx_dictionary_tree_node_workspace_person
    ON dictionary_tree_node (workspace_id, person_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_dictionary_tree_node_one_root_per_workspace
    ON dictionary_tree_node (workspace_id)
    WHERE node_kind = 'root' AND parent_node_id IS NULL;

CREATE TABLE IF NOT EXISTS dictionary_relation (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES dictionary_workspace(id) ON DELETE CASCADE,
    from_person_id TEXT NOT NULL REFERENCES dictionary_person(id) ON DELETE CASCADE,
    to_person_id TEXT NOT NULL REFERENCES dictionary_person(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    pair_key TEXT NOT NULL,
    relation_group_key TEXT NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('forward', 'inverse')),
    status TEXT NOT NULL DEFAULT 'confirmed' CHECK (
        status IN ('confirmed', 'proposed', 'conflicting', 'deprecated')
    ),
    source_kind TEXT NOT NULL DEFAULT 'manual' CHECK (
        source_kind IN ('manual', 'ai', 'import', 'system')
    ),
    source_user_id TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    source_note TEXT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    CHECK (length(btrim(relation_type)) > 0),
    CHECK (from_person_id <> to_person_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_dictionary_relation_unique_edge
    ON dictionary_relation (workspace_id, from_person_id, to_person_id, relation_type, direction);

CREATE INDEX IF NOT EXISTS idx_dictionary_relation_from
    ON dictionary_relation (workspace_id, from_person_id, relation_type, status);

CREATE INDEX IF NOT EXISTS idx_dictionary_relation_to
    ON dictionary_relation (workspace_id, to_person_id, relation_type, status);

CREATE INDEX IF NOT EXISTS idx_dictionary_relation_pair
    ON dictionary_relation (workspace_id, pair_key);

CREATE INDEX IF NOT EXISTS idx_dictionary_relation_group
    ON dictionary_relation (workspace_id, relation_group_key);

CREATE TABLE IF NOT EXISTS dictionary_fact (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES dictionary_workspace(id) ON DELETE CASCADE,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('person', 'tree_node', 'workspace')),
    subject_id TEXT NOT NULL,
    fact_key TEXT NOT NULL,
    value_type TEXT NOT NULL CHECK (value_type IN ('text', 'int', 'bool', 'date', 'json')),
    value_text TEXT,
    value_int BIGINT,
    value_bool BOOLEAN,
    value_date TEXT,
    value_json JSONB,
    unit TEXT,
    confidence DOUBLE PRECISION CHECK (
        confidence IS NULL OR (confidence >= 0 AND confidence <= 1)
    ),
    status TEXT NOT NULL DEFAULT 'confirmed' CHECK (
        status IN ('confirmed', 'proposed', 'conflicting', 'deprecated')
    ),
    source_kind TEXT NOT NULL DEFAULT 'manual' CHECK (
        source_kind IN ('manual', 'ai', 'import', 'system')
    ),
    source_user_id TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    source_note TEXT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    CHECK (length(btrim(fact_key)) > 0),
    UNIQUE (workspace_id, subject_kind, subject_id, fact_key)
);

CREATE INDEX IF NOT EXISTS idx_dictionary_fact_subject
    ON dictionary_fact (workspace_id, subject_kind, subject_id, fact_key);

CREATE INDEX IF NOT EXISTS idx_dictionary_fact_key
    ON dictionary_fact (workspace_id, fact_key);

CREATE TABLE IF NOT EXISTS dictionary_document (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES dictionary_workspace(id) ON DELETE CASCADE,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('person', 'tree_node', 'workspace')),
    subject_id TEXT NOT NULL,
    title TEXT NOT NULL,
    markdown_body TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    last_edited_by_user_id TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    last_edited_source_kind TEXT NOT NULL DEFAULT 'manual' CHECK (
        last_edited_source_kind IN ('manual', 'ai', 'import', 'system')
    ),
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    CHECK (length(btrim(title)) > 0),
    UNIQUE (workspace_id, subject_kind, subject_id)
);

CREATE INDEX IF NOT EXISTS idx_dictionary_document_summary_tsv
    ON dictionary_document USING GIN (
        to_tsvector('simple', coalesce(summary, '') || ' ' || coalesce(markdown_body, ''))
    );

CREATE TABLE IF NOT EXISTS dictionary_document_revision (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES dictionary_document(id) ON DELETE CASCADE,
    revision_no INTEGER NOT NULL,
    markdown_body TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    edited_by_user_id TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    edit_source_kind TEXT NOT NULL DEFAULT 'manual' CHECK (
        edit_source_kind IN ('manual', 'ai', 'import', 'system')
    ),
    edit_note TEXT,
    diff_json JSONB,
    created_ts BIGINT NOT NULL,
    UNIQUE (document_id, revision_no)
);

CREATE INDEX IF NOT EXISTS idx_dictionary_document_revision_document
    ON dictionary_document_revision (document_id, revision_no DESC);

CREATE TABLE IF NOT EXISTS dictionary_account_link (
    user_id TEXT PRIMARY KEY REFERENCES "user"(id) ON DELETE CASCADE,
    space_id TEXT NOT NULL REFERENCES dictionary_space(id) ON DELETE CASCADE,
    person_id TEXT NOT NULL REFERENCES dictionary_person(id) ON DELETE CASCADE,
    family_workspace_id TEXT REFERENCES dictionary_workspace(id) ON DELETE SET NULL,
    friends_workspace_id TEXT REFERENCES dictionary_workspace(id) ON DELETE SET NULL,
    work_workspace_id TEXT REFERENCES dictionary_workspace(id) ON DELETE SET NULL,
    created_by_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE RESTRICT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_dictionary_account_link_space_person
    ON dictionary_account_link (space_id, person_id);

CREATE INDEX IF NOT EXISTS idx_dictionary_account_link_person
    ON dictionary_account_link (person_id);
