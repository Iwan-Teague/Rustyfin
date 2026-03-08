use crate::DbPool;
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct MinecraftServerRow {
    pub id: String,
    pub display_name: String,
    pub slug: String,
    pub description: Option<String>,
    pub owner_user_id: String,
    pub owner_display_name: String,
    pub install_mode: String,
    pub runtime_mode: String,
    pub desired_state: String,
    pub observed_state: String,
    pub health_state: String,
    pub instance_root: String,
    pub server_work_dir: String,
    pub systemd_unit_name: String,
    pub listen_host: String,
    pub listen_port: i64,
    pub advertised_host: Option<String>,
    pub advertised_port: Option<i64>,
    pub autostart: bool,
    pub auto_stop_when_empty: bool,
    pub auto_stop_idle_minutes: Option<i64>,
    pub current_player_count: i64,
    pub max_player_count: Option<i64>,
    pub last_ready_ts: Option<i64>,
    pub last_started_ts: Option<i64>,
    pub last_stopped_ts: Option<i64>,
    pub last_exit_code: Option<i64>,
    pub last_error_summary: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub server_distribution: String,
    pub minecraft_version: String,
    pub java_path: String,
    pub world_name: String,
    pub gamemode: String,
    pub difficulty: String,
    pub hardcore: bool,
    pub motd: String,
    pub min_memory_mb: i64,
    pub max_memory_mb: i64,
    pub online_mode: bool,
    pub pvp: bool,
    pub allow_flight: bool,
    pub enable_command_block: bool,
    pub white_list_enabled: bool,
    pub current_user_role: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateMinecraftServerParams<'a> {
    pub owner_user_id: &'a str,
    pub created_by_user_id: &'a str,
    pub display_name: &'a str,
    pub description: Option<&'a str>,
    pub server_distribution: &'a str,
    pub minecraft_version: &'a str,
    pub world_name: &'a str,
    pub listen_port: i64,
    pub gamemode: &'a str,
    pub difficulty: &'a str,
    pub hardcore: bool,
    pub motd: &'a str,
    pub max_player_count: Option<i64>,
    pub min_memory_mb: i64,
    pub max_memory_mb: i64,
    pub online_mode: bool,
    pub pvp: bool,
    pub allow_flight: bool,
    pub enable_command_block: bool,
    pub white_list_enabled: bool,
    pub autostart: bool,
    pub eula_accepted: bool,
}

#[derive(Debug, Clone)]
pub struct ServerInstanceEventRow {
    pub id: String,
    pub instance_id: String,
    pub job_id: Option<String>,
    pub actor_user_id: Option<String>,
    pub level: String,
    pub event_kind: String,
    pub message: String,
    pub created_ts: i64,
}

#[derive(Debug, Clone)]
pub struct UpdateMinecraftServerRuntimeParams<'a> {
    pub install_mode: Option<&'a str>,
    pub desired_state: &'a str,
    pub observed_state: &'a str,
    pub health_state: &'a str,
    pub current_player_count: i64,
    pub max_player_count: Option<i64>,
    pub last_ready_ts: Option<i64>,
    pub last_started_ts: Option<i64>,
    pub last_stopped_ts: Option<i64>,
    pub last_exit_code: Option<i64>,
    pub last_error_summary: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CreateServerInstanceEventParams<'a> {
    pub instance_id: &'a str,
    pub job_id: Option<&'a str>,
    pub actor_user_id: Option<&'a str>,
    pub level: &'a str,
    pub event_kind: &'a str,
    pub message: &'a str,
    pub details_json: Option<&'a str>,
}

fn base_instances_root() -> String {
    std::env::var("RUSTFIN_SERVERS_INSTANCE_ROOT")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/srv/rustyfin-servers/minecraft/instances".to_string())
}

fn default_java_path() -> String {
    std::env::var("RUSTFIN_SERVERS_DEFAULT_JAVA")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/bin/java".to_string())
}

