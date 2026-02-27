ALTER TABLE "user"
ADD COLUMN IF NOT EXISTS display_name TEXT;

ALTER TABLE "user"
ADD COLUMN IF NOT EXISTS avatar_path TEXT;

ALTER TABLE "user"
ADD COLUMN IF NOT EXISTS avatar_content_type TEXT;

UPDATE "user"
SET display_name = username
WHERE display_name IS NULL OR BTRIM(display_name) = '';
