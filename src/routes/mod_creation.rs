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
use bson::doc;
use chrono::Utc;
use futures::stream::{Stream, StreamExt};
use mongodb::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
struct InitialVersionData {
    pub file_indexes: Vec<i32>,
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
    let cdn_url = dotenv::var("CDN_URL").unwrap();

    let db = client.database("modrinth");

    let mods = db.collection("mods");
    let versions = db.collection("versions");

    let mut mod_id = ModId(random_base62(8));

    //Check if ID is unique
    loop {
        let filter = doc! { "_id": mod_id.0 };

        if mods.find(filter, None).await.unwrap().next().await.is_some() {
            mod_id = ModId(random_base62(8));
        } else {
            break;
        }
    }

    let mut created_versions: Vec<Version> = vec![];

    let mut mod_create_data: Option<ModCreateData> = None;
    let mut icon_url = "".to_string();

    let mut current_file_index = 0;
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

                if let Some(create_data) = &mod_create_data {
                    if name == "icon" {
                        if let Some(ext) = get_image_content_type(file_extension) {
                            let upload_data = upload_file(
                                upload_url.get_ref().clone(),
                                "image/png".to_string(),
                                format!("mods/icons/{}/{}", mod_id.0, file_name),
                                data.to_vec(),
                            )
                                .await
                                .unwrap();

                            icon_url = format!("{}/{}", cdn_url, upload_data.file_name);
                        }
                        else {
                            panic!("Invalid Icon Format!");
                        }
                    } else if file_extension == "jar".to_string() {
                        let initial_version_data = create_data
                            .initial_versions
                            .iter()
                            .position(|x| x.file_indexes.contains(&current_file_index));

                        if let Some(version_data_index) = initial_version_data {
                            let version_data = create_data
                                .initial_versions
                                .get(version_data_index)
                                .unwrap()
                                .clone();

                            let mut created_version_filter = created_versions.iter().filter(|x| x.slug == version_data.version_slug).collect::<Vec<_>>();

                            if created_version_filter.len() > 0 {
                                //TODO: Make this compile let created_version = created_version_filter.get(0).unwrap();
                                //
                                // created_versions.retain(|x| x.id.0 != 0);
                                //
                                // let upload_data = upload_file(
                                //     upload_url.get_ref().clone(),
                                //     "application/java-archive".to_string(),
                                //     format!(
                                //         "{}/{}/{}",
                                //         create_data.mod_namespace.replace(".", "/"),
                                //         version_data.version_slug,
                                //         file_name
                                //     ),
                                //     (&data).to_owned().to_vec(),
                                // )
                                //     .await
                                //     .unwrap();
                                //
                                // // let mut new_created_version = created_version.clone();
                                // //
                                // // new_created_version.files.push(VersionFile {
                                // //     game_versions: version_data.game_versions,
                                // //     hashes: vec![FileHash {
                                // //         algorithm: "sha1".to_string(),
                                // //         hash: upload_data.content_sha1,
                                // //     }],
                                // //     url: format!("{}/{}", cdn_url, upload_data.file_name),
                                // // });
                                // // //created_version.files.push()
                            } else {
                                let mut version_id = VersionId(random_base62(8));
                                //Check if ID is unique
                                loop {
                                    let filter = doc! { "_id": version_id.0 };

                                    if versions.find(filter, None).await.unwrap().next().await.is_some() {
                                        version_id = VersionId(random_base62(8));
                                    } else {
                                        break;
                                    }
                                }

                                let body_url =
                                    format!("data/{}/changelogs/{}/body.md", mod_id.0, version_id.0);

                                upload_file(
                                    upload_url.get_ref().clone(),
                                    "text/plain".to_string(),
                                    body_url.clone(),
                                    version_data.version_body.into_bytes(),
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

                                let mut version = Version {
                                    id: version_id,
                                    mod_id,
                                    name: version_data.version_title,
                                    slug: version_data.version_slug.clone(),
                                    changelog_url: Some(format!("{}/{}", cdn_url, body_url)),
                                    date_published: Utc::now(),
                                    downloads: 0,
                                    version_type: version_data.version_type,
                                    files: vec![VersionFile {
                                        game_versions: version_data.game_versions,
                                        hashes: vec![FileHash {
                                            algorithm: "sha1".to_string(),
                                            hash: upload_data.content_sha1,
                                        }],
                                        url: format!("{}/{}", cdn_url, upload_data.file_name),
                                    }],
                                    dependencies: version_data.dependencies,
                                };
                                //TODO: Malware scan + file validation

                                created_versions.push(version);
                            }
                        }
                    }
                }
            }
        }

        current_file_index += 1;
    }

    for version in &created_versions {
        let serialized_version = serde_json::to_string(&version).unwrap();
        let document = Bson::from(serialized_version)
            .as_document()
            .unwrap()
            .clone();

        versions.insert_one(document, None).await.unwrap();
    }

    if let Some(create_data) = mod_create_data {
        let mut body_url =  format!("data/{}/body.md", mod_id.0);

        upload_file(
            upload_url.get_ref().clone(),
            "text/plain".to_string(),
            body_url.clone(),
            create_data.mod_body.into_bytes(),
        );

        let created_mod: Mod = Mod {
            id: mod_id,
            team: Team {
                id: TeamId(random_base62(8)),
                members: create_data.team_members,
            },
            title: create_data.mod_name,
            icon_url,
            description: create_data.mod_description,
            body_url: format!("{}/{}", cdn_url, body_url),
            published: Utc::now(),
            downloads: 0,
            categories: create_data.categories,
            versions: created_versions.into_iter().map(|x| x.id).collect::<Vec<_>>(),
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

fn get_image_content_type(extension: String) -> Option<String> {
    let content_type = match &*extension {
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "jpeg" | "jpg" | "jpe" => "image/jpeg",
        "png" => "image/png",
        "svg" | "svgz" => "image/svg+xml",
        "webp" => "image/webp",
        "rgb" => "image/x-rgb",
        _ => ""
    };

    if content_type != "" {
        return Some(content_type.to_string())
    } else {
        return None;
    }
}