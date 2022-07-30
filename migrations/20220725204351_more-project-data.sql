-- Add migration script here
ALTER TABLE mods_categories
    ADD COLUMN is_additional BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE mods
    ADD COLUMN approved timestamptz NULL;
