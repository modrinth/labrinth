CREATE TABLE shared_profiles (
    id bigint PRIMARY KEY,
    name varchar(255) NOT NULL,
    owner_id bigint NOT NULL,
    icon_url varchar(255),
    color integer NULL,
    updated timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,

    loader_id int NOT NULL REFERENCES loaders(id), 
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,

    game_id int NOT NULL REFERENCES games(id)
);  

CREATE TABLE shared_profiles_links (
    id bigint PRIMARY KEY, -- id of the shared profile link (doubles as the link identifier)
    shared_profile_id bigint NOT NULL REFERENCES shared_profiles(id),
    created timestamptz NOT NULL DEFAULT now(),
    expires timestamptz NOT NULL
);

CREATE TABLE shared_profiles_users (
    shared_profile_id bigint NOT NULL REFERENCES shared_profiles(id),
    user_id bigint NOT NULL REFERENCES users(id),
    CONSTRAINT shared_profiles_users_unique UNIQUE (shared_profile_id, user_id)
);

-- Together, the following two tables comprise the list of files that are part of a shared profile.
-- for versions we have hosted
CREATE TABLE shared_profiles_versions (
    shared_profile_id bigint NOT NULL REFERENCES shared_profiles(id),
    version_id bigint NULL REFERENCES versions(id) -- for versions
);

-- for files we host directly
CREATE TABLE shared_profiles_files (
    shared_profile_id bigint NOT NULL REFERENCES shared_profiles(id),
    file_id bigint NOT NULL REFERENCES files(id),
    install_path varchar(255) NOT NULL
);

-- Now that files do not necessarily have a version, we create a table to store them
CREATE TABLE versions_files (
    version_id bigint NOT NULL REFERENCES versions(id),
    is_primary boolean NOT NULL DEFAULT false,
    file_id bigint NOT NULL REFERENCES files(id)
);

-- Populate with the previously named 'version_id' column of the files table
INSERT INTO versions_files (version_id, file_id, is_primary)
SELECT version_id, id, is_primary FROM files;

-- Drop the version_id and is_primary columns from the files table
ALTER TABLE files DROP COLUMN version_id;
ALTER TABLE files DROP COLUMN is_primary;