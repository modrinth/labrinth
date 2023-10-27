CREATE TABLE games (
	id int PRIMARY KEY, -- Only used in db
	name varchar(64),
  CONSTRAINT unique_game_name UNIQUE (name)
);
INSERT INTO games(id, name) VALUES (1, 'minecraft-java');
INSERT INTO games(id, name) VALUES (2, 'minecraft-bedrock');

ALTER TABLE loaders ADD CONSTRAINT unique_loader_name UNIQUE (loader);

ALTER TABLE mods ADD COLUMN game_id integer REFERENCES games NOT NULL DEFAULT 1; -- all past ones are minecraft-java
ALTER TABLE loaders ADD COLUMN game_id integer REFERENCES games NOT NULL DEFAULT 1; -- all past ones are minecraft-java

CREATE TABLE loader_field_enums (
  id serial PRIMARY KEY,
  game_id integer REFERENCES games NOT NULL DEFAULT 1,
  enum_name varchar(64) NOT NULL,
  ordering int NULL,
  hidable BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE loader_field_enum_values (
  id serial PRIMARY KEY,
  enum_id integer REFERENCES loader_field_enums NOT NULL,
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
  field varchar(64) UNIQUE NOT NULL,
  -- "integer", "text", "enum", "bool", 
  -- "array_integer", "array_text", "array_enum", "array_bool"
  field_type varchar(64) NOT NULL,
  -- only for enum
  enum_type integer REFERENCES loader_field_enums NULL,
  optional BOOLEAN NOT NULL DEFAULT true,
  -- for int- min/max val, for text- min len, for enum- min items, for bool- nothing
  min_val integer NULL,
  max_val integer NULL
);

CREATE TABLE loader_fields_loaders (
  loader_id integer REFERENCES loaders NOT NULL,
  loader_field_id integer REFERENCES loader_fields NOT NULL,
  CONSTRAINT unique_loader_field UNIQUE (loader_id, loader_field_id)
);

ALTER TABLE loaders ADD COLUMN hidable boolean NOT NULL default false;

CREATE TABLE version_fields (
  version_id bigint REFERENCES versions  NOT NULL,
  field_id integer REFERENCES loader_fields  NOT NULL,
  -- for int/bool values
  int_value integer NULL,
  enum_value integer REFERENCES loader_field_enum_values  NULL,
  string_value text NULL
);

-- Convert side_types
INSERT INTO loader_field_enums (id, enum_name, hidable) VALUES (1, 'side_types', true);
INSERT INTO loader_field_enum_values (original_id, enum_id, value) SELECT id, 1, name FROM side_types st;

INSERT INTO loader_fields (field, field_type, enum_type, optional, min_val, max_val) SELECT 'client_side', 'enum', 1, false, 1, 1;
INSERT INTO loader_fields ( field, field_type, enum_type, optional, min_val, max_val) SELECT 'server_side', 'enum', 1, false, 1, 1;

INSERT INTO loader_fields_loaders (loader_id, loader_field_id) SELECT l.id, lf.id FROM loaders l CROSS JOIN loader_fields lf  WHERE lf.field = 'client_side' AND l.loader = ANY( ARRAY['forge', 'fabric', 'quilt', 'modloader','rift','liteloader', 'neoforge']);
INSERT INTO loader_fields_loaders (loader_id, loader_field_id) SELECT l.id, lf.id FROM loaders l CROSS JOIN loader_fields lf  WHERE lf.field = 'server_side' AND l.loader = ANY( ARRAY['forge', 'fabric', 'quilt', 'modloader','rift','liteloader', 'neoforge']);

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

INSERT INTO loader_fields (field, field_type, enum_type, optional, min_val) VALUES('game_versions', 'array_enum', 2, false, 1);

INSERT INTO version_fields(version_id, field_id, enum_value) 
SELECT gvv.joining_version_id, 2, lfev.id 
FROM game_versions_versions gvv INNER JOIN loader_field_enum_values lfev ON gvv.game_version_id = lfev.original_id
WHERE lfev.enum_id = 2;

ALTER TABLE mods DROP COLUMN loaders;
ALTER TABLE mods DROP COLUMN game_versions;
DROP TABLE game_versions_versions;
DROP TABLE game_versions;

-- Drop original_id columns
ALTER TABLE loader_field_enum_values DROP COLUMN original_id;

-- drop 'minecraft-java' as default
ALTER TABLE loaders ALTER COLUMN game_id DROP DEFAULT;
ALTER TABLE loader_field_enums ALTER COLUMN game_id DROP DEFAULT;
