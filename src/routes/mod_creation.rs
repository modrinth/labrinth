use crate::file_hosting::{upload_file, AuthorizationData, UploadUrlData};
use crate::models::ids::random_base62;
use crate::models::mods::{GameVersion, Mod, ModId, VersionType, VersionId};
use crate::models::teams::{Team, TeamId, TeamMember};
use actix_multipart::{Field, Multipart};
use actix_web::web::Data;
use actix_web::{middleware, post, HttpResponse};
use chrono::Utc;
use futures::StreamExt;
use mongodb::Client;
use pulldown_cmark::html::push_html;
use pulldown_cmark::{Options, Parser};
use serde::{Deserialize, Serialize};

struct InitialVersionData {
    pub file_name: String,
    pub version_slug: String,
    pub version_title: String,
    pub version_description: String,
    pub dependencies: Vec<ModId>,
    pub game_versions: Vec<GameVersion>,
    pub version_type: VersionType,
}

#[derive(Serialize, Deserialize, Debug)]
struct ModCreateData {
    /// The title or name of the mod.
    pub mod_name: String,
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
    authorization: Data<AuthorizationData>,
    upload_url: Data<UploadUrlData>,
) -> HttpResponse {
    let db = client.database("modrinth");

    let mods = db.collection("mods");
    let versions = db.collection("versions");

    let mod_id = ModId(random_base62(8));
    let version_ids : Vec<VersionId> = vec![];

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

                if let Some(create_data) = mod_create_data.clone() {
                    if name == "icon" {
                        let file_extension = content_type.get_filename_ext().expect("Expected icon extension!");

                        file_extension.
                        //TODO: Check file extension if valid -> match to BackBlaze content type, OR reject icon
                        let upload_data = upload_file(
                            **upload_url,
                            "image/png".to_string(),
                            format!("mods/icons/{}/{}", mod_id.0, file_name),
                            data.into_vec(),
                        )
                            .await
                            .unwrap();

                        icon_url = format!("cdnurl/{}", upload_data.file_name)
                    } else {
                        let upload_data = upload_file(
                            **upload_url,
                            "application/java-archive".to_string(),
                            format!(""),
                            data.into_vec()
                        ).await.unwrap();
                        //TODO: Malware scan + file validation
                    }
                }
            }
        }
    }

    if let Some(create_data) = mod_create_data {
        let mut parser = Parser::new_ext(&*create_data.mod_body, Options::empty());

        let mut parsed_body = String::new();
        push_html(&mut parsed_body, parser);

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
            body_url: parsed_body,
            published: Utc::now(),
            downloads: 0,
            categories: create_data.categories,
            versions: create_data.initial_versions,
            issues_url: create_data.issues_url,
            source_url: create_data.source_url,
            wiki_url: create_data.wiki_url,
        };

    }

    HttpResponse::Ok().into()
}
