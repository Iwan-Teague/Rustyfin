ALTER TABLE rustyvault_preference
    ADD COLUMN IF NOT EXISTS password_generator_default_preset TEXT NOT NULL DEFAULT 'balanced',
    ADD COLUMN IF NOT EXISTS password_generator_default_length INTEGER NOT NULL DEFAULT 22,
    ADD COLUMN IF NOT EXISTS password_generator_include_uppercase BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS password_generator_include_lowercase BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS password_generator_include_numbers BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS password_generator_include_symbols BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS password_generator_exclude_ambiguous BOOLEAN NOT NULL DEFAULT TRUE;
