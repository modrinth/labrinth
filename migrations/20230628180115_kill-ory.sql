ALTER TABLE users DROP COLUMN kratos_id;

ALTER TABLE states ADD COLUMN provider varchar(64) NOT NULL default 'github';

ALTER TABLE users ADD COLUMN discord_id bigint;
ALTER TABLE users ADD COLUMN gitlab_id bigint;
ALTER TABLE users ADD COLUMN google_id varchar(256);
ALTER TABLE users ADD COLUMN steam_id bigint;
ALTER TABLE users ADD COLUMN microsoft_id varchar(256);

ALTER TABLE users ALTER COLUMN avatar_url TYPE varchar(1024);
-- TODO: add password, whether email is verified or not
