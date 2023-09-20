CREATE TABLE organizations (
    id bigint PRIMARY KEY,
    name varchar(255) NOT NULL,
    slug varchar(255) NOT NULL,
    description text NOT NULL,
    created_at timestamp NOT NULL DEFAULT now(),
    updated_at timestamp NOT NULL DEFAULT now(),
    default_project_permissions bigint NOT NULL DEFAULT 0,
    team_id bigint NOT NULL REFERENCES teams(id) ON UPDATE CASCADE,

    discord_url varchar(255) NULL,
    website_url varchar(255) NULL,

    icon_url varchar(255) NULL,
    color integer NULL

);

CREATE TABLE organizations_donations (
    joining_organization_id bigint REFERENCES organizations ON UPDATE CASCADE NOT NULL,
    joining_platform_id int REFERENCES donation_platforms ON UPDATE CASCADE NOT NULL,
    url varchar(2048) NOT NULL,
    PRIMARY KEY (joining_organization_id, joining_platform_id)
);


ALTER TABLE mods ADD COLUMN organization_id bigint NULL REFERENCES organizations(id) ON DELETE SET NULL;

ALTER TABLE team_members ADD COLUMN organization_permissions bigint default 0 NULL;
ALTER TABLE team_members ALTER COLUMN permissions DROP NOT NULL;


