use crate::auth::checks::filter_visible_versions;
use crate::database;
use crate::database::models::notification_item::NotificationBuilder;
use crate::database::models::thread_item::ThreadMessageBuilder;
use crate::database::redis::RedisPool;
use crate::models::ids::ProjectId;
use crate::models::notifications::NotificationBody;
use crate::models::pack::{PackFile, PackFileHash, PackFormat};
use crate::models::projects::ProjectStatus;
use crate::models::threads::MessageBody;
use crate::routes::ApiError;
use dashmap::DashSet;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::time::Duration;
use zip::ZipArchive;

const AUTOMOD_ID: i64 = 0;

pub struct ModerationMessages {
    pub messages: Vec<ModerationMessage>,
    pub version_specific: HashMap<String, Vec<ModerationMessage>>,
}

impl ModerationMessages {
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.version_specific.is_empty()
    }

    pub fn markdown(&self) -> String {
        let mut str = "".to_string();

        for message in &self.messages {
            str.push_str(&format!("## {}\n", message.header()));
            str.push_str(&format!("{}\n", message.body()));
            str.push('\n');
        }

        for (version_num, messages) in &self.version_specific {
            for message in messages {
                str.push_str(&format!(
                    "## Version {}: {}\n",
                    version_num,
                    message.header()
                ));
                str.push_str(&format!("{}\n", message.body()));
                str.push('\n');
            }
        }

        str.push_str("<hr />\n\n");
        str.push_str("🤖 This is an automated message generated by AutoMod. If you are facing issues, please [contact support](https://support.modrinth.com).");

        str
    }

    pub fn should_reject(&self, first_time: bool) -> bool {
        self.messages.iter().any(|x| x.rejectable(first_time))
            || self
                .version_specific
                .values()
                .any(|x| x.iter().any(|x| x.rejectable(first_time)))
    }
}

pub enum ModerationMessage {
    NoPrimaryFile,
    PackFilesNotAllowed {
        files: HashMap<String, ApprovalType>,
        incomplete: bool,
    },
}

impl ModerationMessage {
    pub fn rejectable(&self, first_time: bool) -> bool {
        match self {
            ModerationMessage::NoPrimaryFile => true,
            ModerationMessage::PackFilesNotAllowed { files, incomplete } => {
                (!incomplete || first_time)
                    && files.values().any(|x| match x {
                        ApprovalType::Yes => false,
                        ApprovalType::WithAttributionAndSource => false,
                        ApprovalType::WithAttribution => false,
                        ApprovalType::No => first_time,
                        ApprovalType::PermanentNo => true,
                        ApprovalType::Unidentified => first_time,
                    })
            }
        }
    }

