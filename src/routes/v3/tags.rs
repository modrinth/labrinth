use super::ApiError;
use crate::database::models;
use crate::database::models::categories::{DonationPlatform, ProjectType, ReportType};
use crate::database::models::loader_fields::{Loader, GameVersion};
use crate::database::redis::RedisPool;
use actix_web::{get, web, HttpResponse};
use chrono::{DateTime, Utc};
use models::categories::{Category};
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("tag")
            .service(category_list)
            // .service(loader_list)
            // .service(game_version_list)
            // .service(side_type_list),
    );
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CategoryData {
    icon: String,
    name: String,
    project_type: String,
    header: String,
}

#[get("category")]
pub async fn category_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let results = Category::list(&**pool, &redis)
        .await?
        .into_iter()
        .map(|x| CategoryData {
            icon: x.icon,
            name: x.category,
            project_type: x.project_type,
            header: x.header,
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(results))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LoaderData {
    icon: String,
    name: String,
    supported_project_types: Vec<String>,
}

// #[derive(serde::Deserialize)]
// struct LoaderList {
//     game: String
// }
// #[get("loader")]
// pub async fn loader_list(
//     data: web::Query<LoaderList>,
//     pool: web::Data<PgPool>,
//     redis: web::Data<RedisPool>,
// ) -> Result<HttpResponse, ApiError> {
//     let mut results = Loader::list(&data.game,&**pool, &redis)
//         .await?
//         .into_iter()
//         .map(|x| LoaderData {
//             icon: x.icon,
//             name: x.loader,
//             supported_project_types: x.supported_project_types,
//         })
//         .collect::<Vec<_>>();

//     results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

//     Ok(HttpResponse::Ok().json(results))
// }

#[derive(serde::Serialize)]
pub struct GameVersionQueryData {
    pub version: String,
    pub version_type: String,
    pub date: DateTime<Utc>,
    pub major: bool,
}

#[derive(serde::Deserialize)]
pub struct GameVersionQuery {
    #[serde(rename = "type")]
    type_: Option<String>,
    major: Option<bool>,
}

// #[get("game_version")]
// pub async fn game_version_list(
//     pool: web::Data<PgPool>,
//     query: web::Query<GameVersionQuery>,
//     redis: web::Data<RedisPool>,
// ) -> Result<HttpResponse, ApiError> {
//     let results: Vec<GameVersionQueryData> = if query.type_.is_some() || query.major.is_some() {
//         GameVersion::list_filter(query.type_.as_deref(), query.major, &**pool, &redis).await?
//     } else {
//         GameVersion::list(&**pool, &redis).await?
//     }
//     .into_iter()
//     .map(|x| GameVersionQueryData {
//         version: x.version,
//         version_type: x.type_,
//         date: x.created,
//         major: x.major,
//     })
//     .collect();

//     Ok(HttpResponse::Ok().json(results))
// }

#[derive(serde::Serialize)]
pub struct License {
    short: String,
    name: String,
}

// #[get("side_type")]
// pub async fn side_type_list(
//     pool: web::Data<PgPool>,
//     redis: web::Data<RedisPool>,
// ) -> Result<HttpResponse, ApiError> {
//     let results = SideType::list(&**pool, &redis).await?;
//     Ok(HttpResponse::Ok().json(results))
// }
