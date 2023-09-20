use super::{
    ids::{Base62Id, TeamId},
    projects::DonationLink,
    teams::{ProjectPermissions, TeamMember},
};
use serde::{Deserialize, Serialize};

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
    /// The name of the organization
    pub name: String,
    /// The description of the organization
    pub description: String,
    /// The website url of the organization
    pub website_url: Option<String>,
    /// The discord url of the organization
    pub discord_url: Option<String>,
    /// The donation links for the organization
    pub donation_urls: Option<Vec<DonationLink>>,

    /// The icon url of the organization
    pub icon_url: Option<String>,
    /// The color of the organization (picked from the icon)
    pub color: Option<u32>,

    /// Default settings for projects in this organization
    /// (e.g: a member of this org who is not a member of a project in this org will have these permissions for that project)
    /// These are hidden outside org.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_project_permissions: Option<ProjectPermissions>,

    /// A list of the members of the organization
    pub members: Vec<TeamMember>,
}

impl Organization {
    pub fn from(
        data: crate::database::models::organization_item::Organization,
        user_option: &Option<crate::models::users::User>,
        team_members: Vec<TeamMember>,
    ) -> Self {
        // Only show permissions if the user is a member of the team (team is an Organization team)
        let show_permissions = team_members
            .iter()
            .any(|m| Some(m.user.id) == user_option.as_ref().map(|u| u.id));
        Self {
            id: data.id.into(),
            slug: data.slug,
            team_id: data.team_id.into(),
            name: data.name,
            description: data.description,
            members: team_members,
            website_url: data.website_url,
            discord_url: data.discord_url,
            donation_urls: Some(
                data.donation_urls
                    .into_iter()
                    .map(|d| DonationLink {
                        id: d.platform_short,
                        platform: d.platform_name,
                        url: d.url,
                    })
                    .collect(),
            ),
            icon_url: data.icon_url,
            color: data.color,
            default_project_permissions: if show_permissions {
                Some(data.default_project_permissions)
            } else {
                None
            },
        }
    }
}