    pub fn header(&self) -> &'static str {
        match self {
            ModerationMessage::NoPrimaryFile => "No primary files",
            ModerationMessage::PackFilesNotAllowed { .. } => "Copyrighted Content",
        }
    }

    pub fn body(&self) -> String {
        match self {
            ModerationMessage::NoPrimaryFile => "Please attach a file to this version. All files on Modrinth must have files associated with their versions.\n".to_string(),
            ModerationMessage::PackFilesNotAllowed { files, .. } => {
                let mut str = "".to_string();
                str.push_str("This pack redistributes copyrighted material. Please refer to [Modrinth's guide on obtaining modpack permissions](https://docs.modrinth.com/modpacks/permissions) for more information.\n\n");

                let mut attribute_mods = Vec::new();
                let mut no_mods = Vec::new();
                let mut permanent_no_mods = Vec::new();
                let mut unidentified_mods = Vec::new();
                for (path, approval) in files.iter() {
                    match approval {
                        ApprovalType::Yes | ApprovalType::WithAttributionAndSource  => {}
                        ApprovalType::WithAttribution => attribute_mods.push(path),
                        ApprovalType::No => no_mods.push(path),
                        ApprovalType::PermanentNo => permanent_no_mods.push(path),
                        ApprovalType::Unidentified => unidentified_mods.push(path),
                    }
                }

                fn print_mods(projects: Vec<&String>, headline: &str, val: &mut String) {
                    if projects.is_empty() { return }

                    val.push_str(&format!("{headline}\n\n"));

                    for project in &projects {
                        let additional_text = if project.contains("ftb-quests") {
                            Some("Heracles")
                        } else if project.contains("ftb-ranks") || project.contains("ftb-essentials") {
                            Some("Prometheus")
                        } else if project.contains("ftb-teams") {
                            Some("Argonauts")
                        } else if project.contains("ftb-chunks") {
                            Some("Cadmus")
                        } else {
                            None
                        };

                        val.push_str(&if let Some(additional_text) = additional_text {
                            format!("- {project}(consider using [{additional_text}](https://modrinth.com/mod/{}) instead)\n", additional_text.to_lowercase())
                        } else {
                            format!("- {project}\n")
                        })
                    }

                    if !projects.is_empty() {
                        val.push('\n');
                    }
                }

                print_mods(attribute_mods, "The following content has attribution requirements, meaning that you must link back to the page where you originally found this content in your modpack description or version changelog (e.g. linking a mod's CurseForge page if you got it from CurseForge):", &mut str);
                print_mods(no_mods, "The following content is not allowed in Modrinth modpacks due to licensing restrictions. Please contact the author(s) directly for permission or remove the content from your modpack:", &mut str);
                print_mods(permanent_no_mods, "The following content is not allowed in Modrinth modpacks, regardless of permission obtained. This may be because it breaks Modrinth's content rules or because the authors, upon being contacted for permission, have declined. Please remove the content from your modpack:", &mut str);
                print_mods(unidentified_mods, "The following content could not be identified. Please provide proof of its origin along with proof that you have permission to include it:", &mut str);

                str
            }
        }
    }
}

pub struct AutomatedModerationQueue {
    pub projects: DashSet<ProjectId>,
}

impl Default for AutomatedModerationQueue {
    fn default() -> Self {
        Self {
            projects: DashSet::new(),
        }
    }
}

