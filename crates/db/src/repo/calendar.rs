use crate::DbPool;
use chrono::{Datelike, NaiveDate};

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

#[derive(Debug, Clone)]
pub struct NextVisibleCalendarEventRow {
    pub event: CalendarEventRow,
    pub next_occurs_on: String,
}

fn invalid_calendar_date_error(raw: &str) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("calendar event_date {raw} is not valid YYYY-MM-DD"),
    )))
}

fn parse_calendar_date(raw: &str) -> Result<NaiveDate, sqlx::Error> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| invalid_calendar_date_error(raw))
}

fn with_year_safe(date: NaiveDate, year: i32) -> Option<NaiveDate> {
    if let Some(updated) = date.with_year(year) {
        return Some(updated);
    }
    if date.month() == 2 && date.day() == 29 {
        return NaiveDate::from_ymd_opt(year, 2, 28);
    }
    None
}

fn next_occurrence_on_or_after(
    row: &CalendarEventRow,
    on_or_after: NaiveDate,
) -> Result<Option<NaiveDate>, sqlx::Error> {
    let source_date = parse_calendar_date(&row.event_date)?;
    if row.recurrence != "yearly" {
        return Ok((source_date >= on_or_after).then_some(source_date));
    }

    let current_year = on_or_after.year();
    let current_year_occurrence = with_year_safe(source_date, current_year);
    if let Some(candidate) = current_year_occurrence {
        if candidate >= on_or_after {
            return Ok(Some(candidate));
        }
    }
    Ok(with_year_safe(source_date, current_year + 1))
}

fn next_event_scope_rank(scope: &str) -> u8 {
    match scope {
        "personal" => 0,
        "global" => 1,
        _ => 2,
    }
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
        Option<i64>,
        String,
        Option<String>,
        i64,
        i64,
    ),
) -> Result<CalendarEventRow, sqlx::Error> {
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

    let birthday_year = birthday_year
        .map(|year| {
            i32::try_from(year).map_err(|_| {
                sqlx::Error::Decode(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("calendar birthday_year {year} is out of i32 range"),
                )))
            })
        })
        .transpose()?;

    Ok(CalendarEventRow {
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
    })
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

    get_event(pool, &id).await?.ok_or(sqlx::Error::RowNotFound)
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
        Option<i64>,
        String,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT e.id, e.scope, e.owner_user_id, owner.username, e.title, e.description, \
                e.event_date, e.event_type, e.recurrence, e.birthday_year, \
                e.created_by_user_id, creator.username, e.created_ts, e.updated_ts \
         FROM calendar_event e \
         LEFT JOIN \"user\" owner ON owner.id = e.owner_user_id \
         LEFT JOIN \"user\" creator ON creator.id = e.created_by_user_id \
         WHERE e.id = $1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await?;

    row.map(map_row).transpose()
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
        Option<i64>,
        String,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT e.id, e.scope, e.owner_user_id, owner.username, e.title, e.description, \
                e.event_date, e.event_type, e.recurrence, e.birthday_year, \
                e.created_by_user_id, creator.username, e.created_ts, e.updated_ts \
         FROM calendar_event e \
         LEFT JOIN \"user\" owner ON owner.id = e.owner_user_id \
         LEFT JOIN \"user\" creator ON creator.id = e.created_by_user_id \
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

    rows.into_iter().map(map_row).collect()
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
        Option<i64>,
        String,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT e.id, e.scope, e.owner_user_id, owner.username, e.title, e.description, \
                e.event_date, e.event_type, e.recurrence, e.birthday_year, \
                e.created_by_user_id, creator.username, e.created_ts, e.updated_ts \
         FROM calendar_event e \
         LEFT JOIN \"user\" owner ON owner.id = e.owner_user_id \
         LEFT JOIN \"user\" creator ON creator.id = e.created_by_user_id \
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

    rows.into_iter().map(map_row).collect()
}

