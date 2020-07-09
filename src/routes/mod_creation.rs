use crate::file_hosting::{upload_file, AuthorizationData, UploadUrlData};
use crate::models::ids::random_base62;
use crate::models::mods::{
    FileHash, GameVersion, Mod, ModId, Version, VersionFile, VersionId, VersionType,
};
use crate::models::teams::{Team, TeamId, TeamMember};
use actix_multipart::{Field, Multipart};
use actix_web::web::Data;
use actix_web::{middleware, post, HttpResponse};
use bson::Bson;
use chrono::Utc;
use futures::StreamExt;
use mongodb::Client;
use pulldown_cmark::html::push_html;
use pulldown_cmark::{Options, Parser};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
struct InitialVersionData {
    pub file_names: Vec<String>,
    pub version_slug: String,
    pub version_title: String,
    pub version_body: String,
    pub dependencies: Vec<ModId>,
    pub game_versions: Vec<GameVersion>,
    pub version_type: VersionType,
}

#[derive(Serialize, Deserialize, Clone)]
struct ModCreateData {
    /// The title or name of the mod.
    pub mod_name: String,
    /// The namespace of the mod
    pub mod_namespace: String,
    /// A short description of the mod.
    pub mod_description: String,
    /// A long description of the mod, in markdown.
    pub mod_body: String,
    /// A list of initial versions to upload with the created mod
    pub initial_versions: Vec<InitialVersionData>,
    /// The team of people that has ownership of this mod.
    pub team_members: Vec<TeamMember>,
    /// A list of the categories that the mod is in.
    pub categories: Vec<String>,
    /// An optional link to where to submit bugs or issues with the mod.
    pub issues_url: Option<String>,
    /// An optional link to the source code for the mod.
    pub source_url: Option<String>,
    /// An optional link to the mod's wiki page or other relevant information.
    pub wiki_url: Option<String>,
}

#[post("api/v1/mod")]
pub async fn mod_create(
    mut payload: Multipart,
    client: Data<Client>,
    upload_url: Data<UploadUrlData>,
) -> HttpResponse {
    let db = client.database("modrinth");

    let mods = db.collection("mods");
    let versions = db.collection("versions");

    let mod_id = ModId(random_base62(8));
    let version_ids: Vec<VersionId> = vec![];

    let mut mod_create_data: Option<ModCreateData> = None;
    let mut icon_url = "".to_string();

    while let Some(item) = payload.next().await {
        let mut field: Field = item.expect("Error while splitting payload");
        let content_type = field.content_disposition().unwrap();
        let name = content_type.get_name().unwrap();

        while let Some(chunk) = field.next().await {
            let data = &chunk.expect("Error while splitting payload");

            if name == "data" {
                mod_create_data = Some(serde_json::from_slice(&data).unwrap());
            } else {
                let file_name = content_type.get_filename().expect("Expected Filename");
                let file_extension = String::from_utf8(
                    content_type
                        .get_filename_ext()
                        .expect("Expected icon extension!")
                        .clone()
                        .value,
                )
                .unwrap();

                if let Some(create_data) = mod_create_data.clone() {
                    if name == "icon" {
                        //TODO: Check file extension if valid -> match to BackBlaze content type, OR reject icon
                        let upload_data = upload_file(
                            upload_url.get_ref().clone(),
                            "image/png".to_string(),
                            format!("mods/icons/{}/{}", mod_id.0, file_name),
                            data.to_vec(),
                        )
                        .await
                        .unwrap();

                        icon_url = format!("cdnurl/{}", upload_data.file_name)
                    } else if file_extension == "jar".to_string() {
                        let initial_version_data = create_data
                            .initial_versions
                            .iter()
                            .position(|x| x.file_names.contains(&file_name.to_string()));

                        if let Some(version_data_index) = initial_version_data {
                            let version_data: InitialVersionData = create_data
                                .initial_versions
                                .get(version_data_index)
                                .unwrap()
                                .clone();
                            let version_id = VersionId(random_base62(8));

                            let mut parser =
                                Parser::new_ext(&*version_data.version_body, Options::empty());

                            let mut parsed_body = String::new();
                            push_html(&mut parsed_body, parser);

                            let body_url =
                                format!("data/{}/changelogs/{}/body.html", mod_id.0, version_id.0);

                            upload_file(
                                upload_url.get_ref().clone(),
                                "text/html".to_string(),
                                body_url.clone(),
                                parsed_body.into_bytes(),
                            )
                            .await
                            .unwrap();

                            let upload_data = upload_file(
                                upload_url.get_ref().clone(),
                                "application/java-archive".to_string(),
                                format!(
                                    "{}/{}/{}",
                                    create_data.mod_namespace.replace(".", "/"),
                                    version_data.version_slug,
                                    file_name
                                ),
                                (&data).to_owned().to_vec(),
                            )
                            .await
                            .unwrap();

                            let version = Version {
                                id: version_id,
                                mod_id,
                                name: version_data.version_title,
                                changelog_url: Some(format!("cdnurl/{}", body_url)),
                                date_published: Utc::now(),
                                downloads: 0,
                                version_type: version_data.version_type,
                                files: vec![VersionFile {
                                    game_versions: version_data.game_versions,
                                    hashes: vec![FileHash {
                                        algorithm: "sha1".to_string(),
                                        hash: upload_data.content_sha1,
                                    }],
                                    url: format!("cdnurl/{}", upload_data.file_name),
                                }],
                                dependencies: version_data.dependencies,
                            };
                            //TODO: Malware scan + file validation

                            let serialized_version = serde_json::to_string(&version).unwrap();
                            let document = Bson::from(serialized_version)
                                .as_document()
                                .unwrap()
                                .clone();

                            versions.insert_one(document, None).await.unwrap();
                        }
                    }
                }
            }
        }
    }

    if let Some(create_data) = mod_create_data {
        let mut parser = Parser::new_ext(&*create_data.mod_body, Options::empty());

        let mut parsed_body = String::new();
        push_html(&mut parsed_body, parser);

        let body_url = format!("data/{}/body.html", mod_id.0);

        upload_file(
            upload_url.get_ref().clone(),
            "text/html".to_string(),
            body_url.clone(),
            parsed_body.into_bytes(),
        )
        .await
        .unwrap();

        //TODO checks to see if randomly generated ids match
        let created_mod: Mod = Mod {
            id: mod_id,
            team: Team {
                id: TeamId(random_base62(8)),
                members: create_data.team_members,
            },
            title: create_data.mod_name,
            icon_url,
            description: create_data.mod_description,
            body_url: format!("cdnurl/{}", body_url),
            published: Utc::now(),
            downloads: 0,
            categories: create_data.categories,
            versions: version_ids,
            issues_url: create_data.issues_url,
            source_url: create_data.source_url,
            wiki_url: create_data.wiki_url,
        };

        let serialized_mod = serde_json::to_string(&created_mod).unwrap();
        let document = Bson::from(serialized_mod).as_document().unwrap().clone();

        mods.insert_one(document, None).await.unwrap();
    }

    HttpResponse::Ok().into()
}
