DO $do$
DECLARE
    r RECORD;
BEGIN
    FOR r IN
        SELECT table_schema, table_name, column_name
        FROM information_schema.columns
        WHERE table_schema = ANY(current_schemas(false))
          AND data_type = 'integer'
    LOOP
        EXECUTE format(
            'ALTER TABLE %I.%I ALTER COLUMN %I TYPE BIGINT USING %I::BIGINT',
            r.table_schema,
            r.table_name,
            r.column_name,
            r.column_name
        );
    END LOOP;
END
$do$;
