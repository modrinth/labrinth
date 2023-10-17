CREATE TABLE loader_field_enums (
    id serial PRIMARY KEY,
    enum_name varchar(64) NOT NULL,
    ordering int NULL,
    hidable BOOLEAN NOT NULL DEFAULT FALSE,
    metadata varchar(128)
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
    max_val integer NULL
);

ALTER TABLE loaders ADD COLUMN hidable boolean NOT NULL default false;

CREATE TABLE version_fields (
  id serial PRIMARY KEY,
  version_id bigint REFERENCES versions ON UPDATE CASCADE NOT NULL,
  field_id integer REFERENCES loader_fields ON UPDATE CASCADE NOT NULL,
  -- for int/bool values
  int_value integer NULL,
  enum_value integer REFERENCES loader_field_enums ON UPDATE CASCADE NULL,
  string_value text NULL
);

-- DROP TABLE side_types;
-- DROP TABLE game_versions;
