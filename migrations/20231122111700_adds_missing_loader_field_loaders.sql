INSERT INTO loader_fields_loaders (loader_id, loader_field_id) 
SELECT l.id, lf.id FROM loaders l CROSS JOIN loader_fields lf  WHERE lf.field = 'game_versions' 
AND l.loader = ANY( ARRAY['forge', 'fabric', 'quilt', 'modloader','rift','liteloader', 'neoforge'])
ON CONFLICT (loader_id, loader_field_id) DO NOTHING;
