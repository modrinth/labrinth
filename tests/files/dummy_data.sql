-- Dummy test data for use in tests.
-- IDs are listed as integers, followed by their equivalent base 62 representation.

-- Inserts 5 dummy users for testing, with slight differences
-- 'Friend' and 'enemy' function like 'user', but we can use them to simulate 'other' users that may or may not be able to access certain things
-- IDs 1-5, 1-5
INSERT INTO users (id, username, name, email, role) VALUES (1, 'admin', 'Administrator Test', 'admin@modrinth.com', 'admin');
INSERT INTO users (id, username, name, email, role) VALUES (2, 'moderator', 'Moderator Test', 'moderator@modrinth.com', 'moderator');
INSERT INTO users (id, username, name, email, role) VALUES (3, 'user', 'User Test', 'user@modrinth.com', 'developer');
INSERT INTO users (id, username, name, email, role) VALUES (4, 'friend', 'Friend Test', 'friend@modrinth.com', 'developer');
INSERT INTO users (id, username, name, email, role) VALUES (5, 'enemy', 'Enemy Test', 'enemy@modrinth.com', 'developer');

-- Full PATs for each user, with different scopes
-- These are not legal PATs, as they contain all scopes- they mimic permissions of a logged in user
-- IDs: 50-54, o p q r s
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (50, 1, 'admin-pat', 'mrp_patadmin', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (51, 2, 'moderator-pat', 'mrp_patmoderator', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (52, 3, 'user-pat', 'mrp_patuser', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (53, 4, 'friend-pat', 'mrp_patfriend', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');
INSERT INTO pats (id, user_id, name, access_token, scopes, expires) VALUES (54, 5, 'enemy-pat', 'mrp_patenemy', B'11111111111111111111111111111111111'::BIGINT, '2030-08-18 15:48:58.435729+00');

-- -- Sample game versions, loaders, categories
INSERT INTO game_versions (id, version, type, created)
VALUES (20000, '1.20.1', 'release', timezone('utc', now()));

INSERT INTO loaders (id, loader) VALUES (1, 'fabric');
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id) VALUES (1,1);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id) VALUES (1,2); 

INSERT INTO categories (id, category, project_type) VALUES (1, 'combat', 1);
INSERT INTO categories (id, category, project_type) VALUES (2, 'decoration', 1);
INSERT INTO categories (id, category, project_type) VALUES (3, 'economy', 1);

INSERT INTO categories (id, category, project_type) VALUES (4, 'combat', 2);
INSERT INTO categories (id, category, project_type) VALUES (5, 'decoration', 2);
INSERT INTO categories (id, category, project_type) VALUES (6, 'economy', 2);

-- -- Inserts 2 dummy projects for testing, with slight differences
-- ------------------------------------------------------------
-- INSERT INTO teams (id) VALUES (100); -- ID: 100, 1c
-- INSERT INTO team_members (id, team_id, user_id, role, permissions, accepted, payouts_split, ordering) VALUES (200, 100, 3, 'Owner', B'1111111111'::BIGINT, true, 100.0, 0);

-- -- ID: 1000, G8
-- -- This project is approved, viewable
-- INSERT INTO mods (id, team_id, title, description, body, published, downloads, status, requested_status, client_side, server_side, license, slug, project_type, monetization_status)
-- VALUES (1000, 100, 'Test Mod', 'Test mod description', 'Test mod body', timezone('utc', now()), 0, 'approved', 'approved', 1, 2, 'MIT', 'testslug', 1, 'monetized');

-- -- ID: 1100, Hk
-- -- This version is listed, viewable
-- INSERT INTO versions ( id, mod_id, author_id, name, version_number, changelog, date_published, downloads, version_type, featured, status)
-- VALUES (1100, 1000, 3, 'v1', 'v1.2.1', 'No changes', timezone('utc', now()), 0,'released', true, 'listed');

-- INSERT INTO loaders_versions (loader_id, version_id) VALUES (1, 1100);
-- INSERT INTO game_versions_versions (game_version_id, joining_version_id) VALUES (20000, 1100);

-- -- not real hash or file
-- INSERT INTO files (id, version_id, url, filename, is_primary, size, file_type)
-- VALUES (800, 1100, 'http://www.url.to/myfile.jar', 'myfile.jar', true, 1, 'required-resource-pack');
-- INSERT INTO hashes (file_id, algorithm, hash) VALUES (800, 'sha1', '000000000');

-- -- ID: 30, U
-- INSERT INTO threads (id, thread_type, mod_id, report_id) VALUES (30, 'project', 1000, null);

-- ------------------------------------------------------------
-- INSERT INTO teams (id) VALUES (101);    -- ID: 101, 1d
-- INSERT INTO team_members (id, team_id, user_id, role, permissions, accepted, payouts_split, ordering) VALUES (201, 101, 3, 'Owner', B'1111111111'::BIGINT, true, 100.0, 0);

-- -- ID: 1001, G9
-- -- This project is processing, and therefore not publically viewable
-- INSERT INTO mods (id, team_id, title, description, body, published, downloads, status, requested_status, client_side, server_side, license, slug, project_type, monetization_status)
-- VALUES (1001, 101, 'Test Mod 2', 'Test mod description 2', 'Test mod body 2', timezone('utc', now()), 0, 'processing', 'approved', 1, 2, 'MIT', 'testslug2', 1, 'monetized');

-- -- ID: 1101, Hl
-- -- This version is a draft, and therefore not publically viewable
-- INSERT INTO versions (    id, mod_id, author_id, name, version_number, changelog, date_published, downloads, version_type, featured, status)
-- VALUES (1101, 1001, 3, 'v1.0', 'v1.2.1', 'No changes', timezone('utc', now()), 0,'released', true, 'draft');

-- INSERT INTO loaders_versions (loader_id, version_id) VALUES (1, 1101);
-- INSERT INTO game_versions_versions (game_version_id, joining_version_id) VALUES (20000, 1101);

-- -- not real hash or file
-- INSERT INTO files (id, version_id, url, filename, is_primary, size, file_type)
-- VALUES (801, 1101, 'http://www.url.to/myfile2.jar', 'myfile2.jar', true, 1, 'required-resource-pack');
-- INSERT INTO hashes (file_id, algorithm, hash) VALUES (801, 'sha1', '111111111');

-- -- ID: 31, V
-- INSERT INTO threads (id, thread_type, mod_id, report_id, show_in_mod_inbox) VALUES (31, 'project', 1001, null, true);