use crate::models::ids::ProjectId;
use crate::routes::ApiError;
use crate::util::guards::admin_key_guard;
use crate::DownloadQueue;
use actix_web::{patch, post, web, HttpResponse};
use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct DownloadBody {
    pub url: String,
    pub hash: ProjectId,
    pub version_name: String,
}

// This is an internal route, cannot be used without key
#[patch("/_count-download", guard = "admin_key_guard")]
pub async fn count_download(
    pool: web::Data<PgPool>,
    download_body: web::Json<DownloadBody>,
    download_queue: web::Data<Arc<DownloadQueue>>,
) -> Result<HttpResponse, ApiError> {
    let project_id: crate::database::models::ids::ProjectId =
        download_body.hash.into();

    let id_option = crate::models::ids::base62_impl::parse_base62(
        &download_body.version_name,
    )
    .ok()
    .map(|x| x as i64);

    let (version_id, project_id) = if let Some(version) = sqlx::query!(
        "SELECT id, mod_id FROM versions
         WHERE ((version_number = $1 OR id = $3) AND mod_id = $2)",
        download_body.version_name,
        project_id as crate::database::models::ids::ProjectId,
        id_option
    )
    .fetch_optional(pool.as_ref())
    .await?
    {
        (version.id, version.mod_id)
    } else if let Some(version) = sqlx::query!(
        "
        SELECT v.id id, v.mod_id project_id FROM files f
        INNER JOIN versions v ON v.id = f.version_id
        WHERE f.url = $1
        ",
        download_body.url,
    )
    .fetch_optional(pool.as_ref())
    .await?
    {
        (version.id, version.project_id)
    } else {
        return Err(ApiError::InvalidInput(
            "Specified version does not exist!".to_string(),
        ));
    };

    download_queue
        .add(
            crate::database::models::ProjectId(project_id),
            crate::database::models::VersionId(version_id),
        )
        .await;

    let client = reqwest::Client::new();

    client
        .post(format!("{}downloads", dotenv::var("ARIADNE_URL")?))
        .header("Modrinth-Admin", dotenv::var("ARIADNE_ADMIN_KEY")?)
        .json(&json!({
            "url": download_body.url,
            "project_id": download_body.hash
        }))
        .send()
        .await
        .ok();

    Ok(HttpResponse::NoContent().body(""))
}

#[derive(Deserialize)]
pub struct PayoutData {
    amount: Decimal,
    date: DateTime<Utc>,
}

#[post("/_process_payout", guard = "admin_key_guard")]
pub async fn process_payout(
    pool: web::Data<PgPool>,
    data: web::Json<PayoutData>,
) -> Result<HttpResponse, ApiError> {
    let start = data.date.date().and_hms(0, 0, 0);

    let client = reqwest::Client::new();
    let mut transaction = pool.begin().await?;

    #[derive(Deserialize)]
    struct PayoutMultipliers {
        sum: u64,
        values: HashMap<i64, u64>,
    }

    let multipliers: PayoutMultipliers = client
        .get(format!(
            "{}multipliers?start_date=\"{}\"",
            dotenv::var("ARIADNE_URL")?,
            start.to_rfc3339(),
        ))
        .header("Modrinth-Admin", dotenv::var("ARIADNE_ADMIN_KEY")?)
        .send()
        .await
        .map_err(|_| {
            ApiError::Analytics(
                "Error while fetching payout multipliers!".to_string(),
            )
        })?
        .json()
        .await
        .map_err(|_| {
            ApiError::Analytics(
                "Error while deserializing payout multipliers!".to_string(),
            )
        })?;

    sqlx::query!(
        "
        DELETE FROM payouts_values
        WHERE created = $1
        ",
        start
    )
    .execute(&mut *transaction)
    .await?;

    struct Project {
        project_type: String,
        team_members: DashSet<(i64, Decimal)>,
    }

    let projects_map: DashMap<i64, Project> = DashMap::new();

    use futures::TryStreamExt;
    sqlx::query!(
        "
        SELECT m.id id, tm.user_id user_id, tm.payouts_split payouts_split, pt.name project_type
        FROM mods m
        INNER JOIN team_members tm on m.team_id = tm.team_id
        INNER JOIN project_types pt ON pt.id = m.project_type
        WHERE m.id = ANY($1)
        ",
        &multipliers.values.keys().map(|x| *x).collect::<Vec<i64>>()
    )
        .fetch_many(&mut *transaction)
        .try_for_each_concurrent(None, |e| async {
            if let Some(row) = e.right() {
                if let Some(project) = projects_map.get_mut(&row.id) {
                    project.team_members.insert((row.user_id, row.payouts_split));
                } else {
                    let team_members = DashSet::new();
                    team_members.insert((row.user_id, row.payouts_split));

                    projects_map.insert(row.id, Project {
                        project_type: row.project_type,
                        team_members,
                    });
                }
            }

            Ok(())
        })
        .await?;

    // Specific Payout Conditions (ex: modpack payout split)
    // let mut projects_split_dependencies = Vec::new();
    //
    // for (id, project) in &projects_map {
    //     if project.project_type == "modpack" {
    //         projects_split_dependencies.push(id);
    //     }
    // }
    //
    // if !projects_split_dependencies.is_empty() {
    //     sqlx::query!(
    //         "
    //         SELECT m.id id, tm.user_id user_id, tm.payouts_split payouts_split, pt.name project_type
    //         FROM dependencies d
    //         INNER JOIN versions v ON v.id = d.dependency_id
    //         INNER JOIN mods m ON v.mod_id = m.id
    //         INNER JOIN team_members tm on m.team_id = tm.team_id
    //         WHERE d.dependent_id = ANY($1)
    //         ",
    //         projects_split_dependencies
    //     )
    //         .fetch_many(&mut *transaction)
    //         .try_for_each_concurrent(None, |e| async {
    //             // if let Some(row) = e.right() {
    //             //     if let Some(project) = projects_map.get_mut(&row.id) {
    //             //         project.team_members.insert((row.user_id, row.payouts_split));
    //             //     } else {
    //             //         let team_members = DashSet::new();
    //             //         team_members.insert((row.user_id, row.payouts_split));
    //             //
    //             //         projects_map.insert(row.id, Project {
    //             //             project_type: row.project_type,
    //             //             team_members,
    //             //         });
    //             //     }
    //             // }
    //
    //             Ok(())
    //         })
    //         .await?;
    // }

    // TODO: Handle modpack split
    // TODO: for users return payout: return value if payout is withdrawable

    for (id, project) in projects_map {
        if let Some(value) = &multipliers.values.get(&id) {
            let project_multiplier: Decimal =
                Decimal::from(**value) / Decimal::from(multipliers.sum);

            let sum_splits: Decimal =
                project.team_members.iter().map(|x| x.1).sum();

            for (user_id, split) in project.team_members {
                let payout: Decimal =
                    data.amount * project_multiplier * (split / sum_splits);

                sqlx::query!(
                    "
                    INSERT INTO payouts_values (user_id, mod_id, amount, created)
                    VALUES ($1, $2, $3, $4)
                    ",
                    user_id,
                    id,
                    payout,
                    start
                )
                    .execute(&mut *transaction)
                    .await?;
            }
        }
    }

    Ok(HttpResponse::NoContent().body(""))
}
