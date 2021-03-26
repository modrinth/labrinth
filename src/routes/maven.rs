use crate::auth::get_user_from_headers;
use crate::database;
use crate::models::mods::ModId;
use crate::routes::ApiError;
use actix_web::{get, web, HttpRequest, HttpResponse};
use sqlx::PgPool;
use yaserde_derive::YaSerialize;

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
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;
    let id_option: Option<ModId> = serde_json::from_str(&*format!("\"{}\"", string)).ok();

    let mut mod_data;

    if let Some(id) = id_option {
        mod_data = database::models::Mod::get_full(id.into(), &**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

        if mod_data.is_none() {
            mod_data = database::models::Mod::get_full_from_slug(&string, &**pool)
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;
        }
    } else {
        mod_data = database::models::Mod::get_full_from_slug(&string, &**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;
    }

    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();

    if let Some(data) = mod_data {
        let mut authorized = !data.status.is_hidden();

        if let Some(user) = user_option {
            if !authorized {
                if user.role.is_mod() {
                    authorized = true;
                } else {
                    let user_id: database::models::ids::UserId = user.id.into();

                    let mod_exists = sqlx::query!(
                        "SELECT EXISTS(SELECT 1 FROM team_members WHERE team_id = $1 AND user_id = $2)",
                        data.inner.team_id as database::models::ids::TeamId,
                        user_id as database::models::ids::UserId,
                    )
                    .fetch_one(&**pool)
                    .await
                    .map_err(|e| ApiError::DatabaseError(e.into()))?
                    .exists;

                    authorized = mod_exists.unwrap_or(false);
                }
            }
        }

        if authorized {
            let version_names = sqlx::query!(
                "
                SELECT version_number, release_channels.channel channel
                FROM versions
                LEFT JOIN release_channels ON release_channels.id = versions.release_channel
                WHERE mod_id = $1
                ",
                data.inner.id as database::models::ids::ModId
            )
            .fetch_all(&**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

            // let mut response = String::new();

            // response.push_str("<metadata>\n");
            // response.push_str(" <groupId>maven.modrinth</groupId>\n");
            // response.push_str(" <artifactId>");
            // response.push_str(&string);
            // response.push_str("</artifactId>\n");
            // response.push_str(" <versioning>\n");
            // response.push_str("  <latest>");
            // response.push_str();
            // response.push_str("</latest>");
            // response.push_str("  <release>");
            // response.push_str();
            // response.push_str("</release>");
            // response.push_str("  <versions>\n");

            // for name in version_names {
            //     response.push_str("   <version>");
            //     response.push_str(&name.version_number);
            //     response.push_str("</version>\n");
            // }
            // response.push_str("  </versions>\n");
            // response.push_str("  <lastUpdated>");
            // response.push_str(&data.inner.updated.format("%Y%m%d%H%M%S").to_string());
            // response.push_str("</lastUpdated>");
            // response.push_str(" </versioning>\n");
            // response.push_str("</metadata>");

            let respdata = Metadata {
                group_id: "maven.modrinth".to_string(),
                artifact_id: string,
                versioning: Versioning {
                    latest: version_names
                        .last()
                        .map_or("release", |x| &x.version_number)
                        .to_string(),
                    release: version_names
                        .iter()
                        .rfind(|x| x.channel == "release")
                        .map_or("", |x| &x.version_number)
                        .to_string(),
                    versions: Versions {
                        versions: version_names
                            .iter()
                            .map(|x| x.version_number.clone())
                            .collect::<Vec<_>>(),
                    },
                    last_updated: data.inner.updated.format("%Y%m%d%H%M%S").to_string(),
                },
            };

            // return Ok(HttpResponse::Ok().content_type("text/xml").body(response));
            Ok(HttpResponse::Ok()
                .content_type("text/xml")
                .body(yaserde::ser::to_string(&respdata).map_err(|e| ApiError::XmlError(e))?))
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[get("maven/modrinth/{id}/{versionnum}/{file}.pom")]
pub async fn version_pom(
    req: HttpRequest,
    web::Path((string, vnum, file)): web::Path<(String, String, String)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    if format!("{}-{}", string, vnum) != file {
        return Ok(HttpResponse::NotFound().body(""));
    }

    let id_option: Option<ModId> = serde_json::from_str(&*format!("\"{}\"", string)).ok();

    let mut mod_data;

    if let Some(id) = id_option {
        mod_data = database::models::Mod::get_full(id.into(), &**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

        if mod_data.is_none() {
            mod_data = database::models::Mod::get_full_from_slug(&string, &**pool)
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;
        }
    } else {
        mod_data = database::models::Mod::get_full_from_slug(&string, &**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;
    }

    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();

    if let Some(data) = mod_data {
        let mut authorized = !data.status.is_hidden();

        if let Some(user) = user_option {
            if !authorized {
                if user.role.is_mod() {
                    authorized = true;
                } else {
                    let user_id: database::models::ids::UserId = user.id.into();

                    let mod_exists = sqlx::query!(
                        "SELECT EXISTS(SELECT 1 FROM team_members WHERE team_id = $1 AND user_id = $2)",
                        data.inner.team_id as database::models::ids::TeamId,
                        user_id as database::models::ids::UserId,
                    )
                    .fetch_one(&**pool)
                    .await
                    .map_err(|e| ApiError::DatabaseError(e.into()))?
                    .exists;

                    authorized = mod_exists.unwrap_or(false);
                }
            }
        }

        if authorized {
            let vid_option = sqlx::query!(
                "SELECT id FROM versions WHERE mod_id = $1 AND version_number = $2",
                data.inner.id as database::models::ids::ModId,
                vnum
            )
            .fetch_optional(&**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

            if let Some(vid) = vid_option {
                let version_option = database::models::Version::get(
                    database::models::ids::VersionId(vid.id),
                    &**pool,
                )
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;

                if let Some(version) = version_option {
                    let respdata = MavenPom {
                        schema_location: "http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd".to_string(),
                        xsi: "http://www.w3.org/2001/XMLSchema-instance".to_string(),
                        model_version: "4.0.0".to_string(),
                        group_id: "maven.modrinth".to_string(),
                        artifact_id: string,
                        version: version.version_number,
                        name: data.inner.title,
                        description: data.inner.description,
                    };
                    Ok(HttpResponse::Ok().content_type("text/xml").body(
                        yaserde::ser::to_string(&respdata).map_err(|e| ApiError::XmlError(e))?,
                    ))
                } else {
                    Ok(HttpResponse::NotFound().body(""))
                }
            } else {
                Ok(HttpResponse::NotFound().body(""))
            }
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[get("maven/modrinth/{id}/{versionnum}/{file}.jar")]
pub async fn version_jar(
    req: HttpRequest,
    web::Path((string, vnum, file)): web::Path<(String, String, String)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id_option: Option<ModId> = serde_json::from_str(&*format!("\"{}\"", string)).ok();

    let mut mod_data;

    if let Some(id) = id_option {
        mod_data = database::models::Mod::get_full(id.into(), &**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

        if mod_data.is_none() {
            mod_data = database::models::Mod::get_full_from_slug(&string, &**pool)
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;
        }
    } else {
        mod_data = database::models::Mod::get_full_from_slug(&string, &**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;
    }

    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();

    if let Some(data) = mod_data {
        let mut authorized = !data.status.is_hidden();

        if let Some(user) = user_option {
            if !authorized {
                if user.role.is_mod() {
                    authorized = true;
                } else {
                    let user_id: database::models::ids::UserId = user.id.into();

                    let mod_exists = sqlx::query!(
                        "SELECT EXISTS(SELECT 1 FROM team_members WHERE team_id = $1 AND user_id = $2)",
                        data.inner.team_id as database::models::ids::TeamId,
                        user_id as database::models::ids::UserId,
                    )
                    .fetch_one(&**pool)
                    .await
                    .map_err(|e| ApiError::DatabaseError(e.into()))?
                    .exists;

                    authorized = mod_exists.unwrap_or(false);
                }
            }
        }

        if authorized {
            let vid_option = sqlx::query!(
                "SELECT id FROM versions WHERE mod_id = $1 AND version_number = $2",
                data.inner.id as database::models::ids::ModId,
                vnum
            )
            .fetch_optional(&**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

            if let Some(vid) = vid_option {
                let version_option = database::models::Version::get_full(
                    database::models::ids::VersionId(vid.id),
                    &**pool,
                )
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;

                if let Some(version) = version_option {
                    if version.files.len() > 0 {
                        let full_url_file = file + ".jar";
                        let file = if let Some(selected_file) =
                            version.files.iter().find(|x| x.filename == full_url_file)
                        {
                            selected_file
                        } else {
                            version.files.last().unwrap()
                        };
                        Ok(HttpResponse::TemporaryRedirect()
                            .header("Location", &*file.url)
                            .body(""))
                    } else {
                        Ok(HttpResponse::NotFound().body(""))
                    }
                } else {
                    Ok(HttpResponse::NotFound().body(""))
                }
            } else {
                Ok(HttpResponse::NotFound().body(""))
            }
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
