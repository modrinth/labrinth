-- Add migration script here
ALTER TABLE users ADD COLUMN issues_url varchar(2048) NULL;
ALTER TABLE users ADD COLUMN source_url varchar(2048) NULL;
ALTER TABLE users ADD COLUMN wiki_url varchar(2048) NULL;
ALTER TABLE users ADD COLUMN discord_url varchar(2048) NULL;
