use super::{
    ids::{Base62Id, TeamId},
    teams::TeamMember,
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
            name: data.name,
            description: data.description,
            members: team_members,
        }
    }
}
