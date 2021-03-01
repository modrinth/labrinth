CREATE TABLE mod_follows(
    id serial PRIMARY KEY,
    follower_id bigint REFERENCES users NOT NULL,
    mod_id bigint REFERENCES mods NOT NULL,
    created timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL
);

ALTER TABLE mods
    ADD COLUMN follows integer NOT NULL default 0;