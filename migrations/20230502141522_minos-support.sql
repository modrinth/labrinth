-- Change banned_users to use username instead of github_id
ALTER TABLE banned_users ADD COLUMN user_id BIGINT UNIQUE;
UPDATE banned_users SET user_id = users.id FROM users WHERE banned_users.github_id = users.github_id;
ALTER TABLE banned_users DROP COLUMN github_id;

-- Initialize kratos_id 
ALTER TABLE users ADD COLUMN kratos_id varchar(40) UNIQUE;

-- Add pats table
CREATE TABLE pats (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    access_token VARCHAR(64) NOT NULL,
    scope VARCHAR(255) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL
);

-- Drop github_id from users table (offloaded to Minos)
ALTER TABLE users ALTER COLUMN github_id DROP NOT NULL;