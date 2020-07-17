use bson::doc;
use futures::StreamExt;
use log::info;

use crate::database::{Mod, Version};

use super::IndexingError;
use crate::search::SearchMod;
use sqlx::postgres::PgPool;

pub async fn index_local(pool: PgPool) -> Result<Vec<SearchMod>, IndexingError> {
    info!("Indexing local mods!");

    let mut docs_to_add: Vec<SearchMod> = vec![];
    /*
    let db = pool.database("modrinth");

    let mods = db.collection("mods");
    let versions = db.collection("versions");
    /*let mut results = mods
    .find(None, None)
    .await
    .map_err(DatabaseError::LocalDatabaseError)?;*/
    */

    let mut results = sqlx::query!(
        "
        SELECT m.id, m.title, m.description, m.downloads, m.icon_url, c
        FROM mods m
            INNER JOIN mods_categories mc ON m.id=mc.joining_mod_id
            INNER JOIN categories categories ON mc.joining_category_id=c.id
        "
    )
    .fetch(&pool);

    while let Some(result) = results.next().await {
        if let Ok(result) = result {
            /*
            let result: Mod =
                *Mod::from_doc(unparsed_result.map_err(DatabaseError::LocalDatabaseError)?)?;

            let mut mod_versions = versions
                .find(doc! { "mod_id": result.id }, None)
                .await
                .map_err(DatabaseError::LocalDatabaseError)?;

            let mut mod_game_versions = vec![];

            while let Some(unparsed_version) = mod_versions.next().await {
                let mut version = unparsed_version
                    .map_err(DatabaseError::LocalDatabaseError)
                    .and_then(Version::from_doc)?;
                mod_game_versions.append(&mut version.game_versions);
            }
            */

            let versions = sqlx::query!(
                "
                SELECT * FROM versions
                WHERE mod_id = $1
                ",
                result.id
            )
            .fetch_all(&pool)
            .await?;

            let mut icon_url = "".to_string();

            if let Some(url) = result.icon_url {
                icon_url = url;
            }

            docs_to_add.push(SearchMod {
                mod_id: result.id,
                author: "".to_string(),
                title: result.title,
                description: result.description,
                keywords: result.category,
                versions: versions,
                downloads: result.downloads,
                page_url: "".to_string(),
                icon_url,
                author_url: "".to_string(),
                date_created: "".to_string(),
                created: 0,
                date_modified: "".to_string(),
                updated: 0,
                latest_version: "".to_string(),
                empty: String::from("{}{}{}"),
            });
        }
    }

    Ok(docs_to_add)
}
