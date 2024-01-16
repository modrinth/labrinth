use crate::auth::{check_is_moderator_from_headers, get_user_from_headers};
use crate::database;
use crate::database::models::image_item;
use crate::database::models::thread_item::{ThreadBuilder, ThreadMessageBuilder};
use crate::database::redis::RedisPool;
use crate::models::ids::ImageId;
use crate::models::ids::{base62_impl::parse_base62, ProjectId, UserId, VersionId};
use crate::models::images::{Image, ImageContext};
use crate::models::pats::Scopes;
use crate::models::reports::{ItemType, Report};
use crate::models::threads::{MessageBody, ThreadType};
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::img;
use axum::extract::{ConnectInfo, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Extension, Json, Router};
use chrono::Utc;
use serde::Deserialize;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use validator::Validate;

pub fn config() -> Router {
    Router::new()
        .route("/report", get(reports).post(report_create))
        .route("/reports", get(reports_get))
        .route(
            "/report/:id",
            get(report_get).patch(report_edit).delete(report_delete),
        )
}

#[derive(Deserialize, Validate)]
pub struct CreateReport {
    pub report_type: String,
    pub item_id: String,
    pub item_type: ItemType,
    pub body: String,
    // Associations to uploaded images
    #[validate(length(max = 10))]
    #[serde(default)]
    pub uploaded_images: Vec<ImageId>,
}

pub async fn report_create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(new_report): Json<CreateReport>,
) -> Result<Json<Report>, ApiError> {
    let mut transaction = pool.begin().await?;

    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::REPORT_CREATE]),
    )
    .await?
    .1;

    let id = crate::database::models::generate_report_id(&mut transaction).await?;
    let report_type = crate::database::models::categories::ReportType::get_id(
        &new_report.report_type,
        &mut *transaction,
    )
    .await?
    .ok_or_else(|| {
        ApiError::InvalidInput(format!("Invalid report type: {}", new_report.report_type))
    })?;

    let mut report = crate::database::models::report_item::Report {
        id,
        report_type_id: report_type,
        project_id: None,
        version_id: None,
        user_id: None,
        body: new_report.body.clone(),
        reporter: current_user.id.into(),
        created: Utc::now(),
        closed: false,
    };

    match new_report.item_type {
        ItemType::Project => {
            let project_id = ProjectId(parse_base62(new_report.item_id.as_str())?);

            let result = sqlx::query!(
                "SELECT EXISTS(SELECT 1 FROM mods WHERE id = $1)",
                project_id.0 as i64
            )
            .fetch_one(&mut *transaction)
            .await?;

            if !result.exists.unwrap_or(false) {
                return Err(ApiError::InvalidInput(format!(
                    "Project could not be found: {}",
                    new_report.item_id
                )));
            }

            report.project_id = Some(project_id.into())
        }
        ItemType::Version => {
            let version_id = VersionId(parse_base62(new_report.item_id.as_str())?);

            let result = sqlx::query!(
                "SELECT EXISTS(SELECT 1 FROM versions WHERE id = $1)",
                version_id.0 as i64
            )
            .fetch_one(&mut *transaction)
            .await?;

            if !result.exists.unwrap_or(false) {
                return Err(ApiError::InvalidInput(format!(
                    "Version could not be found: {}",
                    new_report.item_id
                )));
            }

            report.version_id = Some(version_id.into())
        }
        ItemType::User => {
            let user_id = UserId(parse_base62(new_report.item_id.as_str())?);

            let result = sqlx::query!(
                "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)",
                user_id.0 as i64
            )
            .fetch_one(&mut *transaction)
            .await?;

            if !result.exists.unwrap_or(false) {
                return Err(ApiError::InvalidInput(format!(
                    "User could not be found: {}",
                    new_report.item_id
                )));
            }

            report.user_id = Some(user_id.into())
        }
        ItemType::Unknown => {
            return Err(ApiError::InvalidInput(format!(
                "Invalid report item type: {}",
                new_report.item_type.as_str()
            )))
        }
    }

    report.insert(&mut transaction).await?;

    for image_id in new_report.uploaded_images {
        if let Some(db_image) =
            image_item::Image::get(image_id.into(), &mut *transaction, &redis).await?
        {
            let image: Image = db_image.into();
            if !matches!(image.context, ImageContext::Report { .. })
                || image.context.inner_id().is_some()
            {
                return Err(ApiError::InvalidInput(format!(
                    "Image {} is not unused and in the 'report' context",
                    image_id
                )));
            }

            sqlx::query!(
                "
                UPDATE uploaded_images
                SET report_id = $1
                WHERE id = $2
                ",
                id.0 as i64,
                image_id.0 as i64
            )
            .execute(&mut *transaction)
            .await?;

            image_item::Image::clear_cache(image.id.into(), &redis).await?;
        } else {
            return Err(ApiError::InvalidInput(format!(
                "Image {} could not be found",
                image_id
            )));
        }
    }

    let thread_id = ThreadBuilder {
        type_: ThreadType::Report,
        members: vec![],
        project_id: None,
        report_id: Some(report.id),
    }
    .insert(&mut transaction)
    .await?;

    transaction.commit().await?;

    Ok(Json(Report {
        id: id.into(),
        report_type: new_report.report_type.clone(),
        item_id: new_report.item_id.clone(),
        item_type: new_report.item_type.clone(),
        reporter: current_user.id,
        body: new_report.body.clone(),
        created: Utc::now(),
        closed: false,
        thread_id: thread_id.into(),
    }))
}

#[derive(Deserialize)]
pub struct ReportsRequestOptions {
    #[serde(default = "default_count")]
    pub count: i16,
    #[serde(default = "default_all")]
    pub all: bool,
}

