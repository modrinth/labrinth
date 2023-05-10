-- Change banned_users to use username instead of github_id
ALTER TABLE banned_users ADD COLUMN user_id BIGINT UNIQUE;
UPDATE banned_users SET user_id = users.id FROM users WHERE banned_users.github_id = users.github_id;
ALTER TABLE banned_users DROP COLUMN github_id;


-- Initialize kratos_id for existing users, starting as unique 'uninitialized_<user id>'
-- After account porting, this will be set to the kratos_id and NO record should have 'uninitialized_' prefix
ALTER TABLE users ADD COLUMN kratos_id varchar(255);
UPDATE users SET kratos_id = 'uninitialized_' || id;
ALTER TABLE users ALTER COLUMN kratos_id SET NOT NULL;
ALTER TABLE users ADD CONSTRAINT kratos_id_unique UNIQUE (kratos_id);

-- Add pats table
CREATE TABLE pats (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    access_token BIGINT NOT NULL,
    scope VARCHAR(255) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL
);