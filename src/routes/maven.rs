use crate::auth::checks::{is_visible_project, is_visible_version};
use crate::database::models::legacy_loader_fields::MinecraftGameVersion;
use crate::database::models::loader_fields::Loader;
use crate::database::models::project_item::QueryProject;
use crate::database::models::version_item::{QueryFile, QueryVersion};
use crate::database::redis::RedisPool;
use crate::models::pats::Scopes;
use crate::models::projects::{ProjectId, VersionId};
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::{auth::get_user_from_headers, database};
use axum::extract::{ConnectInfo, Path};
use axum::http::header::CONTENT_TYPE;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::{Extension, Router};
use sqlx::PgPool;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use yaserde_derive::YaSerialize;

pub fn config() -> Router {
    Router::new()
        .route(
            "/maven/modrinth/:id/maven-metadata.xml",
            get(maven_metadata),
        )
        .route(
            "maven/modrinth/:id/:versionnum/:file",
            get(version_file).head(version_file),
        )
}

#[derive(Default, Debug, Clone, YaSerialize)]
#[yaserde(root = "metadata", rename = "metadata")]
pub struct Metadata {
    #[yaserde(rename = "groupId")]
    group_id: String,
    #[yaserde(rename = "artifactId")]
    artifact_id: String,
    versioning: Versioning,
}

#[derive(Default, Debug, Clone, YaSerialize)]
#[yaserde(rename = "versioning")]
pub struct Versioning {
    latest: String,
    release: String,
    versions: Versions,
    #[yaserde(rename = "lastUpdated")]
    last_updated: String,
}

#[derive(Default, Debug, Clone, YaSerialize)]
#[yaserde(rename = "versions")]
pub struct Versions {
    #[yaserde(rename = "version")]
    versions: Vec<String>,
}

#[derive(Default, Debug, Clone, YaSerialize)]
#[yaserde(rename = "project", namespace = "http://maven.apache.org/POM/4.0.0")]
pub struct MavenPom {
    #[yaserde(rename = "xsi:schemaLocation", attribute)]
    schema_location: String,
    #[yaserde(rename = "xmlns:xsi", attribute)]
    xsi: String,
    #[yaserde(rename = "modelVersion")]
    model_version: String,
    #[yaserde(rename = "groupId")]
    group_id: String,
    #[yaserde(rename = "artifactId")]
    artifact_id: String,
    version: String,
    name: String,
    description: String,
}

pub async fn maven_metadata(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(params): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = params;
    let Some(project) = database::models::Project::get(&project_id, &pool, &redis).await? else {
        return Err(ApiError::NotFound);
    };

    let user_option = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if !is_visible_project(&project.inner, &user_option, &pool).await? {
        return Err(ApiError::NotFound);
    }

    let version_names = sqlx::query!(
        "
        SELECT id, version_number, version_type
        FROM versions
        WHERE mod_id = $1 AND status = ANY($2)
        ORDER BY ordering ASC NULLS LAST, date_published ASC
        ",
        project.inner.id as database::models::ids::ProjectId,
        &*crate::models::projects::VersionStatus::iterator()
            .filter(|x| x.is_listed())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
    )
    .fetch_all(&pool)
    .await?;

    let mut new_versions = Vec::new();
    let mut vals = HashSet::new();
    let mut latest_release = None;

    for row in version_names {
        let value = if vals.contains(&row.version_number) {
            format!("{}", VersionId(row.id as u64))
        } else {
            row.version_number
        };

        vals.insert(value.clone());
        if row.version_type == "release" {
            latest_release = Some(value.clone())
        }

        new_versions.push(value);
    }

    let project_id: ProjectId = project.inner.id.into();

    let respdata = Metadata {
        group_id: "maven.modrinth".to_string(),
        artifact_id: project_id.to_string(),
        versioning: Versioning {
            latest: new_versions
                .last()
                .unwrap_or(&"release".to_string())
                .to_string(),
            release: latest_release.unwrap_or_default(),
            versions: Versions {
                versions: new_versions,
            },
            last_updated: project.inner.updated.format("%Y%m%d%H%M%S").to_string(),
        },
    };

    Ok((
        [(CONTENT_TYPE, "text/xml")],
        yaserde::ser::to_string(&respdata).map_err(ApiError::Xml)?,
    ))
}