pub async fn find_next_visible_event(
    pool: &DbPool,
    user_id: &str,
    is_admin: bool,
    on_or_after: NaiveDate,
) -> Result<Option<NextVisibleCalendarEventRow>, sqlx::Error> {
    let from_date = on_or_after.format("%F").to_string();
    let events = list_visible_events(pool, user_id, is_admin, &from_date, "9999-12-31").await?;

    let mut candidates = events
        .into_iter()
        .filter_map(|event| {
            next_occurrence_on_or_after(&event, on_or_after)
                .transpose()
                .map(|next_occurs_on| {
                    next_occurs_on.map(|next_occurs_on| NextVisibleCalendarEventRow {
                        next_occurs_on: next_occurs_on.format("%F").to_string(),
                        event,
                    })
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    candidates.sort_by(|left, right| {
        left.next_occurs_on
            .cmp(&right.next_occurs_on)
            .then_with(|| {
                next_event_scope_rank(&left.event.scope)
                    .cmp(&next_event_scope_rank(&right.event.scope))
            })
            .then_with(|| left.event.title.cmp(&right.event.title))
            .then_with(|| left.event.id.cmp(&right.event.id))
    });

    Ok(candidates.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::{map_row, next_occurrence_on_or_after};
    use chrono::NaiveDate;

    #[test]
    fn map_row_accepts_bigint_birthday_year() {
        let row = map_row((
            "event-1".to_string(),
            "personal".to_string(),
            Some("user-1".to_string()),
            Some("alpha".to_string()),
            "Birthday".to_string(),
            None,
            "2026-03-12".to_string(),
            "birthday".to_string(),
            "yearly".to_string(),
            Some(1990_i64),
            "user-1".to_string(),
            Some("alpha".to_string()),
            1,
            2,
        ))
        .expect("bigint birthday year should decode");

        assert_eq!(row.birthday_year, Some(1990));
    }

    #[test]
    fn map_row_rejects_out_of_range_birthday_year() {
        let err = map_row((
            "event-1".to_string(),
            "personal".to_string(),
            Some("user-1".to_string()),
            Some("alpha".to_string()),
            "Birthday".to_string(),
            None,
            "2026-03-12".to_string(),
            "birthday".to_string(),
            "yearly".to_string(),
            Some(i64::from(i32::MAX) + 1),
            "user-1".to_string(),
            Some("alpha".to_string()),
            1,
            2,
        ))
        .expect_err("out-of-range birthday year should fail");

        assert!(matches!(err, sqlx::Error::Decode(_)));
    }

    #[test]
    fn next_occurrence_uses_next_year_for_past_yearly_event() {
        let row = map_row((
            "event-1".to_string(),
            "personal".to_string(),
            Some("user-1".to_string()),
            Some("alpha".to_string()),
            "Birthday".to_string(),
            None,
            "2020-03-12".to_string(),
            "birthday".to_string(),
            "yearly".to_string(),
            Some(1990_i64),
            "user-1".to_string(),
            Some("alpha".to_string()),
            1,
            2,
        ))
        .expect("calendar row should decode");

        let next = next_occurrence_on_or_after(
            &row,
            NaiveDate::from_ymd_opt(2026, 4, 1).expect("valid date"),
        )
        .expect("next occurrence should compute");

        assert_eq!(
            next,
            Some(NaiveDate::from_ymd_opt(2027, 3, 12).expect("valid date"))
        );
    }

    #[test]
    fn next_occurrence_keeps_future_one_off_event() {
        let row = map_row((
            "event-2".to_string(),
            "global".to_string(),
            None,
            None,
            "Trip".to_string(),
            None,
            "2026-06-09".to_string(),
            "event".to_string(),
            "none".to_string(),
            None,
            "user-1".to_string(),
            Some("alpha".to_string()),
            1,
            2,
        ))
        .expect("calendar row should decode");

        let next = next_occurrence_on_or_after(
            &row,
            NaiveDate::from_ymd_opt(2026, 4, 1).expect("valid date"),
        )
        .expect("next occurrence should compute");

        assert_eq!(
            next,
            Some(NaiveDate::from_ymd_opt(2026, 6, 9).expect("valid date"))
        );
    }
}
