CREATE TABLE webhooks (
    id serial PRIMARY KEY,
    url varchar(255) NOT NULL UNIQUE
);

CREATE TABLE loaders_webhooks (
    loader_id int REFERENCES loaders ON UPDATE CASCADE NOT NULL,
    webhook_id int REFERENCES webhooks ON UPDATE CASCADE NOT NULL,
    PRIMARY KEY (loader_id, webhook_id)
);

CREATE TABLE mods_webhooks (
    mod_id bigint REFERENCES mods ON UPDATE CASCADE NOT NULL,
    webhook_id int REFERENCES webhooks ON UPDATE CASCADE NOT NULL,
    PRIMARY KEY (mod_id, webhook_id)
);
