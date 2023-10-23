CREATE TYPE event_type AS ENUM ('project_created');
CREATE TYPE id_type AS ENUM ('project_id', 'user_id', 'organization_id');
CREATE TYPE dynamic_id AS (id bigint, type id_type);
CREATE TABLE events(
    id bigint NOT NULL PRIMARY KEY,
    target_id dynamic_id NOT NULL,
    triggerer_id dynamic_id NULL,
    type event_type NOT NULL,
    metadata jsonb NULL,
    created timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);