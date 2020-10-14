-- Add migration script here
ALTER TABLE statuses
ALTER COLUMN id TYPE INT;

INSERT INTO statuses (id, status) VALUES (5, 'processing');