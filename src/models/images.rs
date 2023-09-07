use super::{
    ids::{Base62Id, ProjectId, ThreadMessageId, VersionId},
    pats::Scopes,
    reports::ReportId,
};
use crate::models::ids::UserId;
use crate::{
    database::{
        self,
        models::image_item::{self, Image as DBImage},
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

    pub context: ImageContext,
}

impl From<DBImage> for Image {
    fn from(x: DBImage) -> Self {
        Image {
            id: x.id.into(),
            url: x.url,
            size: x.size,
            created: x.created,
            owner_id: x.owner_id.into(),
            context: x.context,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum ImageContext {
    Project {
        project_id: Option<ProjectId>,
    },
    Version {
        // version changelogs
        version_id: Option<VersionId>,
    },
    ThreadMessage {
        thread_message_id: Option<ThreadMessageId>,
    },
    Report {
        report_id: Option<ReportId>,
    },
    Unknown,
}

impl ImageContext {
    pub fn context_as_str(&self) -> &'static str {
        match self {
            ImageContext::Project { .. } => "project",
            ImageContext::Version { .. } => "version",
            ImageContext::ThreadMessage { .. } => "thread_message",
            ImageContext::Report { .. } => "report",
            ImageContext::Unknown => "unknown",
        }
    }

    pub fn inner_id(&self) -> Option<u64> {
        match self {
            ImageContext::Project { project_id } => project_id.map(|x| x.0),
            ImageContext::Version { version_id } => version_id.map(|x| x.0),
            ImageContext::ThreadMessage { thread_message_id } => thread_message_id.map(|x| x.0),
            ImageContext::Report { report_id } => report_id.map(|x| x.0),
            ImageContext::Unknown => None,
        }
    }
    pub fn relevant_scope(&self) -> Scopes {
        match self {
            ImageContext::Project { .. } => Scopes::PROJECT_WRITE,
            ImageContext::Version { .. } => Scopes::VERSION_WRITE,
            ImageContext::ThreadMessage { .. } => Scopes::THREAD_WRITE,
            ImageContext::Report { .. } => Scopes::REPORT_WRITE,
            ImageContext::Unknown => Scopes::NONE,
        }
    }
    pub fn from_str(context: &str, id: Option<u64>) -> Self {
        match context {
            "project" => ImageContext::Project {
                project_id: id.map(ProjectId),
            },
            "version" => ImageContext::Version {
                version_id: id.map(VersionId),
            },
            "thread_message" => ImageContext::ThreadMessage {
                thread_message_id: id.map(ThreadMessageId),
            },
            "report" => ImageContext::Report {
                report_id: id.map(ReportId),
            },
            _ => ImageContext::Unknown,
        }
    }
}

// check changes to associated images
// if they no longer exist in the String list, delete them
// Eg: if description is modified and no longer contains a link to an iamge
pub async fn delete_unused_images(
    context: ImageContext,
    reference_strings: Vec<&str>,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    redis: &deadpool_redis::Pool,
) -> Result<(), ApiError> {
    let uploaded_images = database::models::Image::get_many_contexted(context, transaction).await?;

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
