CREATE INDEX IF NOT EXISTS idx_user_item_state_continue_watch
    ON user_item_state(user_id, last_played_ts DESC)
    WHERE played = 0 AND progress_ms > 0;
