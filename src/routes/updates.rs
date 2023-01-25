use std::collections::HashMap;

use actix_web::{get, web, HttpRequest, HttpResponse};
use atom_syndication::{Feed, Text, Person, Category, Generator, Link, Entry, Content};
use serde::Serialize;
use sqlx::PgPool;

use crate::database;
use crate::database::models::TeamMember;
use crate::database::models::team_item::QueryTeamMember;
use crate::models::projects::{Version, VersionType};
use crate::util::auth::{
    get_user_from_headers, is_authorized, is_authorized_version,
};
use futures::StreamExt;

use super::ApiError;

#[get("{id}/forge_updates.json")]
pub async fn forge_updates(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    const ERROR: &str = "The specified project does not exist!";

    let (id,) = info.into_inner();

    let project =
        database::models::Project::get_from_slug_or_project_id(&id, &**pool)
            .await?
            .ok_or_else(|| ApiError::InvalidInput(ERROR.to_string()))?;

    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();

    if !is_authorized(&project, &user_option, &pool).await? {
        return Err(ApiError::InvalidInput(ERROR.to_string()));
    }

    let version_ids = database::models::Version::get_project_versions(
        project.id,
        None,
        Some(vec!["forge".to_string()]),
        &**pool,
    )
    .await?;

    let versions =
        database::models::Version::get_many_full(version_ids, &**pool).await?;

    let mut versions = futures::stream::iter(versions)
        .filter_map(|data| async {
            if is_authorized_version(&data.inner, &user_option, &pool)
                .await
                .ok()?
            {
                Some(data)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .await;

    versions
        .sort_by(|a, b| b.inner.date_published.cmp(&a.inner.date_published));

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
        let version = Version::from(version);

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
) -> Result<HttpResponse, ApiError> {
    let (id,) = info.into_inner();

    let project =
        if let Some(proj) = database::models::Project::get_full_from_slug_or_project_id(&id, &**pool)
            .await? {
                proj
            } else {
                return Ok(HttpResponse::NotFound().body(""));
            };

    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();

    if !is_authorized(&project.inner, &user_option, &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    // I would really like to get rid of this .clone() and making the method take a reference, but that
    // necessitates a lot of refactoring.
    let versions =
        database::models::Version::get_many_full(project.versions.clone(), &**pool).await?;

    let mut versions = futures::stream::iter(versions)
        .filter_map(|data| async {
            if is_authorized_version(&data.inner, &user_option, &pool)
                .await
                .ok()?
            {
                Some(data)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .await;

    versions
        .sort_by(|a, b| b.inner.date_published.cmp(&a.inner.date_published));

    let members_data =
        TeamMember::get_from_team_full(project.inner.team_id, &**pool).await?;

    fn team_member_to_person(member: &QueryTeamMember) -> Person {
        Person {
            name: member.user.name.as_ref().unwrap_or(&member.user.username).clone(),
            email: None,
            uri: Some(format!("{}/user/{}", dotenvy::var("SITE_URL").unwrap_or_default(), member.user.username))
        }
    }

    fn tag_to_category(tag: &String) -> Category {
        Category {
            term: tag.clone(),
            scheme: None,
            label: Some(tag.clone())
        }
    }

    let project_id = crate::models::ids::ProjectId::from(project.inner.id);

    let feed = Feed {
        title: Text::plain(project.inner.title.clone()),
        id: project_id.to_string(),
        updated: project.inner.updated.into(),
        authors: members_data
            .iter()
            .filter(|x| x.role == crate::models::teams::OWNER_ROLE)
            .map(team_member_to_person)
            .collect::<Vec<_>>(),
        categories: project.categories
            .iter()
            .chain(project.additional_categories.iter())
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
            version: Some(env!("CARGO_PKG_VERSION").to_string())
        }),
        icon: project.inner.icon_url.clone(),
        links: vec![
            Link {
                href: format!(
                    "{}/{}/{}",
                    dotenvy::var("SITE_URL").unwrap_or_default(),
                    project.project_type,
                    project_id                    
                ),
                rel: "alternate".to_string(),
                hreflang: None,
                mime_type: Some("text/html".to_string()),
                title: None,
                length: None
            }
        ],
        logo: None,
        rights: None,
        subtitle: Some(Text::plain(project.inner.description.clone())),
        entries: versions
            .iter()
            .map(|v| {
                let version_id = crate::models::ids::VersionId::from(v.inner.id);
                let link = format!(
                    "{}/{}/{}/version/{}",
                    dotenvy::var("SITE_URL").unwrap_or_default(), project.project_type, project_id, version_id
                );

                Entry {
                    title: Text::plain(v.inner.name.clone()),
                    id: version_id.to_string(),
                    updated: v.inner.date_published.into(),
                    authors: members_data
                        .iter()
                        .find(|x| x.user.id == v.inner.author_id)
                        .map(team_member_to_person)
                        .into_iter()
                        .collect::<Vec<_>>(),
                    categories: v.loaders
                        .iter()
                        .chain(v.game_versions.iter())
                        .map(tag_to_category)
                        .collect::<Vec<_>>(),
                    contributors: vec![],
                    links: vec![Link {
                        href: link.clone(),
                        rel: "alternate".to_string(),
                        hreflang: None,
                        mime_type: Some("text/html".to_string()),
                        title: None,
                        length: None
                    }],
                    published: Some(v.inner.date_published.into()),
                    rights: None,
                    source: None,
                    summary: None,
                    content: Some(Content {
                        base: None,
                        lang: Some("en_US".to_string()),
                        value: None,
                        src: Some(link),
                        content_type: None
                    }),
                    extensions: Default::default()
                }
            })
            .collect::<Vec<_>>(),
        extensions: Default::default(),
        namespaces: Default::default(),
        base: None,
        lang: Some("en-US".to_string())
    };

    Ok(HttpResponse::Ok()
        .content_type("text/xml")
        .body(feed.to_string()))
}