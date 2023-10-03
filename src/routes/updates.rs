use std::collections::HashMap;

use actix_web::{get, web, HttpRequest, HttpResponse};
use atom_syndication::{Category, Content, Entry, Feed, Generator, Link, Person, Text};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::auth::{filter_authorized_versions, get_user_from_headers, is_authorized};
use crate::database;
use crate::models::pats::Scopes;
use crate::models::projects::VersionType;
use crate::models::teams::TeamMember;
use crate::queue::session::AuthQueue;

use super::ApiError;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(forge_updates);
    cfg.service(atom_feed);
}

#[derive(Serialize, Deserialize)]
pub struct NeoForge {
    #[serde(default = "default_neoforge")]
    pub neoforge: String,
}

fn default_neoforge() -> String {
    "none".into()
}

#[get("{id}/forge_updates.json")]
pub async fn forge_updates(
    req: HttpRequest,
    web::Query(neo): web::Query<NeoForge>,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    const ERROR: &str = "The specified project does not exist!";

    let (id,) = info.into_inner();

    let project = database::models::Project::get(&id, &**pool, &redis)
        .await?
        .ok_or_else(|| ApiError::InvalidInput(ERROR.to_string()))?;

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
        return Err(ApiError::InvalidInput(ERROR.to_string()));
    }

    let versions = database::models::Version::get_many(&project.versions, &**pool, &redis).await?;

    let loaders = match &*neo.neoforge {
        "only" => |x: &String| *x == "neoforge",
        "include" => |x: &String| *x == "forge" || *x == "neoforge",
        _ => |x: &String| *x == "forge",
    };

    let mut versions = filter_authorized_versions(
        versions
            .into_iter()
            .filter(|x| x.loaders.iter().any(loaders))
            .collect(),
        &user_option,
        &pool,
    )
    .await?;

    versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));

    #[derive(Serialize)]
    struct ForgeUpdates {
        homepage: String,
        promos: HashMap<String, String>,
    }

    let mut response = ForgeUpdates {
        homepage: format!(
            "{}/mod/{}",
            dotenvy::var("SITE_URL").unwrap_or_default(),
            id
        ),
        promos: HashMap::new(),
    };

    for version in versions {
        if version.version_type == VersionType::Release {
            for game_version in &version.game_versions {
                response
                    .promos
                    .entry(format!("{}-recommended", game_version.0))
                    .or_insert_with(|| version.version_number.clone());
            }
        }

        for game_version in &version.game_versions {
            response
                .promos
                .entry(format!("{}-latest", game_version.0))
                .or_insert_with(|| version.version_number.clone());
        }
    }

    Ok(HttpResponse::Ok().json(response))
}

#[get("{id}/feed.atom")]
pub async fn atom_feed(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (id,) = info.into_inner();

    let Some(project) = database::models::Project::get(&id, &**pool, &redis).await? else {
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

    let versions = database::models::Version::get_many(&project.versions, &**pool, &redis).await?;

    let mut versions = filter_authorized_versions(versions, &user_option, &pool).await?;

    versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));

    let members_data = {
        let members_data = database::models::TeamMember::get_from_team_full(
            project.inner.team_id,
            &**pool,
            &redis,
        )
        .await?;

        let users = crate::database::models::User::get_many_ids(
            &members_data.iter().map(|x| x.user_id).collect::<Vec<_>>(),
            &**pool,
            &redis,
        )
        .await?;

        members_data
            .into_iter()
            .flat_map(|x| {
                users
                    .iter()
                    .find(|y| y.id == x.user_id)
                    .map(|y| TeamMember::from(x, y.clone(), true))
            })
            .collect::<Vec<TeamMember>>()
    };

    fn team_member_to_person(member: &TeamMember) -> Person {
        Person {
            name: member
                .user
                .name
                .as_ref()
                .unwrap_or(&member.user.username)
                .clone(),
            email: None,
            uri: Some(format!(
                "{}/user/{}",
                dotenvy::var("SITE_URL").unwrap_or_default(),
                member.user.username
            )),
        }
    }

    fn tag_to_category(tag: &str) -> Category {
        Category {
            term: tag.to_string(),
            scheme: None,
            label: Some(tag.to_string()),
        }
    }

    let project_id = crate::models::ids::ProjectId::from(project.inner.id);
    let project_link = format!(
        "{}/{}/{}",
        dotenvy::var("SITE_URL").unwrap_or_default(),
        project.project_type,
        project_id
    );

    let feed = Feed {
        title: Text::plain(project.inner.title.clone()),
        id: project_link.clone(),
        updated: project.inner.updated.into(),
        authors: members_data
            .iter()
            .filter(|x| x.role == crate::models::teams::OWNER_ROLE)
            .map(team_member_to_person)
            .collect::<Vec<_>>(),
        categories: project
            .categories
            .iter()
            .chain(project.additional_categories.iter())
            .map(|x| x.as_str())
            .map(tag_to_category)
            .collect::<Vec<_>>(),
        contributors: members_data
            .iter()
            .filter(|x| x.role != crate::models::teams::OWNER_ROLE)
            .map(team_member_to_person)
            .collect::<Vec<_>>(),
        generator: Some(Generator {
            value: "labrinth".to_string(),
            uri: Some("https://github.com/modrinth/labrinth".to_string()),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
        icon: project.inner.icon_url.clone(),
        links: vec![
            Link {
                href: project_link,
                rel: "alternate".to_string(),
                hreflang: None,
                mime_type: Some("text/html".to_string()),
                title: None,
                length: None,
            },
            Link {
                href: format!(
                    "{}/updates/{}/feed.atom",
                    dotenvy::var("SELF_ADDR").unwrap_or_default(),
                    project_id
                ),
                rel: "self".to_string(),
                hreflang: None,
                mime_type: Some("application/atom+xml".to_string()),
                title: None,
                length: None,
            },
        ],
        logo: None,
        rights: None,
        subtitle: Some(Text::plain(project.inner.description.clone())),
        entries: versions
            .iter()
            .map(|v| {
                let link = format!(
                    "{}/{}/{}/version/{}",
                    dotenvy::var("SITE_URL").unwrap_or_default(),
                    project.project_type,
                    project_id,
                    v.id
                );

                Entry {
                    title: Text::plain(v.name.clone()),
                    id: link.clone(),
                    updated: v.date_published.into(),
                    authors: members_data
                        .iter()
                        .find(|x| x.user.id == v.author_id)
                        .map(team_member_to_person)
                        .into_iter()
                        .collect::<Vec<_>>(),
                    categories: v
                        .loaders
                        .iter()
                        .map(|x| x.0.as_str())
                        .chain(v.game_versions.iter().map(|x| x.0.as_str()))
                        .map(tag_to_category)
                        .collect::<Vec<_>>(),
                    contributors: vec![],
                    links: vec![Link {
                        href: link.clone(),
                        rel: "alternate".to_string(),
                        hreflang: None,
                        mime_type: Some("text/html".to_string()),
                        title: None,
                        length: None,
                    }],
                    published: Some(v.date_published.into()),
                    rights: None,
                    source: None,
                    summary: None,
                    content: Some(Content {
                        base: None,
                        lang: Some("en-us".to_string()),
                        value: None,
                        src: Some(link),
                        content_type: Some("text/html".to_string()),
                    }),
                    extensions: Default::default(),
                }
            })
            .collect::<Vec<_>>(),
        extensions: Default::default(),
        namespaces: Default::default(),
        base: None,
        lang: Some("en-US".to_string()),
    };

    Ok(HttpResponse::Ok()
        .content_type("application/atom+xml")
        .body(feed.to_string()))
}
