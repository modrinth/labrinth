ALTER TABLE loaders
    ADD COLUMN featured boolean DEFAULT FALSE NOT NULL;

UPDATE loaders
    SET featured = TRUE
    WHERE loader = 'forge' OR loader = 'fabric' OR loader = 'quilt';