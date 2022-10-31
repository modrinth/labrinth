use crate::models::ids::ProjectId;
use crate::routes::ApiError;
use crate::util::auth::check_is_admin_from_headers;
use crate::util::guards::admin_key_guard;
use crate::util::payout_calc::get_claimable_time;
use crate::DownloadQueue;
use actix_web::{get, patch, post, web, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};
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
        .post(format!("{}downloads", dotenvy::var("ARIADNE_URL")?))
        .header("Modrinth-Admin", dotenvy::var("ARIADNE_ADMIN_KEY")?)
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
        // user_id, payouts_split
        team_members: Vec<(i64, Decimal)>,
        // user_id, payouts_split
        split_team_members: Vec<(i64, Decimal)>,
    }

    let mut projects_map: HashMap<i64, Project> = HashMap::new();

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
        .try_for_each(|e| {
            if let Some(row) = e.right() {
                if let Some(project) = projects_map.get_mut(&row.id) {
                    project.team_members.push((row.user_id, row.payouts_split));
                } else {
                    projects_map.insert(row.id, Project {
                        project_type: row.project_type,
                        team_members: vec![(row.user_id, row.payouts_split)],
                        split_team_members: Default::default()
                    });
                }
            }

            futures::future::ready(Ok(()))
        })
        .await?;

    // Specific Payout Conditions (ex: modpack payout split)
    let mut projects_split_dependencies = Vec::new();

    for (id, project) in &projects_map {
        if project.project_type == "modpack" {
            projects_split_dependencies.push(*id);
        }
    }

    if !projects_split_dependencies.is_empty() {
        // (dependent_id, (dependency_id, times_depended))
        let mut project_dependencies: HashMap<i64, Vec<(i64, i64)>> =
            HashMap::new();
        // dependency_ids to fetch team members from
        let mut fetch_team_members: Vec<i64> = Vec::new();

        sqlx::query!(
            "
            SELECT mv.mod_id, m.id, COUNT(m.id) times_depended FROM versions mv
            INNER JOIN dependencies d ON d.dependent_id = mv.id
            INNER JOIN versions v ON d.dependency_id = v.id
            INNER JOIN mods m ON v.mod_id = m.id OR d.mod_dependency_id = m.id
            WHERE mv.mod_id = ANY($1)
            group by mv.mod_id, m.id;
            ",
            &projects_split_dependencies
        )
        .fetch_many(&mut *transaction)
        .try_for_each(|e| {
            if let Some(row) = e.right() {
                fetch_team_members.push(row.id);

                if let Some(project) = project_dependencies.get_mut(&row.mod_id)
                {
                    project.push((row.id, row.times_depended.unwrap_or(0)));
                } else {
                    project_dependencies.insert(
                        row.mod_id,
                        vec![(row.id, row.times_depended.unwrap_or(0))],
                    );
                }
            }

            futures::future::ready(Ok(()))
        })
        .await?;

        // (project_id, (user_id, payouts_split))
        let mut team_members: HashMap<i64, Vec<(i64, Decimal)>> =
            HashMap::new();

        sqlx::query!(
            "
            SELECT m.id id, tm.user_id user_id, tm.payouts_split payouts_split
            FROM mods m
            INNER JOIN team_members tm on m.team_id = tm.team_id
            WHERE m.id = ANY($1)
            ",
            &*fetch_team_members
        )
        .fetch_many(&mut *transaction)
        .try_for_each(|e| {
            if let Some(row) = e.right() {
                if let Some(project) = team_members.get_mut(&row.id) {
                    project.push((row.user_id, row.payouts_split));
                } else {
                    team_members
                        .insert(row.id, vec![(row.user_id, row.payouts_split)]);
                }
            }

            futures::future::ready(Ok(()))
        })
        .await?;

        for (project, dependencies) in project_dependencies {
            let dep_sum: i64 = dependencies.iter().map(|x| x.1).sum();

            let project = projects_map.get_mut(&project);

            if let Some(project) = project {
                for dependency in dependencies {
                    let project_multiplier: Decimal =
                        Decimal::from(dependency.1) / Decimal::from(dep_sum);

                    if let Some(members) = team_members.get(&dependency.0) {
                        let members_sum: Decimal =
                            members.iter().map(|x| x.1).sum();

                        for member in members {
                            let member_multiplier: Decimal =
                                member.1 / members_sum;
                            project.split_team_members.push((
                                member.0,
                                member_multiplier * project_multiplier,
                            ));
                        }
                    }
                }
            }
        }
    }

    for (id, project) in projects_map {
        if let Some(value) = &multipliers.values.get(&id) {
            let project_multiplier: Decimal =
                Decimal::from(**value) / Decimal::from(multipliers.sum);

            let default_split_given = Decimal::from(1);
            let split_given = Decimal::from(1) / Decimal::from(5);
            let split_retention = Decimal::from(4) / Decimal::from(5);

            let sum_splits: Decimal =
                project.team_members.iter().map(|x| x.1).sum();
            let sum_tm_splits: Decimal =
                project.split_team_members.iter().map(|x| x.1).sum();

            for (user_id, split) in project.team_members {
                let payout: Decimal = data.amount
                    * project_multiplier
                    * (split / sum_splits)
                    * (if !project.split_team_members.is_empty() {
                        &split_given
                    } else {
                        &default_split_given
                    });

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

            for (user_id, split) in project.split_team_members {
                let payout: Decimal = data.amount
                    * project_multiplier
                    * (split / sum_tm_splits)
                    * split_retention;

                sqlx::query!(
                    "
                    INSERT INTO payouts_values (user_id, amount, created)
                    VALUES ($1, $2, $3)
                    ",
                    user_id,
                    payout,
                    start
                )
                .execute(&mut *transaction)
                .await?;
            }
        }
    }

    transaction.commit().await?;

    Ok(HttpResponse::NoContent().body(""))
}

#[get("/_get-payout-data")]
pub async fn get_payout_data(
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    check_is_admin_from_headers(req.headers(), &**pool).await?;

    use futures::stream::TryStreamExt;

    let payouts = sqlx::query!(
            "
            SELECT u.paypal_email, SUM(pv.amount) amount
            FROM payouts_values pv
            INNER JOIN users u ON pv.user_id = u.id AND u.paypal_email IS NOT NULL
            WHERE pv.created <= $1 AND pv.claimed = FALSE
            GROUP BY u.paypal_email
            ",
            get_claimable_time(Utc::now(), false)
        )
        .fetch_many(&**pool)
        .try_filter_map(|e| async {
            Ok(e.right().map(|r| (r.paypal_email.unwrap_or_default(), r.amount.unwrap_or_default())))
        })
        .try_collect::<HashMap<String, Decimal>>()
        .await?;

    Ok(HttpResponse::Ok().json(payouts))
}
