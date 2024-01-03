INSERT INTO loader_fields_loaders
SELECT l.id, lf.id FROM loaders l CROSS JOIN loader_fields lf
WHERE lf.field=ANY(ARRAY['game_versions'])
  AND
        l.loader IN ('vanilla', 'minecraft', 'optifine', 'iris', 'canvas')
ON CONFLICT DO NOTHING;
