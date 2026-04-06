ALTER TABLE rustyvault_account
    ADD COLUMN IF NOT EXISTS display_name TEXT;

UPDATE rustyvault_account
SET display_name = 'Personal Vault'
WHERE display_name IS NULL OR btrim(display_name) = '';
