DROP INDEX events_targets;
DROP INDEX events_triggerers;
DROP TABLE events;
DROP TYPE dynamic_id;
DROP TYPE event_type;
DROP TYPE id_type;
DELETE FROM _sqlx_migrations
WHERE version = 20231023212957;