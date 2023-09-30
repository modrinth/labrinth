CREATE TABLE organizations (
    id bigint PRIMARY KEY,
    slug varchar(255) NOT NULL,
    description text NOT NULL,
    created_at timestamp NOT NULL DEFAULT now(),
    updated_at timestamp NOT NULL DEFAULT now(),
    team_id bigint NOT NULL REFERENCES teams(id) ON UPDATE CASCADE,

    icon_url varchar(255) NULL,
    color integer NULL
);

ALTER TABLE mods ADD COLUMN organization_id bigint NULL REFERENCES organizations(id) ON DELETE SET NULL;

-- Organization permissions only apply to teams that are associated to an organization
-- If they do, 'permissions' is considered the fallback permissions for projects in the organization
ALTER TABLE team_members ADD COLUMN organization_permissions bigint NULL;

CREATE TABLE link_platforms (
    id serial PRIMARY KEY,
    short varchar(100) UNIQUE NOT NULL,
    name varchar(500) UNIQUE NOT NULL
);

INSERT INTO link_platforms (short, name) VALUES ('github', 'Github');
INSERT INTO link_platforms (short, name) VALUES ('wiki', 'Wiki');
INSERT INTO link_platforms (short, name) VALUES ('discord', 'Discord');
INSERT INTO link_platforms (short, name) VALUES ('website', 'Website');
INSERT INTO link_platforms (short, name) VALUES ('other', 'Other');

CREATE TABLE organization_links (
    joining_organization_id bigint REFERENCES organizations ON UPDATE CASCADE NOT NULL,
    joining_platform_id int REFERENCES link_platforms ON UPDATE CASCADE NOT NULL,
    url varchar(2048) NOT NULL,
    PRIMARY KEY (joining_organization_id, joining_platform_id)
);