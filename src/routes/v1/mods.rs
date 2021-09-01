use crate::file_hosting::FileHost;
use crate::models::projects::SearchRequest;
use crate::routes::project_creation::{project_create_inner, undo_uploads, CreateError};
use crate::routes::projects::{convert_project, ProjectIds};
use crate::routes::ApiError;
use crate::search::{search_for_project, SearchConfig, SearchError};
use crate::util::auth::get_user_from_headers;
use crate::{database, models};
use actix_multipart::Multipart;
use actix_web::web;
use actix_web::web::Data;
use actix_web::{get, post, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResultSearchMod {
    pub mod_id: String,
    pub slug: Option<String>,
    pub author: String,
    pub title: String,
    pub description: String,
    pub categories: Vec<String>,
    pub versions: Vec<String>,
    pub downloads: i32,
    pub follows: i32,
    pub page_url: String,
    pub icon_url: String,
    pub author_url: String,
    pub date_created: String,
    pub date_modified: String,
    pub latest_version: String,
    pub license: String,
    pub client_side: String,
    pub server_side: String,
    pub host: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchResults {
    pub hits: Vec<ResultSearchMod>,
    pub offset: usize,
    pub limit: usize,
    pub total_hits: usize,
}

#[get("mod")]
pub async fn mod_search(
    web::Query(info): web::Query<SearchRequest>,
    config: web::Data<SearchConfig>,
) -> Result<HttpResponse, SearchError> {
    let results = search_for_project(&info, &**config).await?;
    Ok(HttpResponse::Ok().json(SearchResults {
        hits: results
            .hits
            .into_iter()
            .map(|x| ResultSearchMod {
                mod_id: x.project_id.clone(),
                slug: x.slug,
                author: x.author.clone(),
                title: x.title,
                description: x.description,
                categories: x.categories,
                versions: x.versions,
                downloads: x.downloads,
                follows: x.follows,
                page_url: format!("https://modrinth.com/mod/{}", x.project_id),
                icon_url: x.icon_url,
                author_url: format!("https://modrinth.com/user/{}", x.author),
                date_created: x.date_created,
                date_modified: x.date_modified,
                latest_version: x.latest_version,
                license: x.license,
                client_side: x.client_side,
                server_side: x.server_side,
                host: "modrinth".to_string(),
            })
            .collect(),
        offset: results.offset,
        limit: results.limit,
        total_hits: results.total_hits,
    }))
}

#[get("mods")]
pub async fn mods_get(
    req: HttpRequest,
    ids: web::Query<ProjectIds>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let project_ids = serde_json::from_str::<Vec<models::ids::ProjectId>>(&*ids.ids)?
        .into_iter()
        .map(|x| x.into())
        .collect();

    let projects_data = database::models::Project::get_many_full(project_ids, &**pool).await?;

    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();

    let mut projects = Vec::new();

    for project_data in projects_data {
        let mut authorized = !project_data.status.is_hidden();

        if let Some(user) = &user_option {
            if !authorized {
                if user.role.is_mod() {
                    authorized = true;
                } else {
                    let user_id: database::models::ids::UserId = user.id.into();

                    let project_exists = sqlx::query!(
                            "SELECT EXISTS(SELECT 1 FROM team_members WHERE team_id = $1 AND user_id = $2)",
                            project_data.inner.team_id as database::models::ids::TeamId,
                            user_id as database::models::ids::UserId,
                        )
                        .fetch_one(&**pool)
                        .await?
                        .exists;

                    authorized = project_exists.unwrap_or(false);
                }
            }
        }

        if authorized {
            projects.push(convert_project(project_data));
        }
    }

    Ok(HttpResponse::Ok().json(projects))
}

#[post("mod")]
pub async fn mod_create(
    req: HttpRequest,
    payload: Multipart,
    client: Data<PgPool>,
    file_host: Data<Arc<dyn FileHost + Send + Sync>>,
) -> Result<HttpResponse, CreateError> {
    let mut transaction = client.begin().await?;
    let mut uploaded_files = Vec::new();

    let result = project_create_inner(
        req,
        payload,
        &mut transaction,
        &***file_host,
        &mut uploaded_files,
    )
    .await;

    if result.is_err() {
        let undo_result = undo_uploads(&***file_host, &uploaded_files).await;
        let rollback_result = transaction.rollback().await;

        if let Err(e) = undo_result {
            return Err(e);
        }
        if let Err(e) = rollback_result {
            return Err(e.into());
        }
    } else {
        transaction.commit().await?;
    }

    result
}
