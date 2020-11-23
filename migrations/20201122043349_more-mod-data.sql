CREATE TABLE donation_platforms (
    id serial PRIMARY KEY,
    short varchar(100) UNIQUE,
    name varchar(500) UNIQUE
);

INSERT INTO donation_platforms (short, name) VALUES ('patreon', 'Patreon');
INSERT INTO donation_platforms (short, name) VALUES ('bmac', 'Buy Me a Coffee');
INSERT INTO donation_platforms (short, name) VALUES ('paypal', 'PayPal');
INSERT INTO donation_platforms (short, name) VALUES ('github', 'GitHub Sponsors');
INSERT INTO donation_platforms (short, name) VALUES ('ko-fi', 'Ko-fi');
INSERT INTO donation_platforms (short, name) VALUES ('other', 'Other');

CREATE TABLE mods_donations (
    joining_mod_id bigint REFERENCES mods ON UPDATE CASCADE NOT NULL,
    joining_platform_id int REFERENCES donation_platforms ON UPDATE CASCADE NOT NULL,
    url varchar(2048) NOT NULL,
    PRIMARY KEY (joining_mod_id, joining_platform_id)
);

CREATE TABLE side_types (
    id serial PRIMARY KEY,
    name varchar(64) UNIQUE
);

INSERT INTO side_types (name) VALUES ('required');
INSERT INTO side_types (name) VALUES ('no-functionality');
INSERT INTO side_types (name) VALUES ('unsupported');

CREATE TABLE licenses (
    id serial PRIMARY KEY,
    short varchar(60) UNIQUE,
    name varchar(1000) UNIQUE
);

INSERT INTO licenses (short, name) VALUES ('arr', 'All Rights Reserved');

ALTER TABLE versions
    ADD COLUMN featured BOOLEAN NOT NULL default FALSE;
ALTER TABLE files
    ADD COLUMN "primary" BOOLEAN NOT NULL default FALSE;

ALTER TABLE mods
    ADD COLUMN license integer REFERENCES licenses NOT NULL default 1;
ALTER TABLE mods
    ADD COLUMN license_url varchar(1000) NULL;
ALTER TABLE mods
    ADD COLUMN client_side integer REFERENCES side_types NOT NULL default 1;
ALTER TABLE mods
    ADD COLUMN server_side integer REFERENCES side_types NOT NULL default 1;
ALTER TABLE mods
    ADD COLUMN discord_url varchar(255) NULL;
ALTER TABLE mods
    ADD COLUMN slug varchar(255) NULL UNIQUE;