use crate::database::models::categories::Loader;
use crate::database::models::project_item::QueryProject;
use crate::database::models::version_item::{QueryFile, QueryVersion};
use crate::models::pats::Scopes;
use crate::models::projects::{ProjectId, VersionId};
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::{
    auth::{get_user_from_headers, is_authorized, is_authorized_version},
    database,
};
use actix_web::{get, route, web, HttpRequest, HttpResponse};
use sqlx::PgPool;
use std::collections::HashSet;
use yaserde_derive::YaSerialize;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(maven_metadata);
    cfg.service(version_file_sha512);
    cfg.service(version_file_sha1);
    cfg.service(version_file);
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

#[get("maven/modrinth/{id}/maven-metadata.xml")]
pub async fn maven_metadata(
    req: HttpRequest,
    params: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let project_id = params.into_inner().0;
    let Some(project) = database::models::Project::get(&project_id, &**pool, &redis).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if !is_authorized(&project.inner, &user_option, &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    let version_names = sqlx::query!(
        "
        SELECT id, version_number, version_type
        FROM versions
        WHERE mod_id = $1 AND status = ANY($2)
        ORDER BY date_published ASC
        ",
        project.inner.id as database::models::ids::ProjectId,
        &*crate::models::projects::VersionStatus::iterator()
            .filter(|x| x.is_listed())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
    )
    .fetch_all(&**pool)
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

    Ok(HttpResponse::Ok()
        .content_type("text/xml")
        .body(yaserde::ser::to_string(&respdata).map_err(ApiError::Xml)?))
}

async fn find_version(
    project: &QueryProject,
    vcoords: &String,
    pool: &PgPool,
    redis: &deadpool_redis::Pool,
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
        return Ok(exact_matches.get(0).map(|x| (*x).clone()));
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
        .into_iter()
        .filter(|x| {
            let mut bool = x.inner.version_number == vnumber;

            if !loaders.is_empty() {
                bool &= x.loaders.iter().any(|y| loaders.contains(y));
            }
            if !game_versions.is_empty() {
                bool &= x.game_versions.iter().any(|y| game_versions.contains(y));
            }

            bool
        })
        .collect::<Vec<_>>();

    Ok(matched.get(0).cloned())
}

fn find_file<'a>(
    project_id: &str,
    vcoords: &str,
    project: &QueryProject,
    version: &'a QueryVersion,
    file: &str,
) -> Option<&'a QueryFile> {
    if let Some(selected_file) = version.files.iter().find(|x| x.filename == file) {
        return Some(selected_file);
    }

    let fileext = match project.project_type.as_str() {
        "mod" => "jar",
        "modpack" => "mrpack",
        _ => return None,
    };

    if file == format!("{}-{}.{}", &project_id, &vcoords, fileext) {
        version
            .files
            .iter()
            .find(|x| x.primary)
            .or_else(|| version.files.iter().last())
    } else {
        None
    }
}

#[route(
    "maven/modrinth/{id}/{versionnum}/{file}",
    method = "GET",
    method = "HEAD"
)]
pub async fn version_file(
    req: HttpRequest,
    params: web::Path<(String, String, String)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (project_id, vnum, file) = params.into_inner();
    let Some(project) = database::models::Project::get(&project_id, &**pool, &redis).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if !is_authorized(&project.inner, &user_option, &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    let Some(version) = find_version(&project, &vnum, &pool, &redis).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    if !is_authorized_version(&version.inner, &user_option, &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    if file == format!("{}-{}.pom", &project_id, &vnum) {
        let respdata = MavenPom {
            schema_location:
                "http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd"
                    .to_string(),
            xsi: "http://www.w3.org/2001/XMLSchema-instance".to_string(),
            model_version: "4.0.0".to_string(),
            group_id: "maven.modrinth".to_string(),
            artifact_id: project_id,
            version: vnum,
            name: project.inner.title,
            description: project.inner.description,
        };
        return Ok(HttpResponse::Ok()
            .content_type("text/xml")
            .body(yaserde::ser::to_string(&respdata).map_err(ApiError::Xml)?));
    } else if let Some(selected_file) = find_file(&project_id, &vnum, &project, &version, &file) {
        return Ok(HttpResponse::TemporaryRedirect()
            .append_header(("location", &*selected_file.url))
            .body(""));
    }

    Ok(HttpResponse::NotFound().body(""))
}

#[get("maven/modrinth/{id}/{versionnum}/{file}.sha1")]
pub async fn version_file_sha1(
    req: HttpRequest,
    params: web::Path<(String, String, String)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (project_id, vnum, file) = params.into_inner();
    let Some(project) = database::models::Project::get(&project_id, &**pool, &redis).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if !is_authorized(&project.inner, &user_option, &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    let Some(version) = find_version(&project, &vnum, &pool, &redis).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    if !is_authorized_version(&version.inner, &user_option, &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    Ok(find_file(&project_id, &vnum, &project, &version, &file)
        .and_then(|file| file.hashes.get("sha1"))
        .map(|hash_str| HttpResponse::Ok().body(hash_str.clone()))
        .unwrap_or_else(|| HttpResponse::NotFound().body("")))
}

#[get("maven/modrinth/{id}/{versionnum}/{file}.sha512")]
pub async fn version_file_sha512(
    req: HttpRequest,
    params: web::Path<(String, String, String)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (project_id, vnum, file) = params.into_inner();
    let Some(project) = database::models::Project::get(&project_id, &**pool, &redis).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if !is_authorized(&project.inner, &user_option, &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    let Some(version) = find_version(&project, &vnum, &pool, &redis).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    if !is_authorized_version(&version.inner, &user_option, &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    Ok(find_file(&project_id, &vnum, &project, &version, &file)
        .and_then(|file| file.hashes.get("sha512"))
        .map(|hash_str| HttpResponse::Ok().body(hash_str.clone()))
        .unwrap_or_else(|| HttpResponse::NotFound().body("")))
}
