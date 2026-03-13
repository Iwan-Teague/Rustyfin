CREATE TABLE IF NOT EXISTS rustyvault_preference (
    user_id TEXT PRIMARY KEY REFERENCES "user"(id) ON DELETE CASCADE,
    auto_lock_minutes INTEGER NOT NULL DEFAULT 15,
    clipboard_clear_seconds INTEGER NOT NULL DEFAULT 30,
    inline_save_prompt_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    inline_autofill_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    default_match_mode TEXT NOT NULL DEFAULT 'base_domain',
    warn_on_http BOOLEAN NOT NULL DEFAULT TRUE,
    warn_on_untrusted_iframe BOOLEAN NOT NULL DEFAULT TRUE,
    excluded_domains TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    allow_manual_http_fill BOOLEAN NOT NULL DEFAULT FALSE,
    updated_ts BIGINT NOT NULL
);

INSERT INTO rustyvault_preference (
    user_id,
    auto_lock_minutes,
    clipboard_clear_seconds,
    inline_save_prompt_enabled,
    inline_autofill_enabled,
    default_match_mode,
    warn_on_http,
    warn_on_untrusted_iframe,
    excluded_domains,
    allow_manual_http_fill,
    updated_ts
)
SELECT
    user_id,
    COALESCE((json::jsonb -> 'vault' ->> 'auto_lock_minutes')::INTEGER, 15),
    COALESCE((json::jsonb -> 'vault' ->> 'clipboard_clear_seconds')::INTEGER, 30),
    COALESCE((json::jsonb -> 'vault' ->> 'inline_save_prompt_enabled')::BOOLEAN, TRUE),
    COALESCE((json::jsonb -> 'vault' ->> 'inline_autofill_enabled')::BOOLEAN, TRUE),
    COALESCE(json::jsonb -> 'vault' ->> 'default_match_mode', 'base_domain'),
    COALESCE((json::jsonb -> 'vault' ->> 'warn_on_http')::BOOLEAN, TRUE),
    COALESCE((json::jsonb -> 'vault' ->> 'warn_on_untrusted_iframe')::BOOLEAN, TRUE),
    CASE
        WHEN jsonb_typeof(json::jsonb -> 'vault' -> 'excluded_domains') = 'array' THEN
            ARRAY(
                SELECT jsonb_array_elements_text(json::jsonb -> 'vault' -> 'excluded_domains')
            )
        ELSE ARRAY[]::TEXT[]
    END,
    COALESCE((json::jsonb -> 'vault' ->> 'allow_manual_http_fill')::BOOLEAN, FALSE),
    updated_ts
FROM user_pref
WHERE json::jsonb ? 'vault'
ON CONFLICT (user_id) DO NOTHING;

UPDATE user_pref
SET json = (json::jsonb - 'vault')::TEXT
WHERE json::jsonb ? 'vault';
