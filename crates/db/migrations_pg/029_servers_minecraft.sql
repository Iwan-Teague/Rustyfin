CREATE TABLE IF NOT EXISTS server_instance (
    id TEXT PRIMARY KEY,
    game_kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    description TEXT,
    owner_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    created_by_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    install_mode TEXT NOT NULL,
    runtime_mode TEXT NOT NULL,
    desired_state TEXT NOT NULL,
    observed_state TEXT NOT NULL,
    health_state TEXT NOT NULL,
    instance_root TEXT NOT NULL UNIQUE,
    server_work_dir TEXT NOT NULL,
    systemd_unit_name TEXT NOT NULL UNIQUE,
    listen_host TEXT NOT NULL,
    listen_port BIGINT NOT NULL,
    advertised_host TEXT,
    advertised_port BIGINT,
    autostart BOOLEAN NOT NULL DEFAULT FALSE,
    auto_stop_when_empty BOOLEAN NOT NULL DEFAULT FALSE,
    auto_stop_idle_minutes BIGINT,
    current_player_count BIGINT NOT NULL DEFAULT 0,
    max_player_count BIGINT,
    last_ready_ts BIGINT,
    last_started_ts BIGINT,
    last_stopped_ts BIGINT,
    last_exit_code BIGINT,
    last_error_summary TEXT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_server_instance_game_state
    ON server_instance(game_kind, observed_state);
CREATE INDEX IF NOT EXISTS idx_server_instance_owner
    ON server_instance(owner_user_id);
CREATE INDEX IF NOT EXISTS idx_server_instance_port
    ON server_instance(listen_port);

CREATE TABLE IF NOT EXISTS minecraft_server_config (
    instance_id TEXT PRIMARY KEY REFERENCES server_instance(id) ON DELETE CASCADE,
    server_distribution TEXT NOT NULL,
    minecraft_version TEXT NOT NULL,
    loader_version TEXT,
    java_path TEXT NOT NULL,
    min_memory_mb BIGINT NOT NULL,
    max_memory_mb BIGINT NOT NULL,
    jvm_flags_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    world_name TEXT NOT NULL,
    world_seed TEXT,
    level_type TEXT NOT NULL,
    gamemode TEXT NOT NULL,
    difficulty TEXT NOT NULL,
    hardcore BOOLEAN NOT NULL DEFAULT FALSE,
    motd TEXT NOT NULL,
    online_mode BOOLEAN NOT NULL DEFAULT TRUE,
    pvp BOOLEAN NOT NULL DEFAULT TRUE,
    allow_flight BOOLEAN NOT NULL DEFAULT FALSE,
    enable_command_block BOOLEAN NOT NULL DEFAULT FALSE,
    view_distance BIGINT NOT NULL DEFAULT 10,
    simulation_distance BIGINT NOT NULL DEFAULT 10,
    spawn_protection BIGINT NOT NULL DEFAULT 16,
    white_list_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    server_icon_path TEXT,
    eula_accepted BOOLEAN NOT NULL DEFAULT FALSE,
    eula_accepted_by_user_id TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    eula_accepted_ts BIGINT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS server_instance_member (
    instance_id TEXT NOT NULL REFERENCES server_instance(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    created_by_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    created_ts BIGINT NOT NULL,
    PRIMARY KEY(instance_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_server_instance_member_user
    ON server_instance_member(user_id);

CREATE TABLE IF NOT EXISTS server_instance_event (
    id TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL REFERENCES server_instance(id) ON DELETE CASCADE,
    job_id TEXT REFERENCES job(id) ON DELETE SET NULL,
    actor_user_id TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    level TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    message TEXT NOT NULL,
    details_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_server_instance_event_created
    ON server_instance_event(instance_id, created_ts DESC);
CREATE INDEX IF NOT EXISTS idx_server_instance_event_job
    ON server_instance_event(job_id);

CREATE TABLE IF NOT EXISTS server_discovery_candidate (
    id TEXT PRIMARY KEY,
    game_kind TEXT NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    detected_name TEXT,
    detected_distribution TEXT,
    detected_version TEXT,
    detection_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    imported_instance_id TEXT REFERENCES server_instance(id) ON DELETE SET NULL,
    last_scan_status TEXT NOT NULL,
    first_seen_ts BIGINT NOT NULL,
    last_seen_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_server_discovery_candidate_status
    ON server_discovery_candidate(game_kind, last_scan_status);
