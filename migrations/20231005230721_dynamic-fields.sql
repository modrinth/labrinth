CREATE TABLE games (
	id int PRIMARY KEY,
	name varchar(64)
);

INSERT INTO games(id, name) VALUES (1, 'minecraft-java');
INSERT INTO games(id, name) VALUES (2, 'minecraft-bedrock');
ALTER TABLE mods ADD COLUMN game_id integer REFERENCES games ON UPDATE CASCADE NOT NULL DEFAULT 1; -- all past ones are minecraft-java
ALTER TABLE loaders ADD COLUMN game_id integer REFERENCES games ON UPDATE CASCADE NOT NULL DEFAULT 1; -- all past ones are minecraft-java

CREATE TABLE loader_field_enums (
  id serial PRIMARY KEY,
  game_id integer REFERENCES games ON UPDATE CASCADE NOT NULL DEFAULT 1,
  enum_name varchar(64) NOT NULL,
  ordering int NULL,
  hidable BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE loader_field_enum_values (
  id serial PRIMARY KEY,
  enum_id integer REFERENCES loader_field_enums ON UPDATE CASCADE NOT NULL,
  value varchar(64) NOT NULL,
  ordering int NULL,
  created timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
  -- metadata is json of all the extra data for this enum value
  metadata jsonb NULL,
	
  original_id integer, -- This is for mapping only- it is dropped before the end of the migration

  CONSTRAINT unique_variant_per_enum UNIQUE (enum_id, value)

);

CREATE TABLE loader_fields (
  id serial PRIMARY KEY,
  loader_id integer REFERENCES loaders ON UPDATE CASCADE NOT NULL,
  field varchar(64) NOT NULL,
  -- "int", "text", "enum", "bool", 
  -- "array(int)", "array(text)", "array(enum)", "array(bool)"
  field_type varchar(64) NOT NULL,
  -- only for enum
  enum_type integer REFERENCES loader_field_enums ON UPDATE CASCADE NULL,
  optional BOOLEAN NOT NULL DEFAULT true,
  -- for int- min/max val, for text- min len, for enum- min items, for bool- nothing
  min_val integer NULL,
  max_val integer NULL,

  CONSTRAINT unique_field_name_per_loader UNIQUE (loader_id, field)
);

ALTER TABLE loaders ADD COLUMN hidable boolean NOT NULL default false;

CREATE TABLE version_fields (
  id bigint PRIMARY KEY,
  version_id bigint REFERENCES versions ON UPDATE CASCADE NOT NULL,
  field_id integer REFERENCES loader_fields ON UPDATE CASCADE NOT NULL,
  -- for int/bool values
  int_value integer NULL,
  enum_value integer REFERENCES loader_field_enum_values ON UPDATE CASCADE NULL,
  string_value text NULL
);

-- Convert side_types
INSERT INTO loader_field_enums (id, enum_name, hidable) VALUES (1, 'side_types', true);
INSERT INTO loader_field_enum_values (original_id, enum_id, value) SELECT id, 1, name FROM side_types st;

INSERT INTO loader_fields (loader_id, field, field_type, enum_type, optional, min_val, max_val) SELECT l.id, 'client_side', 'enum', 1, false, 1, 1 FROM loaders l;
INSERT INTO loader_fields (loader_id, field, field_type, enum_type, optional, min_val, max_val) SELECT l.id, 'server_side', 'enum', 1, false, 1, 1 FROM loaders l;

INSERT INTO version_fields (version_id, field_id, enum_value) 
SELECT v.id, 1, m.client_side 
FROM versions v
INNER JOIN mods m ON v.mod_id = m.id
INNER JOIN loader_field_enum_values lfev ON m.client_side = lfev.original_id
WHERE client_side IS NOT NULL AND lfev.enum_id = 1;

INSERT INTO version_fields (version_id, field_id, enum_value) 
SELECT v.id, 1, m.server_side 
FROM versions v
INNER JOIN mods m ON v.mod_id = m.id
INNER JOIN loader_field_enum_values lfev ON m.client_side = lfev.original_id
WHERE server_side IS NOT NULL AND lfev.enum_id = 1;

ALTER TABLE mods DROP COLUMN client_side;
ALTER TABLE mods DROP COLUMN server_side;
DROP TABLE side_types;

-- Convert game_versions
INSERT INTO loader_field_enums (id, enum_name, hidable) VALUES (2, 'game_versions', true);
INSERT INTO loader_field_enum_values (original_id, enum_id, value, created, metadata)
SELECT id, 2, version, created, json_build_object('type', type, 'major', major) FROM game_versions;

INSERT INTO loader_fields (loader_id, field, field_type, enum_type, optional, min_val) SELECT l.id, 'game_versions', 'enum', 2, false, 1 FROM loaders l;

INSERT INTO version_fields(version_id, field_id, enum_value) 
SELECT gvv.joining_version_id, 2, lfev.id 
FROM game_versions_versions gvv INNER JOIN loader_field_enum_values lfev ON gvv.game_version_id = lfev.original_id
WHERE lfev.enum_id = 2;

DROP TABLE game_versions_versions;
DROP TABLE game_versions;

-- Drop original_id columns
ALTER TABLE loader_field_enum_values DROP COLUMN original_id;

-- drop 'minecraft-java' as default
ALTER TABLE loaders ALTER COLUMN game_id DROP DEFAULT;
ALTER TABLE loader_field_enums ALTER COLUMN game_id DROP DEFAULT;
