CREATE TABLE events(
    id bigint NOT NULL PRIMARY KEY,
    target_id bigint NOT NULL,
    target_id_type text NOT NULL,
    triggerer_id bigint NULL,
    triggerer_id_type text NULL,
    event_type text NOT NULL,
    metadata jsonb NULL,
    created timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX events_targets ON events (
    target_id,
    target_id_type,
    event_type
);
CREATE INDEX events_triggerers ON events (
    triggerer_id,
    triggerer_id_type,
    event_type
);