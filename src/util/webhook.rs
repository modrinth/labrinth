use crate::database::models::legacy_loader_fields::MinecraftGameVersion;
use crate::database::redis::RedisPool;
use crate::models::projects::ProjectId;
use crate::routes::ApiError;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

#[derive(Serialize)]
struct DiscordEmbed {
    pub author: Option<DiscordEmbedAuthor>,
    pub title: String,
    pub description: String,
    pub url: String,
    pub timestamp: DateTime<Utc>,
    pub color: u32,
    pub fields: Vec<DiscordEmbedField>,
    pub thumbnail: DiscordEmbedThumbnail,
    pub image: Option<DiscordEmbedImage>,
    pub footer: Option<DiscordEmbedFooter>,
}

#[derive(Serialize)]
struct DiscordEmbedAuthor {
    pub name: String,
    pub url: Option<String>,
    pub icon_url: Option<String>,
}

#[derive(Serialize)]
struct DiscordEmbedField {
    pub name: &'static str,
    pub value: String,
    pub inline: bool,
}

#[derive(Serialize)]
struct DiscordEmbedImage {
    pub url: Option<String>,
}

#[derive(Serialize)]
struct DiscordEmbedThumbnail {
    pub url: Option<String>,
}

#[derive(Serialize)]
struct DiscordEmbedFooter {
    pub text: String,
    pub icon_url: Option<String>,
}

#[derive(Serialize)]
struct DiscordWebhook {
    pub avatar_url: Option<String>,
    pub username: Option<String>,
    pub embeds: Vec<DiscordEmbed>,
    pub content: Option<String>,
}

const PLUGIN_LOADERS: &[&str] = &[
    "bukkit",
    "spigot",
    "paper",
    "purpur",
    "bungeecord",
    "waterfall",
    "velocity",
    "sponge",
];

pub async fn send_discord_webhook(
    project_id: ProjectId,
    pool: &PgPool,
    redis: &RedisPool,
    webhook_url: String,
    message: Option<String>,
) -> Result<(), ApiError> {
    // TODO: this currently uses Minecraft as it is a v2 webhook, and requires 'game_versions', a minecraft-java loader field.
    // TODO: This should be updated to use the generic loader fields w/ discord from the project game

    // TODO: This should use the project_item get route
    let all_game_versions = MinecraftGameVersion::list(None, None, pool, redis).await?;

    let row =
        sqlx::query!(
            "
            SELECT m.id id, m.name name, m.summary summary, m.color color,
            m.icon_url icon_url, m.slug slug,
            u.username username, u.avatar_url avatar_url,
            ARRAY_AGG(DISTINCT c.category) filter (where c.category is not null) categories,
            ARRAY_AGG(DISTINCT lo.loader) filter (where lo.loader is not null) loaders,
            ARRAY_AGG(DISTINCT pt.name) filter (where pt.name is not null) project_types,
            ARRAY_AGG(DISTINCT g.slug) filter (where g.slug is not null) games,
            ARRAY_AGG(DISTINCT mg.image_url) filter (where mg.image_url is not null and mg.featured is false) gallery,
            ARRAY_AGG(DISTINCT mg.image_url) filter (where mg.image_url is not null and mg.featured is true) featured_gallery
            FROM mods m
            LEFT OUTER JOIN mods_categories mc ON joining_mod_id = m.id AND mc.is_additional = FALSE
            LEFT OUTER JOIN categories c ON mc.joining_category_id = c.id
            LEFT OUTER JOIN versions v ON v.mod_id = m.id AND v.status != ALL($2)
            LEFT OUTER JOIN loaders_versions lv ON lv.version_id = v.id
            LEFT OUTER JOIN loaders lo ON lo.id = lv.loader_id
            LEFT JOIN loaders_project_types lpt ON lpt.joining_loader_id = lo.id
            LEFT JOIN project_types pt ON pt.id = lpt.joining_project_type_id
            LEFT JOIN loaders_project_types_games lptg ON lptg.loader_id = lo.id AND lptg.project_type_id = pt.id
            LEFT JOIN games g ON lptg.game_id = g.id
            LEFT OUTER JOIN mods_gallery mg ON mg.mod_id = m.id
            INNER JOIN team_members tm ON tm.team_id = m.team_id AND tm.is_owner = TRUE AND tm.accepted = TRUE
            INNER JOIN users u ON tm.user_id = u.id
            WHERE m.id = $1
            GROUP BY m.id, u.id;
            ",
            project_id.0 as i64,
            &*crate::models::projects::VersionStatus::iterator().filter(|x| x.is_hidden()).map(|x| x.to_string()).collect::<Vec<String>>(),
        )
        .fetch_optional(pool)
        .await?;

    if let Some(project) = row {
        let mut fields = vec![];

        let categories = project.categories.unwrap_or_default();
        let loaders = project.loaders.unwrap_or_default();

        if !categories.is_empty() {
            fields.push(DiscordEmbedField {
                name: "Categories",
                value: categories
                    .into_iter()
                    .map(|mut x| format!("{}{x}", x.remove(0).to_uppercase()))
                    .collect::<Vec<_>>()
                    .join("\n"),
                inline: true,
            });
        }

        if !loaders.is_empty() {
            let mut formatted_loaders: String = String::new();

            for loader in &loaders {
                let emoji_id: i64 = match &**loader {
                    "bukkit" => 1049793345481883689,
                    "bungeecord" => 1049793347067314220,
                    "canvas" => 1107352170656968795,
                    "datapack" => 1057895494652788866,
                    "fabric" => 1049793348719890532,
                    "folia" => 1107348745571537018,
                    "forge" => 1049793350498275358,
                    "iris" => 1107352171743281173,
                    "liteloader" => 1049793351630733333,
                    "minecraft" => 1049793352964526100,
                    "modloader" => 1049793353962762382,
                    "neoforge" => 1140437823783190679,
                    "optifine" => 1107352174415052901,
                    "paper" => 1049793355598540810,
                    "purpur" => 1140436034505674762,
                    "quilt" => 1049793857681887342,
                    "rift" => 1049793359373414502,
                    "spigot" => 1049793413886779413,
                    "sponge" => 1049793416969605231,
                    "vanilla" => 1107350794178678855,
                    "velocity" => 1049793419108700170,
                    "waterfall" => 1049793420937412638,
                    _ => 1049805243866681424,
                };

                let mut x = if loader == "datapack" {
                    "Data Pack"
                } else {
                    loader
                }
                .to_string();

                formatted_loaders.push_str(&format!(
                    "<:{loader}:{emoji_id}> {}{x}\n",
                    x.remove(0).to_uppercase()
                ));
            }

            fields.push(DiscordEmbedField {
                name: "Loaders",
                value: formatted_loaders,
                inline: true,
            });
        }

        // TODO: Modified to keep "Versions" as a field as it may be hardcoded. Ideally, this pushes all loader fields to the embed for v3
        // TODO: This might need some work to manually test
        let version_fields = crate::database::models::project_item::Project::get_id(
            crate::database::models::ids::ProjectId(project.id),
            pool,
            redis,
        )
        .await
        .ok()
        .flatten()
        .map(|project| project.aggregate_version_fields)
        .unwrap_or_default();

        let versions = version_fields
            .into_iter()
            .find_map(|vf| MinecraftGameVersion::try_from_version_field(&vf).ok())
            .unwrap_or_default();

        if !versions.is_empty() {
            let formatted_game_versions: String = get_gv_range(versions, all_game_versions);
            fields.push(DiscordEmbedField {
                name: "Versions",
                value: formatted_game_versions,
                inline: true,
            });
        }

        let mut project_types: Vec<String> = project.project_types.unwrap_or_default();
        let mut project_type = project_types.pop().unwrap_or_default(); // TODO: Should this grab a not-first?

        if loaders.iter().all(|x| PLUGIN_LOADERS.contains(&&**x)) {
            project_type = "plugin".to_string();
        } else if loaders.iter().any(|x| x == "datapack") {
            project_type = "datapack".to_string();
        }

        let mut display_project_type = match &*project_type {
            "datapack" => "data pack",
            "resourcepack" => "resource pack",
            _ => &*project_type,
        }
        .to_string();

        let embed = DiscordEmbed {
            author: Some(DiscordEmbedAuthor {
                name: project.username.clone(),
                url: Some(format!(
                    "{}/user/{}",
                    dotenvy::var("SITE_URL").unwrap_or_default(),
                    project.username
                )),
                icon_url: project.avatar_url,
            }),
            url: format!(
                "{}/{}/{}",
                dotenvy::var("SITE_URL").unwrap_or_default(),
                project_type,
                project.slug.unwrap_or_else(|| project_id.to_string())
            ),
            title: project.name, // Do not change DiscordEmbed
            description: project.summary,
            timestamp: Utc::now(),
            color: project.color.unwrap_or(0x1bd96a) as u32,
            fields,
            thumbnail: DiscordEmbedThumbnail {
                url: project.icon_url,
            },
            image: if let Some(first) = project.featured_gallery.unwrap_or_default().first() {
                Some(first.clone())
            } else {
                project.gallery.unwrap_or_default().first().cloned()
            }
            .map(|x| DiscordEmbedImage { url: Some(x) }),
            footer: Some(DiscordEmbedFooter {
                text: format!(
                    "{}{display_project_type} on Modrinth",
                    display_project_type.remove(0).to_uppercase()
                ),
                icon_url: Some("https://cdn-raw.modrinth.com/modrinth-new.png".to_string()),
            }),
        };

        let client = reqwest::Client::new();

        client
            .post(&webhook_url)
            .json(&DiscordWebhook {
                avatar_url: Some("https://cdn.modrinth.com/Modrinth_Dark_Logo.png".to_string()),
                username: Some("Modrinth Release".to_string()),
                embeds: vec![embed],
                content: message,
            })
            .send()
            .await
            .map_err(|_| ApiError::Discord("Error while sending projects webhook".to_string()))?;
    }

    Ok(())
}

