CREATE TABLE user_follows(
                            follower_id bigint REFERENCES users NOT NULL,
                            user_id bigint REFERENCES mods NOT NULL,
                            created timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL,
                            PRIMARY KEY (follower_id, user_id)
);

ALTER TABLE users
    ADD COLUMN follows integer NOT NULL default 0;