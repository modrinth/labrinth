CREATE TABLE collections (
    id bigint PRIMARY KEY,
    title varchar(255) NOT NULL,
    description varchar(2048) NOT NULL,
    team_id bigint REFERENCES teams NOT NULL,
    published timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    slug varchar(255) NULL UNIQUE,
    body varchar(65536) NOT NULL DEFAULT ''::varchar,

    public boolean NOT NULL DEFAULT false,

    icon_url varchar(2048) NULL,
    color integer NULL
);

CREATE TABLE collections_mods (
    collection_id bigint REFERENCES collections NOT NULL,
    mod_id bigint REFERENCES mods NOT NULL,
    PRIMARY KEY (collection_id, mod_id)
);

CREATE TABLE uploaded_images (
    id bigint PRIMARY KEY,
    url varchar(2048) NOT NULL,
    size integer NOT NULL,
    created timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    owner_id bigint REFERENCES users NOT NULL,

    -- Image will be associated with a mod (in description) or a thread message (not both)
    -- Icons are not a part of this table
    mod_id bigint REFERENCES mods NULL,
    thread_message_id bigint REFERENCES threads_messages NULL
    
);
