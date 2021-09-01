use crate::models::ids::{ProjectId, UserId, VersionId};
use crate::models::reports::{ItemType, Report};
use crate::routes::ApiError;
use crate::util::auth::{check_is_moderator_from_headers, get_user_from_headers};
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse};
use futures::StreamExt;
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct CreateReport {
    pub report_type: String,
    pub item_id: String,
    pub item_type: ItemType,
    pub body: String,
}

#[post("report")]
pub async fn report_create(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    mut body: web::Payload,
) -> Result<HttpResponse, ApiError> {
    let mut transaction = pool.begin().await?;

    let current_user = get_user_from_headers(req.headers(), &mut *transaction).await?;

    let mut bytes = web::BytesMut::new();
    while let Some(item) = body.next().await {
        bytes.extend_from_slice(&item.map_err(|_| {
            ApiError::InvalidInputError("Error while parsing request payload!".to_string())
        })?);
    }
    let new_report: CreateReport = serde_json::from_slice(bytes.as_ref())?;

    let id = crate::database::models::generate_report_id(&mut transaction).await?;
    let report_type = crate::database::models::categories::ReportType::get_id(
        &*new_report.report_type,
        &mut *transaction,
    )
    .await?
    .ok_or_else(|| {
        ApiError::InvalidInputError(format!("Invalid report type: {}", new_report.report_type))
    })?;
    let mut report = crate::database::models::report_item::Report {
        id,
        report_type_id: report_type,
        project_id: None,
        version_id: None,
        user_id: None,
        body: new_report.body.clone(),
        reporter: current_user.id.into(),
        created: chrono::Utc::now(),
    };

    match new_report.item_type {
        ItemType::Project => {
            report.project_id = Some(
                serde_json::from_str::<ProjectId>(&*format!("\"{}\"", new_report.item_id))?.into(),
            )
        }
        ItemType::Version => {
            report.version_id = Some(
                serde_json::from_str::<VersionId>(&*format!("\"{}\"", new_report.item_id))?.into(),
            )
        }
        ItemType::User => {
            report.user_id = Some(
                serde_json::from_str::<UserId>(&*format!("\"{}\"", new_report.item_id))?.into(),
            )
        }
        ItemType::Unknown => {
            return Err(ApiError::InvalidInputError(format!(
                "Invalid report item type: {}",
                new_report.item_type.as_str()
            )))
        }
    }

    report.insert(&mut transaction).await?;
    transaction.commit().await?;

    Ok(HttpResponse::Ok().json(Report {
        id: id.into(),
        report_type: new_report.report_type.clone(),
        item_id: new_report.item_id.clone(),
        item_type: new_report.item_type.clone(),
        reporter: current_user.id,
        body: new_report.body.clone(),
        created: chrono::Utc::now(),
    }))
}

#[derive(Deserialize)]
pub struct ResultCount {
    #[serde(default = "default_count")]
    count: i16,
}

fn default_count() -> i16 {
    100
}

#[get("report")]
pub async fn reports(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    count: web::Query<ResultCount>,
) -> Result<HttpResponse, ApiError> {
    check_is_moderator_from_headers(req.headers(), &**pool).await?;

    use futures::stream::TryStreamExt;

    let report_ids = sqlx::query!(
        "
        SELECT id FROM reports
        ORDER BY created ASC
        LIMIT $1;
        ",
        count.count as i64
    )
    .fetch_many(&**pool)
    .try_filter_map(|e| async {
        Ok(e.right()
            .map(|m| crate::database::models::ids::ReportId(m.id)))
    })
    .try_collect::<Vec<crate::database::models::ids::ReportId>>()
    .await?;

    let query_reports =
        crate::database::models::report_item::Report::get_many(report_ids, &**pool).await?;

    let mut reports = Vec::new();

    for x in query_reports {
        let mut item_id = "".to_string();
        let mut item_type = ItemType::Unknown;

        if let Some(project_id) = x.project_id {
            item_id = serde_json::to_string::<ProjectId>(&project_id.into())?;
            item_type = ItemType::Project;
        } else if let Some(version_id) = x.version_id {
            item_id = serde_json::to_string::<VersionId>(&version_id.into())?;
            item_type = ItemType::Version;
        } else if let Some(user_id) = x.user_id {
            item_id = serde_json::to_string::<UserId>(&user_id.into())?;
            item_type = ItemType::User;
        }

        reports.push(Report {
            id: x.id.into(),
            report_type: x.report_type,
            item_id,
            item_type,
            reporter: x.reporter.into(),
            body: x.body,
            created: x.created,
        })
    }

    Ok(HttpResponse::Ok().json(reports))
}

#[delete("report/{id}")]
pub async fn delete_report(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    info: web::Path<(crate::models::reports::ReportId,)>,
) -> Result<HttpResponse, ApiError> {
    check_is_moderator_from_headers(req.headers(), &**pool).await?;

    let result = crate::database::models::report_item::Report::remove_full(
        info.into_inner().0.into(),
        &**pool,
    )
    .await?;

    if result.is_some() {
        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
