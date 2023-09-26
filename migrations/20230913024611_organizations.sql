CREATE TABLE organizations (
    id bigint PRIMARY KEY,
    name varchar(255) NOT NULL,
    slug varchar(255) NOT NULL,
    description text NOT NULL,
    created_at timestamp NOT NULL DEFAULT now(),
    updated_at timestamp NOT NULL DEFAULT now(),
    default_project_permissions bigint NOT NULL DEFAULT 0,
    team_id bigint NOT NULL REFERENCES teams(id) ON UPDATE CASCADE,

    urls varchar(2048) NOT NULL,

    icon_url varchar(255) NULL,
    color integer NULL

);

ALTER TABLE mods ADD COLUMN organization_id bigint NULL REFERENCES organizations(id) ON DELETE SET NULL;

-- Allows getting of organiztion/project from team
ALTER TABLE teams ADD COLUMN organization_id bigint NULL REFERENCES organizations(id) ON DELETE SET NULL;
ALTER TABLE teams ADD COLUMN project_id bigint NULL REFERENCES mods(id) ON DELETE SET NULL;

ALTER TABLE team_members ALTER COLUMN permissions DROP NOT NULL;



