ALTER TABLE threads DISABLE TRIGGER ALL;
ALTER TABLE pats DISABLE TRIGGER ALL;
ALTER TABLE loaders_project_types DISABLE TRIGGER ALL;
ALTER TABLE team_members DISABLE TRIGGER ALL;
ALTER TABLE versions DISABLE TRIGGER ALL;
ALTER TABLE loaders_versions DISABLE TRIGGER ALL;
ALTER TABLE game_versions_versions DISABLE TRIGGER ALL;
ALTER TABLE files DISABLE TRIGGER ALL;
ALTER TABLE hashes DISABLE TRIGGER ALL;

INSERT INTO users (id, username, name, email, role) VALUES (1, 'admin', 'Administrator Test', 'admin@modrinth.com', 'admin');
INSERT INTO users (id, username, name, email, role) VALUES (2, 'moderator', 'Moderator Test', 'moderator@modrinth.com', 'mod');
INSERT INTO users (id, username, name, email, role) VALUES (3, 'user', 'User Test', 'user@modrinth.com', 'developer');
INSERT INTO users (id, username, name, email, role) VALUES (4, 'friend', 'Friend Test', 'friend@modrinth.com', 'developer');
INSERT INTO users (id, username, name, email, role) VALUES (5, 'enemy', 'Enemy Test', 'enemy@modrinth.com', 'developer');

INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (50, 1, 'admin-pat', 'mrp_patadmin', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (51, 2, 'moderator-pat', 'mrp_patmoderator', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (52, 3, 'user-pat', 'mrp_patuser', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (53, 4, 'friend-pat', 'mrp_patfriend', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (54, 5, 'enemy-pat', 'mrp_patenemy', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');

INSERT INTO game_versions (id, version, type, created)
VALUES (20000, '1.20.1', 'release', timezone('utc', now()));

INSERT INTO loaders (id, loader, icon) VALUES (1, 'fabric', 'svgloadercode');
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id) VALUES (1,1);--SELECT 1, id FROM project_types WHERE name = 'mod';
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id) VALUES (1,2); --SELECT 1, id FROM project_types WHERE name = 'modpack';

INSERT INTO teams (id) VALUES (100);
INSERT INTO team_members (id, team_id, user_id, role, permissions, accepted, payouts_split, ordering) VALUES (200, 100, 3, 'Owner', B'1111111111'::BIGINT, true, 100.0, 0);

INSERT INTO mods (
    id, team_id, title, description, body,
    published, downloads,
    status, requested_status,
    client_side, server_side, license,
    slug, project_type, monetization_status
)
VALUES (
    1000, 100, 'Test Mod', 'Test mod description', 'Test mod body',
    timezone('utc', now()), 0,
    'processing', 'approved', 
    1, 2, 'MIT',
    'testslug', 1, 'monetized'
);

INSERT INTO versions (
    id, mod_id, author_id, name, version_number,
    changelog, date_published, downloads,
    version_type, featured, status
)
VALUES (
    1100, 1000, 3, 'v1', 'v1.2.1',
    'No changes', timezone('utc', now()), 0,
    'released', true, 'listed'
);

INSERT INTO loaders_versions (loader_id, version_id) VALUES (1, 1100);
INSERT INTO game_versions_versions (game_version_id, joining_version_id) VALUES (20000, 1100);

-- not real hash or file
INSERT INTO files (id, version_id, url, filename, is_primary, size, file_type)
VALUES (800, 1100, 'http://www.url.to/myfile.jar', 'myfile.jar', true, 1, 'jar');
INSERT INTO hashes (file_id, algorithm, hash) VALUES (800, 'sha1', '10101010');

INSERT INTO threads (id, thread_type, mod_id, report_id) VALUES (30, 'project', '1000', null);