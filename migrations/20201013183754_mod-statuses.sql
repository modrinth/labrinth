-- Add migration script here
CREATE TABLE statuses (
    id bigint PRIMARY KEY,
    status varchar(500)
);

ALTER TABLE mods
ADD COLUMN status bigint REFERENCES statuses NOT NULL;
ALTER TABLE mods
ADD COLUMN updated timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP;