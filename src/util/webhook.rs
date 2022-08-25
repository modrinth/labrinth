use crate::models::projects::Project;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize)]
pub struct DiscordEmbed {
    pub author: Option<DiscordEmbedAuthor>,
    pub title: String,
    pub description: String,
    pub url: String,
    pub timestamp: DateTime<Utc>,
    pub color: u32,
    pub fields: Vec<DiscordEmbedField>,
    pub thumbnail: DiscordEmbedThumbnail,
}

#[derive(Serialize)]
pub struct DiscordEmbedField {
    pub name: &'static str,
    pub value: String,
    pub inline: bool,
}

#[derive(Serialize)]
pub struct DiscordEmbedThumbnail {
    pub url: Option<String>,
}

#[derive(Serialize)]
pub struct DiscordEmbedAuthor {
    pub name: String,
}

#[derive(Serialize)]
pub struct DiscordWebhook {
    pub embeds: Vec<DiscordEmbed>,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
}

pub async fn send_generic_webhook(
    webhook: &DiscordWebhook,
    webhook_url: String,
) -> Result<(), reqwest::Error> {
    let client = reqwest::Client::new();

    client.post(webhook_url).json(&webhook).send().await?;

    Ok(())
}

pub async fn send_discord_moderation_webhook(
    project: Project,
    webhook_url: String,
) -> Result<(), reqwest::Error> {
    let mut fields = vec![
        DiscordEmbedField {
            name: "id",
            value: project.id.to_string(),
            inline: true,
        },
        DiscordEmbedField {
            name: "project_type",
            value: project.project_type.clone(),
            inline: true,
        },
        DiscordEmbedField {
            name: "client_side",
            value: project.client_side.to_string(),
            inline: true,
        },
        DiscordEmbedField {
            name: "server_side",
            value: project.server_side.to_string(),
            inline: true,
        },
    ];

    if !project.categories.is_empty() {
        fields.push(DiscordEmbedField {
            name: "categories",
            value: project.categories.join(", "),
            inline: true,
        });
    }

    if let Some(ref slug) = project.slug {
        fields.push(DiscordEmbedField {
            name: "slug",
            value: slug.clone(),
            inline: true,
        });
    }

    let embed = DiscordEmbed {
        author: None,
        url: format!(
            "{}/{}/{}",
            dotenv::var("SITE_URL").unwrap_or_default(),
            project.project_type,
            project
                .clone()
                .slug
                .unwrap_or_else(|| project.id.to_string())
        ),
        title: project.title,
        description: project.description,
        timestamp: project.published,
        color: 0x1bd96a,
        fields,
        thumbnail: DiscordEmbedThumbnail {
            url: project.icon_url,
        },
    };

    send_generic_webhook(
        &DiscordWebhook {
            embeds: vec![embed],
            username: None,
            avatar_url: None,
        },
        webhook_url,
    )
    .await
}
