use super::ApiError;
use crate::auth::get_user_from_headers;
use crate::file_hosting::FileHost;
use crate::models;
use crate::models::teams::Permissions;
use crate::{database, Pepper};
use actix_web::{delete, get, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::borrow::Borrow;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct Algorithm {
    #[serde(default = "default_algorithm")]
    algorithm: String,
}

fn default_algorithm() -> String {
    "sha1".into()
}

// under /api/v1/version_file/{hash}
#[get("{version_id}")]
pub async fn get_version_from_hash(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    algorithm: web::Query<Algorithm>,
) -> Result<HttpResponse, ApiError> {
    let hash = info.into_inner().0.to_lowercase();

    let result = sqlx::query!(
        "
        SELECT f.version_id version_id FROM hashes h
        INNER JOIN files f ON h.file_id = f.id
        WHERE h.algorithm = $2 AND h.hash = $1
        ",
        hash.as_bytes(),
        algorithm.algorithm
    )
    .fetch_optional(&**pool)
    .await?;

    if let Some(id) = result {
        let version_data = database::models::Version::get_full(
            database::models::VersionId(id.version_id),
            &**pool,
        )
        .await?;

        if let Some(data) = version_data {
            Ok(HttpResponse::Ok().json(super::versions::convert_version(data)))
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Serialize, Deserialize)]
pub struct DownloadRedirect {
    pub url: String,
}

// under /api/v1/version_file/{hash}/download
#[allow(clippy::await_holding_refcell_ref)]
#[get("{version_id}/download")]
pub async fn download_version(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    algorithm: web::Query<Algorithm>,
    pepper: web::Data<Pepper>,
) -> Result<HttpResponse, ApiError> {
    let hash = info.into_inner().0.to_lowercase();

    let result = sqlx::query!(
        "
        SELECT f.url url, f.id id, f.version_id version_id, v.mod_id project_id FROM hashes h
        INNER JOIN files f ON h.file_id = f.id
        INNER JOIN versions v ON v.id = f.version_id
        WHERE h.algorithm = $2 AND h.hash = $1
        ",
        hash.as_bytes(),
        algorithm.algorithm
    )
    .fetch_optional(&**pool)
    .await?;

    if let Some(id) = result {
        let real_ip = req.connection_info();
        let ip_option = real_ip.borrow().remote_addr();

        if let Some(ip) = ip_option {
            let hash = sha1::Sha1::from(format!("{}{}", ip, pepper.pepper)).hexdigest();

            let download_exists = sqlx::query!(
                "SELECT EXISTS(SELECT 1 FROM downloads WHERE version_id = $1 AND date > (CURRENT_DATE - INTERVAL '30 minutes ago') AND identifier = $2)",
                id.version_id,
                hash,
            )
                .fetch_one(&**pool)
                .await
                ?
                .exists.unwrap_or(false);

            if !download_exists {
                sqlx::query!(
                    "
                    INSERT INTO downloads (
                        version_id, identifier
                    )
                    VALUES (
                        $1, $2
                    )
                    ",
                    id.version_id,
                    hash
                )
                .execute(&**pool)
                .await?;

                sqlx::query!(
                    "
                    UPDATE versions
                    SET downloads = downloads + 1
                    WHERE id = $1
                    ",
                    id.version_id,
                )
                .execute(&**pool)
                .await?;

                sqlx::query!(
                    "
                    UPDATE mods
                    SET downloads = downloads + 1
                    WHERE id = $1
                    ",
                    id.project_id,
                )
                .execute(&**pool)
                .await?;
            }
        }
        Ok(HttpResponse::TemporaryRedirect()
            .header("Location", &*id.url)
            .json(DownloadRedirect { url: id.url }))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

// under /api/v1/version_file/{hash}
#[delete("{version_id}")]
pub async fn delete_file(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    algorithm: web::Query<Algorithm>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let hash = info.into_inner().0.to_lowercase();

    let result = sqlx::query!(
        "
        SELECT f.id id, f.version_id version_id, f.filename filename, v.version_number version_number, v.mod_id project_id FROM hashes h
        INNER JOIN files f ON h.file_id = f.id
        INNER JOIN versions v ON v.id = f.version_id
        WHERE h.algorithm = $2 AND h.hash = $1
        ",
        hash.as_bytes(),
        algorithm.algorithm
    )
        .fetch_optional(&**pool)
        .await
        ?;

    if let Some(row) = result {
        if !user.role.is_mod() {
            let team_member = database::models::TeamMember::get_from_user_id_version(
                database::models::ids::VersionId(row.version_id),
                user.id.into(),
                &**pool,
            )
            .await
            .map_err(ApiError::DatabaseError)?
            .ok_or_else(|| {
                ApiError::CustomAuthenticationError(
                    "You don't have permission to delete this file!".to_string(),
                )
            })?;

            if !team_member
                .permissions
                .contains(Permissions::DELETE_VERSION)
            {
                return Err(ApiError::CustomAuthenticationError(
                    "You don't have permission to delete this file!".to_string(),
                ));
            }
        }

        let mut transaction = pool.begin().await?;

        sqlx::query!(
            "
            DELETE FROM hashes
            WHERE file_id = $1
            ",
            row.id
        )
        .execute(&mut *transaction)
        .await?;

        sqlx::query!(
            "
            DELETE FROM files
            WHERE files.id = $1
            ",
            row.id,
        )
        .execute(&mut *transaction)
        .await?;

        let project_id: models::projects::ProjectId =
            database::models::ids::ProjectId(row.project_id).into();
        file_host
            .delete_file_version(
                "",
                &format!(
                    "data/{}/versions/{}/{}",
                    project_id, row.version_number, row.filename
                ),
            )
            .await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
