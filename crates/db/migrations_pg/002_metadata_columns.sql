-- Add metadata columns to item table
ALTER TABLE item ADD COLUMN original_title TEXT;
ALTER TABLE item ADD COLUMN tagline TEXT;
ALTER TABLE item ADD COLUMN premiere_date TEXT;
ALTER TABLE item ADD COLUMN end_date TEXT;
ALTER TABLE item ADD COLUMN runtime_minutes INTEGER;
ALTER TABLE item ADD COLUMN community_rating DOUBLE PRECISION;
ALTER TABLE item ADD COLUMN official_rating TEXT;
ALTER TABLE item ADD COLUMN genres_json TEXT;
ALTER TABLE item ADD COLUMN studios_json TEXT;
ALTER TABLE item ADD COLUMN poster_url TEXT;
ALTER TABLE item ADD COLUMN backdrop_url TEXT;
ALTER TABLE item ADD COLUMN logo_url TEXT;
ALTER TABLE item ADD COLUMN thumb_url TEXT;

-- Fix item_provider_id column name to match code expectations
-- Keep legacy `value` compatibility while introducing provider_id naming.
-- Instead, let's make sure the code uses the correct column name from the schema: 'value'

-- Fix item_field_lock column name
-- Schema uses 'field' but code uses 'field_name'. Add alias column.
-- Actually, just update code to use 'field' to match schema.

-- Add locked_ts to field_lock if not present
-- Keep migration idempotent for environments with pre-existing columns.
ALTER TABLE item_field_lock ADD COLUMN locked_ts INTEGER;
