-- Adds loader_fields_loaders entries for all loaders
-- (at this point, they are all Minecraft loaders, and thus have the same fields)
-- These are loaders such as bukkit, minecraft, vanilla, waterfall, velocity... etc
-- This also allows v2 routes (which have things such as client_side to remain to work with these loaders)
INSERT INTO loader_fields_loaders
SELECT l.id, lf.id FROM loaders l CROSS JOIN loader_fields lf
WHERE lf.field=ANY(ARRAY['game_versions','client_and_server','server_only','client_only','singleplayer'])
ON CONFLICT DO NOTHING;