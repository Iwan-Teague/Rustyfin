use crate::DbPool;

#[derive(Debug, Clone)]
pub struct CalendarEventRow {
    pub id: String,
    pub scope: String,
    pub owner_user_id: Option<String>,
    pub owner_username: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub event_date: String,
    pub event_type: String,
    pub recurrence: String,
    pub birthday_year: Option<i32>,
    pub created_by_user_id: String,
    pub created_by_username: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

#[derive(Debug, Clone)]
pub struct NewCalendarEvent {
    pub scope: String,
    pub owner_user_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub event_date: String,
    pub event_type: String,
    pub recurrence: String,
    pub birthday_year: Option<i32>,
    pub created_by_user_id: String,
}

#[derive(Debug, Clone)]
pub struct UpdateCalendarEvent {
    pub scope: String,
    pub owner_user_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub event_date: String,
    pub event_type: String,
    pub recurrence: String,
    pub birthday_year: Option<i32>,
}

fn map_row(
    row: (
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<i32>,
        String,
        Option<String>,
        i64,
        i64,
    ),
) -> CalendarEventRow {
    let (
        id,
        scope,
        owner_user_id,
        owner_username,
        title,
        description,
        event_date,
        event_type,
        recurrence,
        birthday_year,
        created_by_user_id,
        created_by_username,
        created_ts,
        updated_ts,
    ) = row;

    CalendarEventRow {
        id,
        scope,
        owner_user_id,
        owner_username,
        title,
        description,
        event_date,
        event_type,
        recurrence,
        birthday_year,
        created_by_user_id,
        created_by_username,
        created_ts,
        updated_ts,
    }
}

pub async fn create_event(
    pool: &DbPool,
    new_event: &NewCalendarEvent,
) -> Result<CalendarEventRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO calendar_event \
         (id, scope, owner_user_id, title, description, event_date, event_type, recurrence, birthday_year, created_by_user_id, created_ts, updated_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(&id)
    .bind(&new_event.scope)
    .bind(&new_event.owner_user_id)
    .bind(&new_event.title)
    .bind(&new_event.description)
    .bind(&new_event.event_date)
    .bind(&new_event.event_type)
    .bind(&new_event.recurrence)
    .bind(new_event.birthday_year)
    .bind(&new_event.created_by_user_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    // Safe unwrap because the row was just inserted.
    Ok(get_event(pool, &id)
        .await?
        .expect("created calendar event must exist"))
}

pub async fn get_event(
    pool: &DbPool,
    event_id: &str,
) -> Result<Option<CalendarEventRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<i32>,
        String,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT e.id, e.scope, e.owner_user_id, owner.username, e.title, e.description, \
                e.event_date, e.event_type, e.recurrence, e.birthday_year, \
                e.created_by_user_id, creator.username, e.created_ts, e.updated_ts \
         FROM calendar_event e \
         LEFT JOIN user owner ON owner.id = e.owner_user_id \
         LEFT JOIN user creator ON creator.id = e.created_by_user_id \
         WHERE e.id = $1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_row))
}

pub async fn update_event(
    pool: &DbPool,
    event_id: &str,
    patch: &UpdateCalendarEvent,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE calendar_event \
         SET scope = $1, owner_user_id = $2, title = $3, description = $4, event_date = $5, \
             event_type = $6, recurrence = $7, birthday_year = $8, updated_ts = $9 \
         WHERE id = $10",
    )
    .bind(&patch.scope)
    .bind(&patch.owner_user_id)
    .bind(&patch.title)
    .bind(&patch.description)
    .bind(&patch.event_date)
    .bind(&patch.event_type)
    .bind(&patch.recurrence)
    .bind(patch.birthday_year)
    .bind(now)
    .bind(event_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn delete_event(pool: &DbPool, event_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM calendar_event WHERE id = $1")
        .bind(event_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_visible_events(
    pool: &DbPool,
    user_id: &str,
    is_admin: bool,
    from_date: &str,
    to_date: &str,
) -> Result<Vec<CalendarEventRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<i32>,
        String,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT e.id, e.scope, e.owner_user_id, owner.username, e.title, e.description, \
                e.event_date, e.event_type, e.recurrence, e.birthday_year, \
                e.created_by_user_id, creator.username, e.created_ts, e.updated_ts \
         FROM calendar_event e \
         LEFT JOIN user owner ON owner.id = e.owner_user_id \
         LEFT JOIN user creator ON creator.id = e.created_by_user_id \
         WHERE (
             e.scope = 'global'
             OR ($1 = 1)
             OR (e.scope = 'personal' AND e.owner_user_id = $2)
         ) \
           AND (
             (e.recurrence = 'none' AND e.event_date >= $3 AND e.event_date <= $4)
             OR e.recurrence = 'yearly'
           ) \
         ORDER BY e.event_date ASC, e.title ASC, e.created_ts ASC",
    )
    .bind(if is_admin { 1 } else { 0 })
    .bind(user_id)
    .bind(from_date)
    .bind(to_date)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(map_row).collect())
}

pub async fn list_personal_events(
    pool: &DbPool,
    from_date: &str,
    to_date: &str,
) -> Result<Vec<CalendarEventRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<i32>,
        String,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT e.id, e.scope, e.owner_user_id, owner.username, e.title, e.description, \
                e.event_date, e.event_type, e.recurrence, e.birthday_year, \
                e.created_by_user_id, creator.username, e.created_ts, e.updated_ts \
         FROM calendar_event e \
         LEFT JOIN user owner ON owner.id = e.owner_user_id \
         LEFT JOIN user creator ON creator.id = e.created_by_user_id \
         WHERE e.scope = 'personal'
           AND (
             (e.recurrence = 'none' AND e.event_date >= $1 AND e.event_date <= $2)
             OR e.recurrence = 'yearly'
           ) \
         ORDER BY e.event_date ASC, e.title ASC, e.created_ts ASC",
    )
    .bind(from_date)
    .bind(to_date)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(map_row).collect())
}