fn get_gv_range(
    mut game_versions: Vec<MinecraftGameVersion>,
    mut all_game_versions: Vec<MinecraftGameVersion>,
) -> String {
    // both -> least to greatest
    game_versions.sort_by(|a, b| a.created.cmp(&b.created));
    game_versions.dedup_by(|a, b| a.version == b.version);

    all_game_versions.sort_by(|a, b| a.created.cmp(&b.created));

    let all_releases = all_game_versions
        .iter()
        .filter(|x| &*x.type_ == "release")
        .cloned()
        .collect::<Vec<_>>();

    let mut intervals = Vec::new();
    let mut current_interval = 0;

    const MAX_VALUE: usize = 1000000;

    for (i, current_version) in game_versions.iter().enumerate() {
        let current_version = &current_version.version;

        let index = all_game_versions
            .iter()
            .position(|x| &*x.version == current_version)
            .unwrap_or(MAX_VALUE);
        let release_index = all_releases
            .iter()
            .position(|x| &*x.version == current_version)
            .unwrap_or(MAX_VALUE);

        if i == 0 {
            intervals.push(vec![vec![i, index, release_index]])
        } else {
            let interval_base = &intervals[current_interval];

            if ((index as i32) - (interval_base[interval_base.len() - 1][1] as i32) == 1
                || (release_index as i32) - (interval_base[interval_base.len() - 1][2] as i32) == 1)
                && (all_game_versions[interval_base[0][1]].type_ == "release"
                    || all_game_versions[index].type_ != "release")
            {
                if intervals[current_interval].get(1).is_some() {
                    intervals[current_interval][1] = vec![i, index, release_index];
                } else {
                    intervals[current_interval].insert(1, vec![i, index, release_index]);
                }
            } else {
                current_interval += 1;
                intervals.push(vec![vec![i, index, release_index]]);
            }
        }
    }

    let mut new_intervals = Vec::new();

    for interval in intervals {
        if interval.len() == 2 && interval[0][2] != MAX_VALUE && interval[1][2] == MAX_VALUE {
            let mut last_snapshot: Option<usize> = None;

            for j in ((interval[0][1] + 1)..=interval[1][1]).rev() {
                if all_game_versions[j].type_ == "release" {
                    new_intervals.push(vec![
                        interval[0].clone(),
                        vec![
                            game_versions
                                .iter()
                                .position(|x| x.version == all_game_versions[j].version)
                                .unwrap_or(MAX_VALUE),
                            j,
                            all_releases
                                .iter()
                                .position(|x| x.version == all_game_versions[j].version)
                                .unwrap_or(MAX_VALUE),
                        ],
                    ]);

                    if let Some(last_snapshot) = last_snapshot {
                        if last_snapshot != j + 1 {
                            new_intervals.push(vec![
                                vec![
                                    game_versions
                                        .iter()
                                        .position(|x| {
                                            x.version == all_game_versions[last_snapshot].version
                                        })
                                        .unwrap_or(MAX_VALUE),
                                    last_snapshot,
                                    MAX_VALUE,
                                ],
                                interval[1].clone(),
                            ])
                        }
                    } else {
                        new_intervals.push(vec![interval[1].clone()])
                    }

                    break;
                } else {
                    last_snapshot = Some(j);
                }
            }
        } else {
            new_intervals.push(interval);
        }
    }

    let mut output = Vec::new();

    for interval in new_intervals {
        if interval.len() == 2 {
            output.push(format!(
                "{}—{}",
                &game_versions[interval[0][0]].version, &game_versions[interval[1][0]].version
            ))
        } else {
            output.push(game_versions[interval[0][0]].version.clone())
        }
    }

    output.join("\n")
}