fn default_count() -> i16 {
    100
}
fn default_all() -> bool {
    true
}

pub async fn reports(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Query(count): Query<ReportsRequestOptions>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<Report>>, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::REPORT_READ]),
    )
    .await?
    .1;

    use futures::stream::TryStreamExt;

    let report_ids = if user.role.is_mod() && count.all {
        sqlx::query!(
            "
            SELECT id FROM reports
            WHERE closed = FALSE
            ORDER BY created ASC
            LIMIT $1;
            ",
            count.count as i64
        )
        .fetch_many(&pool)
        .try_filter_map(|e| async {
            Ok(e.right()
                .map(|m| crate::database::models::ids::ReportId(m.id)))
        })
        .try_collect::<Vec<crate::database::models::ids::ReportId>>()
        .await?
    } else {
        sqlx::query!(
            "
            SELECT id FROM reports
            WHERE closed = FALSE AND reporter = $1
            ORDER BY created ASC
            LIMIT $2;
            ",
            user.id.0 as i64,
            count.count as i64
        )
        .fetch_many(&pool)
        .try_filter_map(|e| async {
            Ok(e.right()
                .map(|m| crate::database::models::ids::ReportId(m.id)))
        })
        .try_collect::<Vec<crate::database::models::ids::ReportId>>()
        .await?
    };

    let query_reports =
        crate::database::models::report_item::Report::get_many(&report_ids, &pool).await?;

    let mut reports: Vec<Report> = Vec::new();

    for x in query_reports {
        reports.push(x.into());
    }

    Ok(Json(reports))
}

#[derive(Deserialize)]
pub struct ReportIds {
    pub ids: String,
}

pub async fn reports_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<ReportIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<Report>>, ApiError> {
    let report_ids: Vec<crate::database::models::ids::ReportId> =
        serde_json::from_str::<Vec<crate::models::ids::ReportId>>(&ids.ids)?
            .into_iter()
            .map(|x| x.into())
            .collect();

    let reports_data =
        crate::database::models::report_item::Report::get_many(&report_ids, &pool).await?;

    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::REPORT_READ]),
    )
    .await?
    .1;

    let all_reports = reports_data
        .into_iter()
        .filter(|x| user.role.is_mod() || x.reporter == user.id.into())
        .map(|x| x.into())
        .collect::<Vec<Report>>();

    Ok(Json(all_reports))
}

pub async fn report_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Path(id): Path<crate::models::reports::ReportId>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Report>, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::REPORT_READ]),
    )
    .await?
    .1;

    let report = crate::database::models::report_item::Report::get(id.into(), &pool).await?;

    if let Some(report) = report {
        if !user.role.is_mod() && report.reporter != user.id.into() {
            return Err(ApiError::NotFound);
        }

        let report: Report = report.into();
        Ok(Json(report))
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(Deserialize, Validate)]
pub struct EditReport {
    #[validate(length(max = 65536))]
    pub body: Option<String>,
    pub closed: Option<bool>,
}

pub async fn report_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Path(id): Path<crate::models::reports::ReportId>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    edit_report: Json<EditReport>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::REPORT_WRITE]),
    )
    .await?
    .1;

    let id = id.into();
    let report = crate::database::models::report_item::Report::get(id, &pool).await?;

    if let Some(report) = report {
        if !user.role.is_mod() && report.reporter != user.id.into() {
            return Err(ApiError::NotFound);
        }

        let mut transaction = pool.begin().await?;

        if let Some(edit_body) = &edit_report.body {
            sqlx::query!(
                "
                UPDATE reports
                SET body = $1
                WHERE (id = $2)
                ",
                edit_body,
                id as crate::database::models::ids::ReportId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(edit_closed) = edit_report.closed {
            if !user.role.is_mod() {
                return Err(ApiError::InvalidInput(
                    "You cannot reopen a report!".to_string(),
                ));
            }

            ThreadMessageBuilder {
                author_id: Some(user.id.into()),
                body: if !edit_closed && report.closed {
                    MessageBody::ThreadReopen
                } else {
                    MessageBody::ThreadClosure
                },
                thread_id: report.thread_id,
            }
            .insert(&mut transaction)
            .await?;

            sqlx::query!(
                "
                UPDATE reports
                SET closed = $1
                WHERE (id = $2)
                ",
                edit_closed,
                id as crate::database::models::ids::ReportId,
            )
            .execute(&mut *transaction)
            .await?;

            sqlx::query!(
                "
                UPDATE threads
                SET show_in_mod_inbox = $1
                WHERE id = $2
                ",
                !(edit_closed || report.closed),
                report.thread_id.0,
            )
            .execute(&mut *transaction)
            .await?;
        }

        // delete any images no longer in the body
        let checkable_strings: Vec<&str> = vec![&edit_report.body]
            .into_iter()
            .filter_map(|x: &Option<String>| x.as_ref().map(|y| y.as_str()))
            .collect();
        let image_context = ImageContext::Report {
            report_id: Some(id.into()),
        };
        img::delete_unused_images(image_context, checkable_strings, &mut transaction, &redis)
            .await?;

        transaction.commit().await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

pub async fn report_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Path(id): Path<crate::models::reports::ReportId>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    check_is_moderator_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::REPORT_DELETE]),
    )
    .await?;

    let mut transaction = pool.begin().await?;

    let context = ImageContext::Report {
        report_id: Some(id),
    };
    let uploaded_images =
        database::models::Image::get_many_contexted(context, &mut transaction).await?;
    for image in uploaded_images {
        image_item::Image::remove(image.id, &mut transaction, &redis).await?;
    }

    let result =
        crate::database::models::report_item::Report::remove_full(id.into(), &mut transaction)
            .await?;
    transaction.commit().await?;

    if result.is_some() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
