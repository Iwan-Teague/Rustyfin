-- PostgreSQL strict typing: Rust decodes these as f64.
-- Convert legacy REAL columns to DOUBLE PRECISION.
ALTER TABLE job
    ALTER COLUMN progress TYPE DOUBLE PRECISION
    USING progress::double precision;

ALTER TABLE item
    ALTER COLUMN community_rating TYPE DOUBLE PRECISION
    USING community_rating::double precision;