fn slugify_display_name(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in value.trim().chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            slug.push(normalized);
            last_was_dash = false;
            continue;
        }
        if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn decode_minecraft_server_row(
    row: sqlx::postgres::PgRow,
) -> Result<MinecraftServerRow, sqlx::Error> {
    Ok(MinecraftServerRow {
        id: row.try_get("id")?,
        display_name: row.try_get("display_name")?,
        slug: row.try_get("slug")?,
        description: row.try_get("description")?,
        owner_user_id: row.try_get("owner_user_id")?,
        owner_display_name: row.try_get("owner_display_name")?,
        install_mode: row.try_get("install_mode")?,
        runtime_mode: row.try_get("runtime_mode")?,
        desired_state: row.try_get("desired_state")?,
        observed_state: row.try_get("observed_state")?,
        health_state: row.try_get("health_state")?,
        instance_root: row.try_get("instance_root")?,
        server_work_dir: row.try_get("server_work_dir")?,
        systemd_unit_name: row.try_get("systemd_unit_name")?,
        listen_host: row.try_get("listen_host")?,
        listen_port: row.try_get("listen_port")?,
        advertised_host: row.try_get("advertised_host")?,
        advertised_port: row.try_get("advertised_port")?,
        autostart: row.try_get("autostart")?,
        auto_stop_when_empty: row.try_get("auto_stop_when_empty")?,
        auto_stop_idle_minutes: row.try_get("auto_stop_idle_minutes")?,
        current_player_count: row.try_get("current_player_count")?,
        max_player_count: row.try_get("max_player_count")?,
        last_ready_ts: row.try_get("last_ready_ts")?,
        last_started_ts: row.try_get("last_started_ts")?,
        last_stopped_ts: row.try_get("last_stopped_ts")?,
        last_exit_code: row.try_get("last_exit_code")?,
        last_error_summary: row.try_get("last_error_summary")?,
        created_ts: row.try_get("created_ts")?,
        updated_ts: row.try_get("updated_ts")?,
        server_distribution: row.try_get("server_distribution")?,
        minecraft_version: row.try_get("minecraft_version")?,
        java_path: row.try_get("java_path")?,
        world_name: row.try_get("world_name")?,
        gamemode: row.try_get("gamemode")?,
        difficulty: row.try_get("difficulty")?,
        hardcore: row.try_get("hardcore")?,
        motd: row.try_get("motd")?,
        min_memory_mb: row.try_get("min_memory_mb")?,
        max_memory_mb: row.try_get("max_memory_mb")?,
        online_mode: row.try_get("online_mode")?,
        pvp: row.try_get("pvp")?,
        allow_flight: row.try_get("allow_flight")?,
        enable_command_block: row.try_get("enable_command_block")?,
        white_list_enabled: row.try_get("white_list_enabled")?,
        current_user_role: row.try_get("role")?,
    })
}

fn decode_server_instance_event_row(
    row: sqlx::postgres::PgRow,
) -> Result<ServerInstanceEventRow, sqlx::Error> {
    Ok(ServerInstanceEventRow {
        id: row.try_get("id")?,
        instance_id: row.try_get("instance_id")?,
        job_id: row.try_get("job_id")?,
        actor_user_id: row.try_get("actor_user_id")?,
        level: row.try_get("level")?,
        event_kind: row.try_get("event_kind")?,
        message: row.try_get("message")?,
        created_ts: row.try_get("created_ts")?,
    })
}

fn minecraft_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT \
            si.id, si.display_name, si.slug, si.description, si.owner_user_id, \
            COALESCE(NULLIF(owner.display_name, ''), owner.username) AS owner_display_name, \
            si.install_mode, si.runtime_mode, si.desired_state, si.observed_state, si.health_state, \
            si.instance_root, si.server_work_dir, si.systemd_unit_name, si.listen_host, si.listen_port, \
            si.advertised_host, si.advertised_port, si.autostart, si.auto_stop_when_empty, \
            si.auto_stop_idle_minutes, si.current_player_count, si.max_player_count, \
            si.last_ready_ts, si.last_started_ts, si.last_stopped_ts, si.last_exit_code, \
            si.last_error_summary, si.created_ts, si.updated_ts, \
            cfg.server_distribution, cfg.minecraft_version, cfg.java_path, cfg.world_name, cfg.gamemode, \
            cfg.difficulty, cfg.hardcore, cfg.motd, cfg.min_memory_mb, cfg.max_memory_mb, \
            cfg.online_mode, cfg.pvp, cfg.allow_flight, cfg.enable_command_block, cfg.white_list_enabled, member.role \
        FROM server_instance si \
        JOIN minecraft_server_config cfg ON cfg.instance_id = si.id \
        JOIN \"user\" owner ON owner.id = si.owner_user_id \
        LEFT JOIN server_instance_member member \
            ON member.instance_id = si.id AND member.user_id = $2 \
        {where_clause} \
        ORDER BY si.created_ts DESC"
    )
}

