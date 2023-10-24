CREATE TYPE event_type AS ENUM ('project_created');
CREATE TYPE id_type AS ENUM ('project_id', 'user_id', 'organization_id');
CREATE TYPE dynamic_id AS (id bigint, id_type id_type);
CREATE TABLE events(
    id bigint NOT NULL PRIMARY KEY,
    target_id dynamic_id NOT NULL,
    triggerer_id dynamic_id NULL,
    event_type event_type NOT NULL,
    metadata jsonb NULL,
    created timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX events_targets ON events (
    ((target_id).id),
    ((target_id).id_type),
    event_type
);
CREATE INDEX events_triggerers ON events (
    ((triggerer_id).id),
    ((triggerer_id).id_type),
    event_type
);