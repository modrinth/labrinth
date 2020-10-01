
# Routes

CORS allowed origins: `http://localhost:3000`, `https://modrinth.com` 

Prefix: `https://api.modrinth.com/`

Types:  
`datetime`: A RFC 3339 formatted datetime  
`id`s: All id types are random base 62 numbers.  They are stored as strings in JSON.  
`?`: The field is optional

## Mods

### Search Mods
GET `/api/v1/mod`

#### Parameters:
| field | type | description | default |
| --- | --- | --- | --- |
| query         |  string | The query to search for | |
| filters       |  filters | A list of filters relating to the categories of a mod | |
| version       |  filters | A list of filters relating to the versions of a mod | |
| facets        |  facets | Another system of filtering, described below | |
| index         |  string | What the results are sorted by | `relevance` |
| offset        |  integer | The offset into the search; skips this number of results |  `0` |
| limit         |  integer | The number of mods returned by the search | `10` |

`index` controls the sort ordering of the search results;  the valid values are `relevance`, `downloads`, `updated`, and `newest`

`filters` and `version` are parts of the filter for MeiliSearch.  `filters` should be a set of conditions relating to the `categories` or other fields, and `version` should be a set of conditions relating to versions.  The syntax is specified in [MeiliSearch's documentation](https://docs.meilisearch.com/guides/advanced_guides/filtering.html).

Ex:  
filters = `categories="fabric" AND (categories="technology" OR categories="utility")`  
versions = `version="1.16.3" OR version="1.16.2" OR version="1.16.1"`

`facets` is a 2 deep json array used to specify which categories the search should use; This follows the format documented [here](https://docs.meilisearch.com/guides/advanced_guides/faceted_search.html#usage), except that single elements are represented with an array of one element.

The allowed categories for `facets` are:
- `"categories:{category-name}"`  
  Where category name is one of `worldgen`, `technology`, `food`, `magic`, `storage`, `library`, `adventure`, `utility`, `decoration`, `misc`, `equipment`, `cursed`, `fabric`, or `forge`
- `"host:{host-name}"`  
  Where host is one of `modrinth`, `curseforge`
- `"versions:{version}"`  
  Where version is a valid Minecraft version, like `1.16.3` or `20w08a`

#### Response:

| field | type | description |
| --- | --- | --- |
| hits | array of `ModResult`s | The list of results |
| offset | integer | The number of results that were skipped by the query |
| limit | integer | The number of mods returned by the query |
| total_hits | integer | The total number of mods that the query found |

`ModResult`: (this is different from the [`Mod`](#Mod) struct)

| field | type | description |
| --- | --- | --- |
| `mod_id` | string | The id of the mod; prefixed to differentiate curseforge and local ids.|
| `author` | string | The username of the author of the mod |
| `title` | string | The name of the mod |
| `description` | string | A short description of the mod |
| `categories` | array of strings | A list of the categories the mod is in |
| `versions` | array of strings | A list of the minecraft versions supported by the mod |
| `downloads` | integer | The total number of downloads for the mod |
| `page_url` | string | A link to the mod's main page; for curseforge mods, this is an external link to curseforge |
| `icon_url` | string | The url of the mod's icon |
| `author_url` | string | The url of the mod's author |
| `date_created` | datetime | The date that the mod was originally created |
| `date_modified` | datetime | The date that the mod was last modified |
| `latest_version` | string | The latest version of minecraft that this mod supports |
| `host` | string | The host that this mod is from, either `modrinth` or `curseforge` |

### Get mod
GET `/api/v1/mod/{id}`

Returns information about the mod with the given id.

Response: [`Mod` struct](#Mod)

### Mod Delete
DELETE `/api/v1/mod/{id}`

Deletes the mod with the given id; TODO: permissions documentation

#### Response
HTTP 200 with empty body if successful  
HTTP 404 if mod does not exist (Note: DELETE should be idempotent; is this still fine?)

### Mod Create
POST `/api/v1/mod`

#### Request
A multipart request:

Must start with a field named `data` with these contents (in JSON)

| field | type | description |
| --- | --- | --- |
| `mod_name` | string | The title or name of the mod |
| `mod_namespace` | string | The namespace of the mod (TODO: describe / verify this, justify its existence) |
| `mod_description` | string | A short description of the mod |
| `mod_body` | string | A long description of the mod, in markdown (TODO: we may want this as a multipart upload) |
| `initial_versions` | array of `InitialVersionData` | A list of initial versions to upload with the created mod |
| `team_members` | array of `TeamMember` | The team of people that has ownership of this mod |
| `categories` | array of strings | A list of the categories that the mod is in |
| `issues_url` | string? | An optional link to where to submit bugs or issues with the mod |
| `source_url` | string? | An optional link to the source code for the mod |
| `wiki_url` | string? | An optional link to the mod's wiki page or other relevant information |
TODO: add mod license and other metadata

`InitialVersionData`:

| field | type | description |
| --- | --- | --- |
| `file_parts` | array of strings | An array of the multipart field names of each file that goes with this version |
| `version_number` | string | A string that describes this version; should be something similar to semver, but isn't require to have a specific format |
| `version_title` | string | The human readable name of the version |
| `version_body` | string | A description of the version |
| `dependencies` | array of version ids | A list of dependencies of this version; this must be specified as an array of version ids of other mods' versions  |
| `game_versions` | array of game versions | A list of game versions that this version supports |
| `release_channel` | `VersionType` | What type of release that this version is; `release`, `beta`, or `alpha` |
| `loaders` | array of mod loaders | An array of the mod loaders that this mod supports |

game versions: string, versions of minecraft  
mod loaders: string, name of a modloader (`forge`, `fabric`)  
`VersionType`: string, `release`, `beta`, or `alpha`  
TODO: changelog URL?  What is `version_body` meant for?

`TeamMember`:

| field | type | description |
| --- | --- | --- |
| `user_id` | user id | The ID of the user associated with this member |
| `name` | string | The name of the user |
| `role` | string | The role of the user in this team |

Can optionally contain a field `icon` with an icon image; supported types are `.bmp`, `.gif`, `.jpeg`, `.png`, `.svg(z)`, `.webp`, and `.rgb`

Mod jars are then uploaded; any file with a `jar` file extension is checked against the `initial_versions` array and uploaded.

## Mod Versions

### Version List
GET `/api/v1/mod/{mod_id}/version`

Response: An array of version ids for the mod

### Version Get
GET `/api/v1/mod/{mod_id}/version/{version_id}`

Response: A [`Version` struct](#version) for the given version id

### Version Delete
DELETE `/api/v1/mod/{mod_id}/version/{version_id}`

Deletes the version with the given id

### Version Create
POST `/api/v1/mod/{mod_id}/version`

#### Request
A multipart request:

Must start with a field named `data` with these contents (in JSON)

| field | type | description |
| --- | --- | --- |
| `file_parts` | array of strings | An array of the multipart field names of each file that goes with this version |
| `version_number` | string | A string that describes this version; should be something similar to semver, but isn't require to have a specific format |
| `version_title` | string | The human readable name of the version |
| `version_body` | string | A description of the version |
| `dependencies` | array of version ids | A list of dependencies of this version; this must be specified as an array of version ids of other mods' versions  |
| `game_versions` | array of game versions | A list of game versions that this version supports |
| `release_channel` | `VersionType` | What type of release that this version is; `release`, `beta`, or `alpha` |
| `loaders` | array of mod loaders | An array of the mod loaders that this mod supports |

game versions: string, versions of minecraft  
mod loaders: string, name of a modloader (`forge`, `fabric`)  
`VersionType`: string, `release`, `beta`, or `alpha`  
TODO: changelog URL?  What is `version_body` meant for?  

Mod jars are then uploaded; any file with a `jar` file extension is checked against the `file_parts` array to make sure the name matches and is uploaded.

### Add file to version
POST `/api/v1/mod/{mod_id}/version/{version_id}/file`

#### Request
A multipart request:

Must start with a field named `data` with no contents (`{}`?)

Mod jars are then uploaded; any file with a `jar` file extension is uploaded and added to the version.

## Tags

### Categories
#### List Categories
GET `/api/v1/tag/category`  
Lists the defined categories  
Response: an array of category names
#### Create Category
PUT `/api/v1/tag/category/{name}`  
Creates a new category with the given name  
Requires admin?
#### Delete Category
DELETE `/api/v1/tag/category/{name}`
Deletes the category with the given name
Requires admin?

### Loaders
#### List Loaders
GET `/api/v1/tag/loader`  
Lists the defined mod loaders  
Response: an array of mod loader names
#### Create Loader
PUT `/api/v1/tag/loader/{name}`  
Creates a new loader with the given name  
Requires admin?
#### Delete Loader
DELETE `/api/v1/tag/loader/{name}`  
Deletes the loader with the given name  
Requires admin?

### Game Versions
#### List Game Versions
GET `/api/v1/tag/game_version`  
Lists the defined game versions  
Response: an array of game versions
#### Create Game Version
PUT `/api/v1/tag/game_version/{version}`  
Creates a new game version  
Requires admin?
#### Delete Game Version
DELETE `/api/v1/tag/game_version/{version}`  
Deletes the given game version  
Requires admin?

## Auth
Oauth stuff for Github integration
### Auth Callback
GET `/api/v1/auth/callback`

Query parameters:

| field | type | description |
| --- | --- | --- |
| code | string | ??? |
| state | string | ??? |

### Init
GET `/api/v1/auth/init`

Query parameters:

| field | type | description |
| --- | --- | --- |
| url | string | ??? |

## Users
### Current User
GET `/api/v1/user`  
Gets the currently logged in user  
Response: [`User` struct](#user)
### User Get
GET `/api/v1/user/{id}`  
Gets the user with the given id  
Response: [`User` struct](#user)
### User Delete
DELETE `/api/v1/user/{id}`  
Currently does nothing, but in the future will delete the user with the given id

TODO: let users delete their own accounts? GDPR compliance?

## Structure Definitions

### User

| field | type | description |
| --- | --- | --- |
| `id` | user id | The user's id |
| `github_id` | u64? | The user's github id; only visible to the user themself |
| `username` | string | The user's username |
| `name` | string | The user's display name |
| `email` | string? | The user's email; only visible to the user themself |
| `avatar_url` | string? | The user's avatar url; uses github's icons |
| `bio` | string | A description of the user |
| `created` | datetime | The time at which the user was created |
| `role` | string | The user's role `developer`, `moderator`, or `admin` |

### Mod

| field | type | description |
| --- | --- | --- |
| `id` | mod id | The ID of the mod, encoded as a base62 string |
| `team` | team id | The id of the team that has ownership of this mod |
| `title` | string | The title or name of the mod |
| `description` | string | A short description of the mod |
| `body_url` | string | The link to the long description of the mod |
| `icon_url` | string? | The URL of the icon of the mod |
| `issues_url` | string? | An optional link to where to submit bugs or issues with the mod |
| `source_url` | string? | An optional link to the source code for the mod |
| `wiki_url` | string? | An optional link to the mod's wiki page or other relevant information |
| `published` | datetime | The date at which the mod was first published |
| `downloads` | integer | The total number of downloads the mod has |
| `categories` | array of strings | A list of the categories that the mod is in |
| `versions` | array of version ids | A list of ids for versions of the mod |

### Version

| field | type | description |
| --- | --- | --- |
|`id` | version id | The ID of the version, encoded as a base62 string |
|`mod_id` | mod id | The ID of the mod this version is for |
|`author_id` | user id | The ID of the author who published this version |
|`name` | string | The name of this version |
|`version_number` | string | The version number. Ideally will follow semantic versioning |
|`changelog_url` | string? | A link to the changelog for this version of the mod |
|`date_published` | datetime | The date that this version was published |
|`downloads` | integer | The number of downloads this specific version has |
|`version_type` | string | The type of the release - `alpha`, `beta`, or `release` |
|`files` | array of [`VersionFile`s](#versionfile) | A list of files available for download for this version |
|`dependencies` | array of version ids | A list of specific versions of mods that this version depends on |
|`game_versions` | array of game versions | A list of versions of Minecraft that this version of the mod supports |
|`loaders` | array of mod loaders | The mod loaders that this version supports |

#### VersionFile

A single mod file, with a url for the file and the file's hash

| field | type | description |
| --- | --- | --- |
| `hashes` | string to string map | A map of hashes of the file.  The key is the hashing algorithm and the value is the string version of the hash. |
| `url` | string | A direct link to the file |
| `filename` | string | The name of the file |
