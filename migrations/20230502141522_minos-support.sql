-- Change banned_users to use username instead of github_id
ALTER TABLE banned_users ADD COLUMN username varchar(255) UNIQUE;
UPDATE banned_users SET username = users.username FROM users WHERE banned_users.github_id = users.github_id;
ALTER TABLE banned_users DROP COLUMN github_id;


-- Initialize kratos_id for existing users, starting as unique 'uninitialized_<username>'
-- After account porting, this will be set to the kratos_id and no record should have 'uninitialized_' prefix
ALTER TABLE users ADD COLUMN kratos_id varchar(255);
UPDATE users SET kratos_id = 'uninitialized_' || username;
ALTER TABLE users ALTER COLUMN kratos_id SET NOT NULL;
ALTER TABLE users ADD CONSTRAINT kratos_id_unique UNIQUE (kratos_id);


