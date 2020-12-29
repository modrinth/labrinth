CREATE TABLE project_types (
    id serial PRIMARY KEY,
    name varchar(64) UNIQUE NOT NULL
);

INSERT INTO project_types (name) VALUES ('mod');
INSERT INTO project_types (name) VALUES ('modpack');
INSERT INTO project_types (name) VALUES ('datapack');
INSERT INTO project_types (name) VALUES ('world');
INSERT INTO project_types (name) VALUES ('resourcepack');

ALTER TABLE mods
    ADD COLUMN project_type integer REFERENCES project_types NOT NULL default 1;

ALTER TABLE categories
    ADD COLUMN project_type integer REFERENCES project_types NOT NULL default 1;