async fn find_version(
    project: &QueryProject,
    vcoords: &String,
    pool: &PgPool,
    redis: &RedisPool,
) -> Result<Option<QueryVersion>, ApiError> {
    let id_option = crate::models::ids::base62_impl::parse_base62(vcoords)
        .ok()
        .map(|x| x as i64);

    let all_versions = database::models::Version::get_many(&project.versions, pool, redis).await?;

    let exact_matches = all_versions
        .iter()
        .filter(|x| &x.inner.version_number == vcoords || Some(x.inner.id.0) == id_option)
        .collect::<Vec<_>>();

    if exact_matches.len() == 1 {
        return Ok(Some(exact_matches[0].clone()));
    }

    // Try to parse version filters from version coords.
    let Some((vnumber, filter)) = vcoords.rsplit_once('-') else {
        return Ok(exact_matches.first().map(|x| (*x).clone()));
    };

    let db_loaders: HashSet<String> = Loader::list(pool, redis)
        .await?
        .into_iter()
        .map(|x| x.loader)
        .collect();

    let (loaders, game_versions) = filter
        .split(',')
        .map(String::from)
        .partition::<Vec<_>, _>(|el| db_loaders.contains(el));

    let matched = all_versions
        .iter()
        .filter(|x| {
            let mut bool = x.inner.version_number == vnumber;

            if !loaders.is_empty() {
                bool &= x.loaders.iter().any(|y| loaders.contains(y));
            }

            // For maven in particular, we will hardcode it to use GameVersions rather than generic loader fields, as this is minecraft-java exclusive
            if !game_versions.is_empty() {
                let version_game_versions = x
                    .version_fields
                    .clone()
                    .into_iter()
                    .find_map(|v| MinecraftGameVersion::try_from_version_field(&v).ok());
                if let Some(version_game_versions) = version_game_versions {
                    bool &= version_game_versions
                        .iter()
                        .any(|y| game_versions.contains(&y.version));
                }
            }

            bool
        })
        .collect::<Vec<_>>();

    Ok(matched
        .first()
        .or_else(|| exact_matches.first())
        .copied()
        .cloned())
}

fn find_file<'a>(
    project_id: &str,
    vcoords: &str,
    version: &'a QueryVersion,
    file: &str,
) -> Option<&'a QueryFile> {
    if let Some(selected_file) = version.files.iter().find(|x| x.filename == file) {
        return Some(selected_file);
    }

    // Minecraft mods are not going to be both a mod and a modpack, so this minecraft-specific handling is fine
    // As there can be multiple project types, returns the first allowable match
    let mut fileexts = vec![];
    for project_type in version.project_types.iter() {
        match project_type.as_str() {
            "mod" => fileexts.push("jar"),
            "modpack" => fileexts.push("mrpack"),
            _ => (),
        }
    }

    for fileext in fileexts {
        if file == format!("{}-{}.{}", &project_id, &vcoords, fileext) {
            return version
                .files
                .iter()
                .find(|x| x.primary)
                .or_else(|| version.files.iter().last());
        }
    }
    None
}

pub async fn version_file(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((project_id, vnum, file)): Path<(String, String, String)>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<axum::response::Response, ApiError> {
    let Some(project) = database::models::Project::get(&project_id, &pool, &redis).await? else {
        return Err(ApiError::NotFound);
    };

    let user_option = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if !is_visible_project(&project.inner, &user_option, &pool).await? {
        return Err(ApiError::NotFound);
    }

    let Some(version) = find_version(&project, &vnum, &pool, &redis).await? else {
        return Err(ApiError::NotFound);
    };

    if !is_visible_version(&version.inner, &user_option, &pool, &redis).await? {
        return Err(ApiError::NotFound);
    }

    let processed_file = file.replace(".sha1", "").replace(".sha1", "");

    if processed_file == format!("{}-{}.pom", &project_id, &vnum) {
        let respdata = MavenPom {
            schema_location:
                "http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd"
                    .to_string(),
            xsi: "http://www.w3.org/2001/XMLSchema-instance".to_string(),
            model_version: "4.0.0".to_string(),
            group_id: "maven.modrinth".to_string(),
            artifact_id: project_id,
            version: vnum,
            name: project.inner.name,
            description: project.inner.description,
        };

        return Ok((
            [(CONTENT_TYPE, "text/xml")],
            yaserde::ser::to_string(&respdata).map_err(ApiError::Xml)?,
        )
            .into_response());
    } else if let Some(selected_file) = find_file(&project_id, &vnum, &version, &processed_file) {
        if file.ends_with(".sha1") {
            if let Some(hash) = selected_file.hashes.get("sha1") {
                return Ok(hash.clone().into_response());
            }
        } else if file.ends_with(".sha512") {
            if let Some(hash) = selected_file.hashes.get("sha512") {
                return Ok(hash.clone().into_response());
            }
        } else {
            return Ok(Redirect::temporary(&*selected_file.url).into_response());
        }
    }

    Err(ApiError::NotFound)
}
