
CREATE TABLE IF NOT EXISTS calendar_event (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL CHECK (scope IN ('global', 'personal')),
    owner_user_id TEXT REFERENCES "user"(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    event_date TEXT NOT NULL,
    event_type TEXT NOT NULL DEFAULT 'event' CHECK (event_type IN ('event', 'birthday')),
    recurrence TEXT NOT NULL DEFAULT 'none' CHECK (recurrence IN ('none', 'yearly')),
    birthday_year INTEGER,
    created_by_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL,
    CHECK (
      (scope = 'global' AND owner_user_id IS NULL)
      OR (scope = 'personal' AND owner_user_id IS NOT NULL)
    ),
    CHECK (
      (event_type = 'birthday' AND recurrence = 'yearly' AND birthday_year IS NOT NULL)
      OR event_type = 'event'
    ),
    CHECK (length(trim(title)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_calendar_event_scope_date
    ON calendar_event (scope, event_date);

CREATE INDEX IF NOT EXISTS idx_calendar_event_owner_date
    ON calendar_event (owner_user_id, event_date);

CREATE INDEX IF NOT EXISTS idx_calendar_event_recurrence
    ON calendar_event (recurrence, event_date);