impl AutomatedModerationQueue {
    pub async fn task(&self, pool: PgPool, redis: RedisPool) {
        loop {
            let projects = self.projects.clone();
            self.projects.clear();

            for project in projects {
                async {
                    let project =
                        database::Project::get_id((project).into(), &pool, &redis).await?;

                    if let Some(project) = project {
                        let res = async {
                            let mut mod_messages = ModerationMessages {
                                messages: vec![],
                                version_specific: HashMap::new(),
                            };

                            let versions =
                                database::Version::get_many(&project.versions, &pool, &redis)
                                    .await?
                                    .into_iter()
                                    // we only support modpacks at this time
                                    .filter(|x| x.project_types.contains(&"modpack".to_string()))
                                    .collect::<Vec<_>>();

                            for version in versions {
                                let primary_file = version.files.iter().find_or_first(|x| x.primary);

                                if let Some(primary_file) = primary_file {
                                    let data = reqwest::get(&primary_file.url).await?.bytes().await?;

                                    let reader = Cursor::new(data);
                                    let mut zip = ZipArchive::new(reader)?;

                                    let pack: PackFormat = {
                                        let mut file =
                                            if let Ok(file) = zip.by_name("modrinth.index.json") {
                                                file
                                            } else {
                                                continue;
                                            };

                                        let mut contents = String::new();
                                        file.read_to_string(&mut contents)?;

                                        serde_json::from_str(&contents)?
                                    };

                                    // sha1, pack file, file path, murmur
                                    let mut hashes: Vec<(
                                        String,
                                        Option<PackFile>,
                                        String,
                                        Option<u32>,
                                    )> = pack
                                        .files
                                        .clone()
                                        .into_iter()
                                        .flat_map(|x| {
                                            let hash = x.hashes.get(&PackFileHash::Sha1);

                                            if let Some(hash) = hash {
                                                let path = x.path.clone();
                                                Some((hash.clone(), Some(x), path, None))
                                            } else {
                                                None
                                            }
                                        })
                                        .collect();

                                    for i in 0..zip.len() {
                                        let mut file = zip.by_index(i)?;

                                        if file.name().starts_with("overrides/mods")
                                            || file.name().starts_with("client-overrides/mods")
                                            || file.name().starts_with("server-overrides/mods")
                                            || file.name().starts_with("overrides/shaderpacks")
                                            || file.name().starts_with("client-overrides/shaderpacks")
                                            || file.name().starts_with("overrides/resourcepacks")
                                            || file.name().starts_with("client-overrides/resourcepacks")
                                        {
                                            let mut contents = Vec::new();
                                            file.read_to_end(&mut contents)?;

                                            let hash = sha1::Sha1::from(&contents).hexdigest();
                                            let murmur = hash_flame_murmur32(contents);

                                            hashes.push((
                                                hash,
                                                None,
                                                file.name().to_string(),
                                                Some(murmur),
                                            ));
                                        }
                                    }

                                    let files = database::models::Version::get_files_from_hash(
                                        "sha1".to_string(),
                                        &hashes.iter().map(|x| x.0.clone()).collect::<Vec<_>>(),
                                        &pool,
                                        &redis,
                                    )
                                        .await?;

                                    let version_ids =
                                        files.iter().map(|x| x.version_id).collect::<Vec<_>>();
                                    let versions_data = filter_visible_versions(
                                        database::models::Version::get_many(
                                            &version_ids,
                                            &pool,
                                            &redis,
                                        )
                                            .await?,
                                        &None,
                                        &pool,
                                        &redis,
                                    )
                                        .await?;

                                    let mut final_hashes = HashMap::new();

                                    for version in versions_data {
                                        for file in
                                        files.iter().filter(|x| x.version_id == version.id.into())
                                        {
                                            if let Some(hash) = file.hashes.get(&"sha1".to_string()) {
                                                if let Some((index, (_, _, file_name, _))) = hashes
                                                    .iter()
                                                    .enumerate()
                                                    .find(|(_, (value, _, _, _))| value == hash)
                                                {
                                                    final_hashes
                                                        .insert(file_name.clone(), ApprovalType::Yes);

                                                    hashes.remove(index);

                                                }
                                            }
                                        }
                                    }

                                    // All files are on Modrinth, so we don't send any messages
                                    if hashes.is_empty() {
                                        continue;
                                    }

                                    let rows = sqlx::query!(
                                        "
                                        SELECT encode(mef.sha1, 'escape') sha1, mel.status status
                                        FROM moderation_external_files mef
                                        INNER JOIN moderation_external_licenses mel ON mef.external_license_id = mel.id
                                        WHERE mef.sha1 = ANY($1)
                                        ",
                                        &hashes.iter().map(|x| x.0.as_bytes().to_vec()).collect::<Vec<_>>()
                                    )
                                        .fetch_all(&pool)
                                        .await?;

                                    for row in rows {
                                        if let Some(sha1) = row.sha1 {
                                            if let Some((index, (_, _, file_name, _))) = hashes.iter().enumerate().find(|(_, (value, _, _, _))| value == &sha1) {
                                                final_hashes.insert(file_name.clone(), ApprovalType::from_str(&row.status).unwrap_or(ApprovalType::Unidentified));
                                                hashes.remove(index);
                                            }
                                        }
                                    }

                                    if hashes.is_empty() {
                                        if final_hashes.values().any(|x| x != &ApprovalType::Yes && x != &ApprovalType::WithAttributionAndSource) {
                                            let val = mod_messages.version_specific.entry(version.inner.version_number).or_default();
                                            val.push(ModerationMessage::PackFilesNotAllowed {files: final_hashes, incomplete: false });
                                        }
                                        continue;
                                    }

                                    let client = reqwest::Client::new();
                                    let res = client
                                        .post(format!("{}/v1/fingerprints", dotenvy::var("FLAME_ANVIL_URL")?))
                                        .json(&serde_json::json!({
                                        "fingerprints": hashes.iter().filter_map(|x| x.3).collect::<Vec<u32>>()
                                    }))
                                        .send()
                                        .await?.text()
                                        .await?;

                                    let flame_hashes = serde_json::from_str::<FlameResponse<FingerprintResponse>>(&res)?
                                        .data
                                        .exact_matches
                                        .into_iter()
                                        .map(|x| x.file)
                                        .collect::<Vec<_>>();

                                    let mut flame_files = Vec::new();

                                    for file in flame_hashes {
                                        let hash = file
                                            .hashes
                                            .iter()
                                            .find(|x| x.algo == 1)
                                            .map(|x| x.value.clone());

                                        if let Some(hash) = hash  {
                                            flame_files.push((hash, file.mod_id))
                                        }
                                    }

                                    let rows = sqlx::query!(
                                        "
                                        SELECT mel.id, mel.flame_project_id, mel.status status
                                        FROM moderation_external_licenses mel
                                        WHERE mel.flame_project_id = ANY($1)
                                        ",
                                        &flame_files.iter().map(|x| x.1 as i32).collect::<Vec<_>>()
                                    )
                                        .fetch_all(&pool).await?;

                                    let mut insert_hashes = Vec::new();
                                    let mut insert_ids = Vec::new();

                                    for row in rows {
                                        if let Some((curse_index, (hash, _flame_id))) = flame_files.iter().enumerate().find(|(_, x)| Some(x.1 as i32) == row.flame_project_id) {
                                            if let Some((index, (_, _, file_name, _))) = hashes.iter().enumerate().find(|(_, (value, _, _, _))| value == hash) {
                                                final_hashes.insert(file_name.clone(), ApprovalType::from_str(&row.status).unwrap_or(ApprovalType::Unidentified));

                                                insert_hashes.push(hash.clone().as_bytes().to_vec());
                                                insert_ids.push(row.id);

                                                hashes.remove(index);
                                                flame_files.remove(curse_index);
                                            }
                                        }
                                    }

                                    if !insert_ids.is_empty() && !insert_hashes.is_empty() {
                                        sqlx::query!(
                                            "
                                            INSERT INTO moderation_external_files (sha1, external_license_id)
                                            SELECT * FROM UNNEST ($1::bytea[], $2::bigint[])
                                            ",
                                            &insert_hashes[..],
                                            &insert_ids[..]
                                        )
                                            .execute(&pool)
                                            .await?;
                                    }

                                    if hashes.is_empty() {
                                        if final_hashes.values().any(|x| x != &ApprovalType::Yes && x != &ApprovalType::WithAttributionAndSource) {
                                            let val = mod_messages.version_specific.entry(version.inner.version_number).or_default();
                                            val.push(ModerationMessage::PackFilesNotAllowed {files: final_hashes, incomplete: false });
                                        }
                                        continue;
                                    }

                                    let flame_projects  = if flame_files.is_empty() {
                                        Vec::new()
                                    } else {
                                        let res = client
                                            .post(format!("{}v1/mods", dotenvy::var("FLAME_ANVIL_URL")?))
                                            .json(&serde_json::json!({
                                                "modIds": flame_files.iter().map(|x| x.1).collect::<Vec<_>>()
                                            }))
                                            .send()
                                            .await?
                                            .text()
                                            .await?;

                                        serde_json::from_str::<FlameResponse<Vec<FlameProject>>>(&res)?.data
                                    };

                                    let mut missing_metadata = MissingMetadata {
                                        identified: final_hashes,
                                        flame_files: HashMap::new(),
                                        unknown_files: vec![],
                                    };

                                    for (sha1, _pack_file, file_name, _mumur2) in hashes {
                                        let flame_file = flame_files.iter().find(|x| x.0 == sha1);

                                        if let Some((_, flame_project_id)) = flame_file {
                                            if let Some(project) = flame_projects.iter().find(|x| &x.id == flame_project_id) {
                                                missing_metadata.flame_files.insert(file_name, MissingMetadataFlame {
                                                    url: project.links.website_url.clone(),
                                                    id: *flame_project_id,
                                                });

                                                continue;
                                            }
                                        }

                                        missing_metadata.unknown_files.push(file_name);
                                    }

                                    sqlx::query!(
                                        "
                                        UPDATE files
                                        SET metadata = $1
                                        WHERE id = $2
                                        ",
                                        serde_json::to_value(&missing_metadata)?,
                                        primary_file.id.0
                                    )
                                        .execute(&pool)
                                        .await?;

                                    if missing_metadata.identified.values().any(|x| x != &ApprovalType::Yes && x != &ApprovalType::WithAttributionAndSource) {
                                        let val = mod_messages.version_specific.entry(version.inner.version_number).or_default();
                                        val.push(ModerationMessage::PackFilesNotAllowed {files: missing_metadata.identified, incomplete: true });
                                    }
                                } else {
                                    let val = mod_messages.version_specific.entry(version.inner.version_number).or_default();
                                    val.push(ModerationMessage::NoPrimaryFile);
                                }
                            }

                            if !mod_messages.is_empty() {
                                let first_time = database::models::Thread::get(project.thread_id, &pool).await?
                                    .map(|x| x.messages.iter().all(|x| match x.body { MessageBody::Text { hide_identity, .. } => x.author_id == Some(database::models::UserId(AUTOMOD_ID)) || hide_identity, _ => true}))
                                    .unwrap_or(true);

                                let mut transaction = pool.begin().await?;
                                let id = ThreadMessageBuilder {
                                    author_id: Some(database::models::UserId(AUTOMOD_ID)),
                                    body: MessageBody::Text {
                                        body: mod_messages.markdown(),
                                        private: false,
                                        hide_identity: false,
                                        replying_to: None,
                                        associated_images: vec![],
                                    },
                                    thread_id: project.thread_id,
                                }
                                    .insert(&mut transaction)
                                    .await?;

                                let members = database::models::TeamMember::get_from_team_full(
                                    project.inner.team_id,
                                    &pool,
                                    &redis,
                                )
                                    .await?;

                                if mod_messages.should_reject(first_time) {
                                    ThreadMessageBuilder {
                                        author_id: Some(database::models::UserId(AUTOMOD_ID)),
                                        body: MessageBody::StatusChange {
                                            new_status: ProjectStatus::Rejected,
                                            old_status: project.inner.status,
                                        },
                                        thread_id: project.thread_id,
                                    }
                                        .insert(&mut transaction)
                                        .await?;

                                    NotificationBuilder {
                                        body: NotificationBody::StatusChange {
                                            project_id: project.inner.id.into(),
                                            old_status: project.inner.status,
                                            new_status: ProjectStatus::Rejected,
                                        },
                                    }
                                        .insert_many(members.into_iter().map(|x| x.user_id).collect(), &mut transaction, &redis)
                                        .await?;

                                    if let Ok(webhook_url) = dotenvy::var("MODERATION_DISCORD_WEBHOOK") {
                                        crate::util::webhook::send_discord_webhook(
                                            project.inner.id.into(),
                                            &pool,
                                            &redis,
                                            webhook_url,
                                            Some(
                                                format!(
                                                    "**[AutoMod]({}/user/AutoMod)** changed project status from **{}** to **Rejected**",
                                                    dotenvy::var("SITE_URL")?,
                                                    &project.inner.status.as_friendly_str(),
                                                )
                                                    .to_string(),
                                            ),
                                        )
                                            .await
                                            .ok();
                                    }

                                    sqlx::query!(
                                        "
                                        UPDATE mods
                                        SET status = 'rejected'
                                        WHERE id = $1
                                        ",
                                        project.inner.id.0
                                    )
                                        .execute(&pool)
                                        .await?;

                                    database::models::Project::clear_cache(
                                        project.inner.id,
                                        project.inner.slug.clone(),
                                        None,
                                        &redis,
                                    )
                                        .await?;
                                } else {
                                    NotificationBuilder {
                                        body: NotificationBody::ModeratorMessage {
                                            thread_id: project.thread_id.into(),
                                            message_id: id.into(),
                                            project_id: Some(project.inner.id.into()),
                                            report_id: None,
                                        },
                                    }
                                        .insert_many(
                                            members.into_iter().map(|x| x.user_id).collect(),
                                            &mut transaction,
                                            &redis,
                                        )
                                        .await?;
                                }

                                transaction.commit().await?;
                            }

                            Ok::<(), ApiError>(())
                        }.await;

                        if let Err(err) = res {
                            let err = err.as_api_error();

                            let mut str = String::new();
                            str.push_str("## Internal AutoMod Error\n\n");
                            str.push_str(&format!("Error code: {}\n\n", err.error));
                            str.push_str(&format!("Error description: {}\n\n", err.description));

                            let mut transaction = pool.begin().await?;
                            ThreadMessageBuilder {
                                author_id: Some(database::models::UserId(AUTOMOD_ID)),
                                body: MessageBody::Text {
                                    body: str,
                                    private: true,
                                    hide_identity: false,
                                    replying_to: None,
                                    associated_images: vec![],
                                },
                                thread_id: project.thread_id,
                            }
                                .insert(&mut transaction)
                                .await?;
                            transaction.commit().await?;
                        }
                    }

                    Ok::<(), ApiError>(())
                }.await.ok();
            }

            tokio::time::sleep(Duration::from_secs(5)).await
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct MissingMetadata {
    pub identified: HashMap<String, ApprovalType>,
    pub flame_files: HashMap<String, MissingMetadataFlame>,
    pub unknown_files: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct MissingMetadataFlame {
    pub url: String,
    pub id: u32,
}

#[derive(Deserialize, Serialize, Copy, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalType {
    Yes,
    WithAttributionAndSource,
    WithAttribution,
    No,
    PermanentNo,
    Unidentified,
}

impl ApprovalType {
    fn approved(&self) -> bool {
        match self {
            ApprovalType::Yes => true,
            ApprovalType::WithAttributionAndSource => true,
            ApprovalType::WithAttribution => true,
            ApprovalType::No => false,
            ApprovalType::PermanentNo => false,
            ApprovalType::Unidentified => false,
        }
    }

    fn from_str(string: &str) -> Option<Self> {
        match string {
            "yes" => Some(ApprovalType::Yes),
            "with-attribution-and-source" => Some(ApprovalType::WithAttributionAndSource),
            "with-attribution" => Some(ApprovalType::WithAttribution),
            "no" => Some(ApprovalType::No),
            "permanent-no" => Some(ApprovalType::PermanentNo),
            "unidentified" => Some(ApprovalType::Unidentified),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct FlameResponse<T> {
    pub data: T,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintResponse {
    pub exact_matches: Vec<FingerprintMatch>,
}

#[derive(Deserialize, Serialize)]
pub struct FingerprintMatch {
    pub id: u32,
    pub file: FlameFile,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlameFile {
    pub id: u32,
    pub mod_id: u32,
    pub hashes: Vec<FlameFileHash>,
    pub file_fingerprint: u32,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct FlameFileHash {
    pub value: String,
    pub algo: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlameProject {
    pub id: u32,
    pub name: String,
    pub slug: String,
    pub links: FlameLinks,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlameLinks {
    pub website_url: String,
}

fn hash_flame_murmur32(input: Vec<u8>) -> u32 {
    murmur2::murmur2(
        &input
            .into_iter()
            .filter(|x| *x != 9 && *x != 10 && *x != 13 && *x != 32)
            .collect::<Vec<u8>>(),
        1,
    )
}
