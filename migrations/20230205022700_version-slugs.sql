ALTER TABLE versions ADD COLUMN slug varchar(255) NULL;

-- TODO: add version slugs for existing versions

ALTER TABLE versions
    ADD UNIQUE (mod_id, slug);

ALTER TABLE versions ALTER COLUMN slug SET NOT NULL;