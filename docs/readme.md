# Labrinth Docs

## [Routes](routes.md)

## Environment Variables
Labrinth is mostly configured at runtime through environment variables.  Labrinth automatically loads in environment variables specified in a `.env` file (using [`dotenv`](https://github.com/dotenv-rs/dotenv)).

### Basic config
`CDN_URL`: The publicly accessible base URL for files uploaded to the CDN
`DATABASE_URL`: The URL for the Postgres database
`MEILISEARCH_ADDR`: The URL for the MeiliSearch instance used for search
`BIND_ADDR`: The bind address for the server.  Should support IPv4 and IPv6
`RUST_LOG`: A specifier for what information to log, from rust's [`env-logger`](https://github.com/env-logger-rs/env_logger);  a reasonable default is `info,sqlx::query=warn`.

### CDN Config
`BACKBLAZE_ENABLED`: `true` or `false`, defaults to `false`
Controls whether backblaze is used as the CDN backend.

The backblaze backend is configured using these variables:
`BACKBLAZE_KEY_ID`: The backblaze key id
`BACKBLAZE_KEY`: The backblaze key
`BACKBLAZE_BUCKET_ID`: The backblaze bucket id

If backblaze support is disabled, the filesystem is used to store uploaded files:
`MOCK_FILE_PATH`: The path used to store uploaded files; no default value, will panic if unspecified (Note: we may want to change this in the future)

### Indexing
`INDEX_CURSEFORGE`: `true` or `false`, defaults to `false`; Whether to index curseforge mods and add them to the search database
`MAX_CURSEFORGE_ID`:  The maximum curseforge mod ID to index; This should currently be around `450000`, but will change as more mods are added

`LOCAL_INDEX_INTERVAL`: The interval, in seconds, at which the local database is reindexed for searching.  Defaults to `3600` seconds (1 hour).
`EXTERNAL_INDEX_INTERVAL`: The interval, in seconds, at which curseforge is reindexed for searching.  Defaults to `43200` seconds (12 hours).

### Auth
Github Oauth support:
`GITHUB_CLIENT_ID`: The github client id
`GITHUB_CLIENT_SECRET`: The github client secret

TODO: docs about the Github integration

## Commandline Config

`--skip-first-index`: Skips indexing the local database and external sources (Curseforge) on startup;  This is useful to prevent doing unneccessary work when frequently restarting.

`--reconfigure-indices`: Resets the MeiliSearch settings for the search indices and exits

`--reset-indices`: Resets the MeiliSearch indices and exits; this clears all previously indexed mods.


