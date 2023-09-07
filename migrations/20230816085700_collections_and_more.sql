CREATE TABLE collections (
    id bigint PRIMARY KEY,
    title varchar(255) NOT NULL,
    description varchar(2048) NOT NULL,
    user_id bigint REFERENCES users NOT NULL,
    created timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,

    status varchar(64) NOT NULL DEFAULT 'listed',

    icon_url varchar(2048) NULL,
    color integer NULL
);

CREATE TABLE collections_mods (
    collection_id bigint REFERENCES collections NOT NULL,
    mod_id bigint REFERENCES mods NOT NULL,
    PRIMARY KEY (collection_id, mod_id)
);

CREATE TABLE uploaded_images_context (
    id serial PRIMARY KEY,
    name varchar(64) NOT NULL
);

CREATE TABLE uploaded_images (
    id bigint PRIMARY KEY,
    url varchar(2048) NOT NULL,
    size integer NOT NULL,
    created timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    owner_id bigint REFERENCES users NOT NULL,

    -- Associated with another table
    context_type int REFERENCES uploaded_images_context NOT NULL,
    context_id bigint NULL -- references the id of the context table it's associated with (e.g. version_id)
);


-- project, version, thread_message, report
INSERT INTO uploaded_images_context (name) VALUES ('project');
INSERT INTO uploaded_images_context (name) VALUES ('version');
INSERT INTO uploaded_images_context (name) VALUES ('thread_message');
INSERT INTO uploaded_images_context (name) VALUES ('report');