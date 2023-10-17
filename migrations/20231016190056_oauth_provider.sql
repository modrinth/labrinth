CREATE TABLE oauth_clients (
    id bigint PRIMARY KEY,
    name varchar(255) NOT NULL,
    icon_url varchar(255) NULL,
    max_scopes bigint NOT NULL,
    secret_hash char(512) NOT NULL,
    created timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by bigint NOT NULL REFERENCES users(id)
);
CREATE TABLE oauth_client_redirect_uris (
    id bigint PRIMARY KEY,
    client_id bigint REFERENCES oauth_clients(id) NOT NULL ON DELETE CASCADE,
    uri varchar(255)
);
CREATE TABLE oauth_client_authorizations (
    id bigint PRIMARY KEY,
    client_id bigint NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    user_id bigint NOT NULL REFERENCES users(id),
    scopes bigint NOT NULL,
    created timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);