pub async fn list_accessible_minecraft_servers(
    pool: &DbPool,
    user_id: &str,
    is_admin: bool,
) -> Result<Vec<MinecraftServerRow>, sqlx::Error> {
    let sql = minecraft_select_sql(
        "WHERE si.game_kind = 'minecraft' AND ($1 OR si.owner_user_id = $2 OR member.user_id IS NOT NULL)",
    );
    let rows = sqlx::query(&sql)
        .bind(is_admin)
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(decode_minecraft_server_row).collect()
}

pub async fn get_accessible_minecraft_server(
    pool: &DbPool,
    user_id: &str,
    is_admin: bool,
    instance_id: &str,
) -> Result<Option<MinecraftServerRow>, sqlx::Error> {
    let sql = minecraft_select_sql(
        "WHERE si.game_kind = 'minecraft' \
         AND si.id = $3 \
         AND ($1 OR si.owner_user_id = $2 OR member.user_id IS NOT NULL)",
    );
    let row = sqlx::query(&sql)
        .bind(is_admin)
        .bind(user_id)
        .bind(instance_id)
        .fetch_optional(pool)
        .await?;
    row.map(decode_minecraft_server_row).transpose()
}

pub async fn get_minecraft_server_by_id(
    pool: &DbPool,
    instance_id: &str,
) -> Result<Option<MinecraftServerRow>, sqlx::Error> {
    let sql = minecraft_select_sql("WHERE si.game_kind = 'minecraft' AND si.id = $1");
    let row = sqlx::query(&sql)
        .bind(instance_id)
        .bind("")
        .fetch_optional(pool)
        .await?;
    row.map(decode_minecraft_server_row).transpose()
}

pub async fn create_minecraft_server(
    pool: &DbPool,
    params: CreateMinecraftServerParams<'_>,
) -> Result<MinecraftServerRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let short_id = &id[..8];
    let slug_base = slugify_display_name(params.display_name);
    let slug = if slug_base.is_empty() {
        format!("minecraft-{short_id}")
    } else {
        format!("{slug_base}-{short_id}")
    };
    let instance_root = format!("{}/{}", base_instances_root(), id);
    let server_work_dir = format!("{instance_root}/server");
    let systemd_unit_name = format!("rustyfin-minecraft-{short_id}.service");
    let now = chrono::Utc::now().timestamp();
    let java_path = default_java_path();
    let event_id = uuid::Uuid::new_v4().to_string();
    let description = params
        .description
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let eula_accepted_ts = if params.eula_accepted {
        Some(now)
    } else {
        None
    };
    let eula_accepted_by_user_id = if params.eula_accepted {
        Some(params.created_by_user_id)
    } else {
        None
    };

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO server_instance (
            id, game_kind, display_name, slug, description, owner_user_id, created_by_user_id,
            install_mode, runtime_mode, desired_state, observed_state, health_state,
            instance_root, server_work_dir, systemd_unit_name, listen_host, listen_port,
            advertised_host, advertised_port, autostart, auto_stop_when_empty,
            auto_stop_idle_minutes, current_player_count, max_player_count, last_ready_ts,
            last_started_ts, last_stopped_ts, last_exit_code, last_error_summary,
            created_ts, updated_ts
        ) VALUES (
            $1, 'minecraft', $2, $3, $4, $5, $6,
            'managed', 'native_systemd', 'stopped', 'draft', 'unknown',
            $7, $8, $9, '0.0.0.0', $10,
            NULL, NULL, $11, FALSE,
            NULL, 0, $12, NULL,
            NULL, NULL, NULL, NULL,
            $13, $14
        )",
    )
    .bind(&id)
    .bind(params.display_name.trim())
    .bind(&slug)
    .bind(description)
    .bind(params.owner_user_id)
    .bind(params.created_by_user_id)
    .bind(&instance_root)
    .bind(&server_work_dir)
    .bind(&systemd_unit_name)
    .bind(params.listen_port)
    .bind(params.autostart)
    .bind(params.max_player_count)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO minecraft_server_config (
            instance_id, server_distribution, minecraft_version, loader_version, java_path,
            min_memory_mb, max_memory_mb, jvm_flags_json, world_name, world_seed,
            level_type, gamemode, difficulty, hardcore, motd, online_mode, pvp,
            allow_flight, enable_command_block, view_distance, simulation_distance,
            spawn_protection, white_list_enabled, server_icon_path, eula_accepted,
            eula_accepted_by_user_id, eula_accepted_ts, created_ts, updated_ts
        ) VALUES (
            $1, $2, $3, NULL, $4,
            $5, $6, $7::jsonb, $8, NULL,
            'minecraft:normal', $9, $10, $11, $12, $13, $14,
            $15, $16, 10, 10,
            16, $17, NULL, $18,
            $19, $20, $21, $22
        )",
    )
    .bind(&id)
    .bind(params.server_distribution)
    .bind(params.minecraft_version)
    .bind(java_path)
    .bind(params.min_memory_mb)
    .bind(params.max_memory_mb)
    .bind("[]")
    .bind(params.world_name.trim())
    .bind(params.gamemode)
    .bind(params.difficulty)
    .bind(params.hardcore)
    .bind(params.motd)
    .bind(params.online_mode)
    .bind(params.pvp)
    .bind(params.allow_flight)
    .bind(params.enable_command_block)
    .bind(params.white_list_enabled)
    .bind(params.eula_accepted)
    .bind(eula_accepted_by_user_id)
    .bind(eula_accepted_ts)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO server_instance_member (
            instance_id, user_id, role, created_by_user_id, created_ts
        ) VALUES ($1, $2, 'manager', $3, $4)",
    )
    .bind(&id)
    .bind(params.owner_user_id)
    .bind(params.created_by_user_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO server_instance_event (
            id, instance_id, job_id, actor_user_id, level, event_kind, message, details_json, created_ts
        ) VALUES (
            $1, $2, NULL, $3, 'info', 'draft_created', $4, $5::jsonb, $6
        )",
    )
    .bind(event_id)
    .bind(&id)
    .bind(params.created_by_user_id)
    .bind(format!(
        "Draft server created for {} {}.",
        params.server_distribution.trim(),
        params.minecraft_version.trim()
    ))
    .bind("{}")
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    get_accessible_minecraft_server(pool, params.owner_user_id, true, &id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn list_server_instance_events(
    pool: &DbPool,
    instance_id: &str,
    limit: i64,
) -> Result<Vec<ServerInstanceEventRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, instance_id, job_id, actor_user_id, level, event_kind, message, created_ts
             FROM server_instance_event
             WHERE instance_id = $1
             ORDER BY created_ts DESC
             LIMIT $2",
    )
    .bind(instance_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(decode_server_instance_event_row)
        .collect()
}

