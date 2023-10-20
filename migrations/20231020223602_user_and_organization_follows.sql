CREATE TABLE user_follows(
    follower_id bigint NOT NULL REFERENCES users ON DELETE CASCADE,
    target_id bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL,
    PRIMARY KEY (follower_id, target_id)
);
CREATE TABLE organization_follows(
    follower_id bigint NOT NULL REFERENCES users ON DELETE CASCADE,
    target_id bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    created timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL,
    PRIMARY KEY (follower_id, target_id)
);