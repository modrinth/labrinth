ALTER TABLE mods ADD COLUMN license_new varchar(2048) DEFAULT 'LicenseRef-All-Rights-Reserved' NOT NULL;

UPDATE mods SET license_new = licenses.short FROM licenses WHERE mods.license = licenses.id;

UPDATE mods SET license_new = 'LicenseRef-Custom' WHERE license = 'custom';
UPDATE mods SET license_new = 'LicenseRef-All-Rights-Reserved' WHERE license = 'arr';
UPDATE mods SET license_new = 'Apache-2.0' WHERE license = 'apache';
UPDATE mods SET license_new = 'BSD-2-Clause' WHERE license = 'bsd-2-clause';
UPDATE mods SET license_new = 'BSD-3-Clause' WHERE license = 'bsd-3-clause' OR license = 'bsd';
UPDATE mods SET license_new = 'CC0-1.0' WHERE license = 'cc0';
UPDATE mods SET license_new = 'Unlicense' WHERE license = 'unlicense';
UPDATE mods SET license_new = 'MIT' WHERE license = 'mit';
UPDATE mods SET license_new = 'LGPL-3.0-only' WHERE license = 'lgpl-3';
UPDATE mods SET license_new = 'LGPL-2.1-only' WHERE license = 'lgpl-2.1' OR license = 'lgpl';
UPDATE mods SET license_new = 'MPL-2.0' WHERE license = 'mpl-2';
UPDATE mods SET license_new = 'ISC' WHERE license = 'isc';
UPDATE mods SET license_new = 'Zlib' WHERE license = 'zlib';
UPDATE mods SET license_new = 'GPL-2.0-only' WHERE license = 'gpl-2';
UPDATE mods SET license_new = 'GPL-3.0-only' WHERE license = 'gpl-3';
UPDATE mods SET license_new = 'AGPL-3.0-only' WHERE license = 'agpl';

UPDATE mods SET license_url = NULL WHERE license_url = 'https://cdn.modrinth.com/licenses/arr.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/Apache-2.0.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/apache.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/BSD-2-Clause.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/bsd-2-clause.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/BSD-3-Clause.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/bsd-3-clause.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/CC0-1.0.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/cc0.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/Unlicense.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/unlicense.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/MIT.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/mit.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/LGPL-3.0-only.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/lgpl-3.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/LGPL-2.1-only.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/lgpl-2.1.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/MPL-2.0.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/mpl-2.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/ISC.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/isc.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/Zlib.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/zlib.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/GPL-2.0-only.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/gpl-2.txt';
UPDATE mods SET license_url = 'https://spdx.org/licenses/GPL-3.0-only.html' WHERE license_url = 'https://cdn.modrinth.com/licenses/gpl-3.txt';

ALTER TABLE mods DROP COLUMN license;
ALTER TABLE mods RENAME COLUMN license_new TO license;

DROP TABLE licenses;
