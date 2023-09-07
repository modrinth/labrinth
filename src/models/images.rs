use super::{
    ids::{Base62Id, ProjectId, ThreadMessageId, VersionId},
    pats::Scopes,
    reports::ReportId,
};
use crate::{models::ids::UserId, database::models::{image_item::QueryImage, ImageContextTypeId, categories::ImageContextType}};
use crate::{
    database::{
        self,
        models::image_item,
    },
    routes::ApiError,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ImageId(pub u64);

#[derive(Serialize, Deserialize)]
pub struct Image {
    pub id: ImageId,
    pub url: String,
    pub size: u64,
    pub created: DateTime<Utc>,
    pub owner_id: UserId,

    // context it is associated with (at most one)
    pub project_id: Option<ProjectId>,
    pub version_id: Option<VersionId>,
    pub thread_message_id: Option<ThreadMessageId>,
    pub report_id: Option<ReportId>,
}

impl From<QueryImage> for Image {
    fn from(x: QueryImage) -> Self {

        let mut image = Image {
            id: x.id.into(),
            url: x.url,
            size: x.size,
            created: x.created,
            owner_id: x.owner_id.into(),

            project_id: None,
            version_id: None,
            thread_message_id: None,
            report_id: None,
        };

        match x.context_type_name.as_str() {
            "project" => {
                image.project_id = x.context_id.map(|x| ProjectId(x as u64));
            },
            "version" => {
                image.version_id = x.context_id.map(|x| VersionId(x as u64));
            },
            "thread_message" => {
                image.thread_message_id = x.context_id.map(|x| ThreadMessageId(x as u64));
            },
            "report" => {
                image.report_id = x.context_id.map(|x| ReportId(x as u64));
            },
            _ => {},
        }
        
        image
    }
}

impl ImageContextType {
    pub fn relevant_scope(name : &str) -> Option<Scopes> {
        match name {
            "project" => Some(Scopes::PROJECT_WRITE),
            "version" => Some(Scopes::VERSION_WRITE),
            "thread_message" => Some(Scopes::THREAD_WRITE),
            "report" => Some(Scopes::REPORT_WRITE),
            _ => None,
        }
    }
}

// check changes to associated images
// if they no longer exist in the String list, delete them
// Eg: if description is modified and no longer contains a link to an iamge
pub async fn delete_unused_images(
    context_type_id: ImageContextTypeId,
    context_id: u64,
    reference_strings: Vec<&str>,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    redis: &deadpool_redis::Pool,
) -> Result<(), ApiError> {
    let uploaded_images = database::models::Image::get_many_contexted(context_type_id, context_id as i64, transaction).await?;

    for image in uploaded_images {
        let mut should_delete = true;
        for reference in &reference_strings {
            if image.url.contains(reference) {
                should_delete = false;
                break;
            }
        }

        if should_delete {
            image_item::Image::remove(image.id, transaction, redis).await?;
            image_item::Image::clear_cache(image.id, redis).await?;
        }
    }

    Ok(())
}
