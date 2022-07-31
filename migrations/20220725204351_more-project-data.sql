-- Add migration script here
ALTER TABLE mods_categories
    ADD COLUMN is_additional BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE mods
    ADD COLUMN approved timestamptz NULL;

UPDATE mods
    SET approved = published
    WHERE status = 'approved' OR status = 'unlisted';

CREATE INDEX mods_slug
    ON mods (slug);

CREATE INDEX versions_mod_id
    ON versions (mod_id);

CREATE INDEX files_version_id
    ON files (version_id);

CREATE INDEX dependencies_dependent_id
    ON dependencies (dependent_id);

CREATE INDEX mods_gallery_mod_id
    ON mods_gallery(mod_id);

CREATE INDEX game_versions_versions_joining_version_id
    ON game_versions_versions(joining_version_id);

CREATE INDEX loaders_versions_version_id
    ON loaders_versions(version_id);

CREATE INDEX notifications_user_id
    ON notifications(user_id);