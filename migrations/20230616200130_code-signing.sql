CREATE TABLE signing_keys (
    id bigint PRIMARY KEY,
    owner_id bigint REFERENCES users ON UPDATE CASCADE NOT NULL,
    body_type varchar(16) NOT NULL,
    body text NOT NULL UNIQUE,
    created timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL
)
