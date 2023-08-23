CREATE TABLE collections (
    id bigint PRIMARY KEY,
    title varchar(255) NOT NULL,
    description varchar(2048) NOT NULL,
    user_id bigint REFERENCES users NOT NULL,
    created timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,

    status varchar(64) NOT NULL DEFAULT 'listed', -- 

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
    owner_id bigint REFERENCES users NOT NULL
);

-- Currently, images will be associated with mod descriptions and thread messages
-- In the future, we may want to allow images to be associated with other things like reports, comments, etc.

CREATE TABLE images_mods (
    mod_id bigint REFERENCES mods NOT NULL,
    image_id bigint REFERENCES uploaded_images NOT NULL,
    PRIMARY KEY (mod_id, image_id)
);

CREATE TABLE images_threads (
    thread_message_id bigint REFERENCES threads_messages NOT NULL,
    image_id bigint REFERENCES uploaded_images NOT NULL,
    PRIMARY KEY (thread_message_id, image_id)
);