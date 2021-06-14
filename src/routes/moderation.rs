use super::ApiError;
use crate::auth::check_is_moderator_from_headers;
use crate::database;
use crate::models::projects::{Project, ProjectStatus};
use actix_web::{get, web, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct ResultCount {
    #[serde(default = "default_count")]
    pub count: i16,
}

fn default_count() -> i16 {
    100
}

#[get("projects")]
pub async fn get_projects(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    count: web::Query<ResultCount>,
) -> Result<HttpResponse, ApiError> {
    check_is_moderator_from_headers(req.headers(), &**pool).await?;

    use futures::stream::TryStreamExt;

    let project_ids = sqlx::query!(
        "
        SELECT id FROM mods
        WHERE status = (
            SELECT id FROM statuses WHERE status = $1
        )
        ORDER BY updated ASC
        LIMIT $2;
        ",
        ProjectStatus::Processing.as_str(),
        count.count as i64
    )
    .fetch_many(&**pool)
    .try_filter_map(|e| async { Ok(e.right().map(|m| database::models::ProjectId(m.id))) })
    .try_collect::<Vec<database::models::ProjectId>>()
    .await?;

    let projects: Vec<Project> = database::Project::get_many_full(project_ids, &**pool)
        .await?
        .into_iter()
        .map(super::projects::convert_project)
        .collect();

    Ok(HttpResponse::Ok().json(projects))
}

#[derive(Serialize)]
pub struct DiscordEmbed {
    pub title: String,
    pub description: String,
    pub url: String,
    pub timestamp: DateTime<Utc>,
    pub color: u32,
    pub fields: Vec<DiscordEmbedField>,
    pub image: DiscordEmbedImage,
}

#[derive(Serialize)]
pub struct DiscordEmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

#[derive(Serialize)]
pub struct DiscordEmbedImage {
    pub url: Option<String>,
}

#[derive(Serialize)]
pub struct DiscordWebhook {
    pub embeds: Vec<DiscordEmbed>,
}

pub async fn send_discord_webhook(project: Project) -> Result<(), reqwest::Error> {
    if let Some(webhook_url) = dotenv::var("MODERATION_DISCORD_WEBHOOK").ok() {
        let mut fields = Vec::new();

        fields.push(DiscordEmbedField {
            name: "id".to_string(),
            value: project.id.to_string(),
            inline: true,
        });

        if let Some(slug) = project.slug.clone() {
            fields.push(DiscordEmbedField {
                name: "slug".to_string(),
                value: slug,
                inline: true,
            });
        }

        fields.push(DiscordEmbedField {
            name: "project_type".to_string(),
            value: project.project_type.to_string(),
            inline: true,
        });

        fields.push(DiscordEmbedField {
            name: "client_side".to_string(),
            value: project.client_side.to_string(),
            inline: true,
        });

        fields.push(DiscordEmbedField {
            name: "server_side".to_string(),
            value: project.server_side.to_string(),
            inline: true,
        });

        fields.push(DiscordEmbedField {
            name: "categories".to_string(),
            value: project.categories.join(", "),
            inline: true,
        });

        let embed = DiscordEmbed {
            title: project.title,
            description: project.description,
            url: format!(
                "{}/mod/{}",
                dotenv::var("SITE_URL").unwrap_or_default(),
                project.slug.unwrap_or(project.id.to_string())
            ),
            timestamp: project.published,
            color: 6137157,
            fields,
            image: DiscordEmbedImage {
                url: project.icon_url,
            },
        };

        let client = reqwest::Client::new();

        client
            .post(&webhook_url)
            .json(&DiscordWebhook {
                embeds: vec![embed],
            })
            .send()
            .await?;
    }

    Ok(())
}
