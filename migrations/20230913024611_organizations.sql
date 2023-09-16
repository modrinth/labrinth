CREATE TABLE organizations (
    id bigint PRIMARY KEY,
    name varchar(255) NOT NULL,
    slug varchar(255) NOT NULL,
    description text NOT NULL,
    created_at timestamp NOT NULL DEFAULT now(),
    updated_at timestamp NOT NULL DEFAULT now(),
    default_project_permissions bigint NOT NULL DEFAULT 0,
    
    team_id bigint NOT NULL REFERENCES teams(id) ON UPDATE CASCADE

);

ALTER TABLE mods ADD COLUMN organization_id bigint NULL REFERENCES organizations(id) ON DELETE SET NULL;

ALTER TABLE team_members ADD COLUMN organization_permissions bigint default 0 NULL;
ALTER TABLE team_members ALTER COLUMN permissions DROP NOT NULL;
