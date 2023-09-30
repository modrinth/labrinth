use super::{
    ids::{Base62Id, TeamId},
    teams::TeamMember,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

/// The ID of a team
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct OrganizationId(pub u64);

/// An organization of users who control a project
#[derive(Serialize, Deserialize)]
pub struct Organization {
    /// The id of the organization
    pub id: OrganizationId,
    /// The slug of the organization
    pub slug: String,
    /// The associated team of the organization
    pub team_id: TeamId,
    /// The description of the organization
    pub description: String,
    /// Any attached urls of the organization
    /// ie: "discord" -> "https://discord.gg/..."
    pub link_urls: Option<Vec<UrlLink>>,

    /// The icon url of the organization
    pub icon_url: Option<String>,
    /// The color of the organization (picked from the icon)
    pub color: Option<u32>,

    /// A list of the members of the organization
    pub members: Vec<TeamMember>,
}

impl Organization {
    pub fn from(
        data: crate::database::models::organization_item::Organization,
        team_members: Vec<TeamMember>,
    ) -> Self {
        Self {
            id: data.id.into(),
            slug: data.slug,
            team_id: data.team_id.into(),
            description: data.description,
            members: team_members,
            link_urls: Some(
                data.link_urls
                    .into_iter()
                    .map(|d| UrlLink {
                        id: d.platform_short,
                        platform: d.platform_name,
                        url: d.url,
                    })
                    .collect(),
            ),
            icon_url: data.icon_url,
            color: data.color,
        }
    }
}

#[derive(Serialize, Deserialize, Validate, Clone, Eq, PartialEq)]
pub struct UrlLink {
    pub id: String,
    pub platform: String,
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub url: String,
}