pub async fn update_minecraft_server_runtime(
    pool: &DbPool,
    instance_id: &str,
    params: UpdateMinecraftServerRuntimeParams<'_>,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE server_instance
         SET install_mode = COALESCE($1, install_mode),
             desired_state = $2,
             observed_state = $3,
             health_state = $4,
             current_player_count = $5,
             max_player_count = COALESCE($6, max_player_count),
             last_ready_ts = $7,
             last_started_ts = $8,
             last_stopped_ts = $9,
             last_exit_code = $10,
             last_error_summary = $11,
             updated_ts = $12
         WHERE id = $13",
    )
    .bind(params.install_mode)
    .bind(params.desired_state)
    .bind(params.observed_state)
    .bind(params.health_state)
    .bind(params.current_player_count)
    .bind(params.max_player_count)
    .bind(params.last_ready_ts)
    .bind(params.last_started_ts)
    .bind(params.last_stopped_ts)
    .bind(params.last_exit_code)
    .bind(params.last_error_summary)
    .bind(now)
    .bind(instance_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn create_server_instance_event(
    pool: &DbPool,
    params: CreateServerInstanceEventParams<'_>,
) -> Result<ServerInstanceEventRow, sqlx::Error> {
    let row_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO server_instance_event (
            id, instance_id, job_id, actor_user_id, level, event_kind, message, details_json, created_ts
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9)",
    )
    .bind(&row_id)
    .bind(params.instance_id)
    .bind(params.job_id)
    .bind(params.actor_user_id)
    .bind(params.level)
    .bind(params.event_kind)
    .bind(params.message)
    .bind(params.details_json.unwrap_or("{}"))
    .bind(now)
    .execute(pool)
    .await?;

    Ok(ServerInstanceEventRow {
        id: row_id,
        instance_id: params.instance_id.to_string(),
        job_id: params.job_id.map(str::to_string),
        actor_user_id: params.actor_user_id.map(str::to_string),
        level: params.level.to_string(),
        event_kind: params.event_kind.to_string(),
        message: params.message.to_string(),
        created_ts: now,
    })
}

pub async fn delete_minecraft_server(
    pool: &DbPool,
    instance_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM server_instance
         WHERE id = $1
           AND game_kind = 'minecraft'",
    )
    .bind(instance_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
