ALTER TABLE "user"
ADD COLUMN display_name TEXT;

ALTER TABLE "user"
ADD COLUMN avatar_path TEXT;

ALTER TABLE "user"
ADD COLUMN avatar_content_type TEXT;

UPDATE "user"
SET display_name = username
WHERE display_name IS NULL OR TRIM(display_name) = '';
