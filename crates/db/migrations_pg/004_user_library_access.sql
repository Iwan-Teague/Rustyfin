-- Per-user library visibility.
-- Admin users are not constrained by this table because role-based checks in the API grant full access.
CREATE TABLE IF NOT EXISTS user_library_access (
    user_id TEXT NOT NULL REFERENCES user(id) ON DELETE CASCADE,
    library_id TEXT NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    created_ts INTEGER NOT NULL,
    PRIMARY KEY(user_id, library_id)
);

CREATE INDEX IF NOT EXISTS idx_user_library_access_library
    ON user_library_access(library_id);
