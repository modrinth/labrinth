CREATE TABLE shared_profiles (
    id bigint PRIMARY KEY,
    name varchar(255) NOT NULL,
    owner_id bigint NOT NULL,
    icon_url varchar(255),
    color integer NULL,
    updated timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,

    maximum_users integer NOT NULL,

    game_version_id int NOT NULL REFERENCES loader_field_enum_values(id),
    loader_id int NOT NULL REFERENCES loaders(id), 
    loader_version varchar(255) NOT NULL
);  

CREATE TABLE shared_profiles_mods (
    shared_profile_id bigint NOT NULL REFERENCES shared_profiles(id),

    -- for versions we have hosted
    version_id bigint NULL REFERENCES versions(id), -- for versions

    -- for cdn links to files we host directly
    file_hash varchar(255) NULL,
    install_path varchar(255) NULL,

    CHECK (
        (version_id IS NOT NULL AND file_hash IS NULL AND install_path IS NULL) OR
        (version_id IS NULL AND file_hash IS NOT NULL AND install_path IS NOT NULL)
    )
);

CREATE TABLE shared_profiles_links (
    id bigint PRIMARY KEY, -- id of the shared profile link (ignored in labrinth, for db use only)
    link varchar(48) NOT NULL UNIQUE, -- extension of the url that identifies this (ie profiles/afgxxczsewq)
    shared_profile_id bigint NOT NULL REFERENCES shared_profiles(id),
    created timestamptz NOT NULL DEFAULT now(),
    expires timestamptz NOT NULL
);

CREATE TABLE shared_profiles_users (
    shared_profile_id bigint NOT NULL REFERENCES shared_profiles(id),
    user_id bigint NOT NULL REFERENCES users(id),
    CONSTRAINT shared_profiles_users_unique UNIQUE (shared_profile_id, user_id)
);

-- Index off 'link'
CREATE INDEX shared_profiles_links_link_idx ON shared_profiles_links(link);

-- generated tokens for downloading files
CREATE TABLE cdn_auth_tokens (
    token varchar(255) PRIMARY KEY,
    shared_profiles_id bigint NOT NULL REFERENCES shared_profiles(id),
    user_id bigint NOT NULL REFERENCES users(id),
    created timestamptz NOT NULL DEFAULT now(),
    expires timestamptz NOT NULL,

    -- unique combinations of shared_profiles_links_id and user_id
    CONSTRAINT cdn_auth_tokens_unique UNIQUE (shared_profiles_id, user_id)
);