CREATE TABLE collections (
    id bigint PRIMARY KEY,
    title varchar(255) NOT NULL,
    description varchar(2048) NOT NULL,
    team_id bigint REFERENCES teams NOT NULL,
    published timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    slug varchar(255) NULL UNIQUE,
    body varchar(65536) NOT NULL DEFAULT ''::varchar,

    moderation_message varchar(2000),
    moderation_message_body varchar(65536),
    approved timestamp with time zone,
    queued timestamp with time zone,

    public boolean NOT NULL DEFAULT false,

    icon_url varchar(2048) NULL,
    color integer NULL

    -- Loaders, gameversions, licenses can be added as the owner sees fit
    -- loaders varchar(255)[] NOT NULL default array[]::varchar[],
    -- game_versions varchar(255)[] NOT NULL default array[]::varchar[],


    -- ADD PRIVACY
    -- COLOR
);

CREATE TABLE collections_mods (
    collection_id bigint REFERENCES collections NOT NULL,
    mod_id bigint REFERENCES mods NOT NULL,
    PRIMARY KEY (collection_id, mod_id)
);
