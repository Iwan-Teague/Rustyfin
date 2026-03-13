ALTER TABLE IF EXISTS vault_account RENAME TO rustyvault_account;
ALTER TABLE IF EXISTS vault_wrapped_key RENAME TO rustyvault_wrapped_key;
ALTER TABLE IF EXISTS vault_item RENAME TO rustyvault_item;
ALTER TABLE IF EXISTS vault_item_uri_index RENAME TO rustyvault_item_uri_index;
ALTER TABLE IF EXISTS vault_device_session RENAME TO rustyvault_device_session;
ALTER TABLE IF EXISTS vault_device_session_refresh_token RENAME TO rustyvault_device_session_refresh_token;
ALTER TABLE IF EXISTS vault_pending_device_approval RENAME TO rustyvault_pending_device_approval;
ALTER TABLE IF EXISTS vault_protected_action_token RENAME TO rustyvault_protected_action_token;
ALTER TABLE IF EXISTS vault_audit_event RENAME TO rustyvault_audit_event;

ALTER INDEX IF EXISTS idx_vault_wrapped_key_user_active
    RENAME TO idx_rustyvault_wrapped_key_user_active;
ALTER INDEX IF EXISTS idx_vault_item_user_updated
    RENAME TO idx_rustyvault_item_user_updated;
ALTER INDEX IF EXISTS idx_vault_item_user_deleted
    RENAME TO idx_rustyvault_item_user_deleted;
ALTER INDEX IF EXISTS idx_vault_item_uri_index_lookup
    RENAME TO idx_rustyvault_item_uri_index_lookup;
ALTER INDEX IF EXISTS idx_vault_device_session_user_last_used
    RENAME TO idx_rustyvault_device_session_user_last_used;
ALTER INDEX IF EXISTS idx_vault_device_session_refresh_family
    RENAME TO idx_rustyvault_device_session_refresh_family;
ALTER INDEX IF EXISTS idx_vault_pending_device_approval_user_created
    RENAME TO idx_rustyvault_pending_device_approval_user_created;
ALTER INDEX IF EXISTS idx_vault_protected_action_user_kind
    RENAME TO idx_rustyvault_protected_action_user_kind;
ALTER INDEX IF EXISTS idx_vault_audit_event_user_created
    RENAME TO idx_rustyvault_audit_event_user_created;
ALTER INDEX IF EXISTS idx_vault_refresh_token_session
    RENAME TO idx_rustyvault_refresh_token_session;
ALTER INDEX IF EXISTS idx_vault_refresh_token_user
    RENAME TO idx_rustyvault_refresh_token_user;
ALTER INDEX IF EXISTS idx_vault_refresh_token_family
    RENAME TO idx_rustyvault_refresh_token_family;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'vault_item_uri_index_item_fk'
    ) THEN
        ALTER TABLE rustyvault_item_uri_index
            RENAME CONSTRAINT vault_item_uri_index_item_fk TO rustyvault_item_uri_index_item_fk;
    END IF;
END $$;
