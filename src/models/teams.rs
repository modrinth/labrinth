use super::ids::Base62Id;
use crate::models::users::User;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The ID of a team
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct TeamId(pub u64);

pub const OWNER_ROLE: &str = "Owner";
pub const DEFAULT_ROLE: &str = "Member";

/// A team of users who control a project
#[derive(Serialize, Deserialize)]
pub struct Team {
    /// The id of the team
    pub id: TeamId,
    /// A list of the members of the team
    pub members: Vec<TeamMember>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Permissions {
    Project(ProjectPermissions),
    Organization(OrganizationPermissions),
}


bitflags::bitflags! {
    #[derive(Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct ProjectPermissions: u64 {
        const UPLOAD_VERSION = 1 << 0;
        const DELETE_VERSION = 1 << 1;
        const EDIT_DETAILS = 1 << 2;
        const EDIT_BODY = 1 << 3;
        const MANAGE_INVITES = 1 << 4;
        const REMOVE_MEMBER = 1 << 5;
        const EDIT_MEMBER = 1 << 6;
        const DELETE_PROJECT = 1 << 7;
        const VIEW_ANALYTICS = 1 << 8;
        const VIEW_PAYOUTS = 1 << 9;

        const ALL = 0b1111111111;
    }
}

impl Default for ProjectPermissions {
    fn default() -> ProjectPermissions {
        ProjectPermissions::UPLOAD_VERSION | ProjectPermissions::DELETE_VERSION
    }
}

impl ProjectPermissions {
    pub fn get_permissions_by_role(
        role: &crate::models::users::Role,
        team_member: &Option<crate::database::models::TeamMember>,
        project_organization: &Option<crate::database::models::Organization>,
    ) -> Option<Self> {
        if role.is_admin() {
            return Some(ProjectPermissions::ALL);
        } else if let Some(member) = team_member {
            if let Some(permissions) = member.permissions {
                return Some(permissions);
            } else if let Some(organization) = project_organization {
                return Some(organization.default_project_permissions);
            }
        };
        
         if role.is_mod() {
            Some(ProjectPermissions::EDIT_DETAILS | ProjectPermissions::EDIT_BODY | ProjectPermissions::UPLOAD_VERSION)
        } else {
            None
        }
    }

}

bitflags::bitflags! {
    #[derive(Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct OrganizationPermissions: u64 {
        const EDIT_DETAILS = 1 << 0;
        const EDIT_BODY = 1 << 1;
        const MANAGE_INVITES = 1 << 2;
        const REMOVE_MEMBER = 1 << 3;
        const EDIT_MEMBER = 1 << 4;
        const ADD_PROJECT = 1 << 5;
        const REMOVE_PROJECT = 1 << 6;
        const EDIT_PROJECT_MEMBER = 1 << 7; // Edit permissions of a member in a project owned by the organization TODO
        const ALL = 0b1111111;
        const NONE = 0b0;
    }
}

impl Default for OrganizationPermissions {
    fn default() -> OrganizationPermissions {
        OrganizationPermissions::NONE
    }
}

impl OrganizationPermissions {
    pub fn get_permissions_by_role(
        role: &crate::models::users::Role,
        team_member: &Option<crate::database::models::TeamMember>,
    ) -> Option<Self> {
        if role.is_admin() {
            Some(OrganizationPermissions::ALL)
        } else if let Some(member) = team_member {
            member.organization_permissions
        } else if role.is_mod() {
            Some(OrganizationPermissions::EDIT_DETAILS | OrganizationPermissions::EDIT_BODY | OrganizationPermissions::ADD_PROJECT)
        } else {
            None
        }
    }
}



/// A member of a team
#[derive(Serialize, Deserialize, Clone)]
pub struct TeamMember {
    /// The ID of the team this team member is a member of
    pub team_id: TeamId,
    /// The user associated with the member
    pub user: User,
    /// The role of the user in the team
    pub role: String,
    /// A bitset containing the user's permissions in this team.
    /// In an organization-controlled project, these are the unique overriding permissions for the user's role for any project in the organization, if they exist.
    /// In an organization, these are the meta-permission with regards to the organization.
    pub permissions: Option<Permissions>,
    /// Whether the user has joined the team or is just invited to it
    pub accepted: bool,

    #[serde(with = "rust_decimal::serde::float_option")]
    /// Payouts split. This is a weighted average. For example. if a team has two members with this
    /// value set to 25.0 for both members, they split revenue 50/50
    pub payouts_split: Option<Decimal>,
    /// Ordering of the member in the list
    pub ordering: i64,
}

impl TeamMember {
    pub fn from(
        data: crate::database::models::team_item::TeamMember,
        user: crate::database::models::User,
        override_permissions: bool,
    ) -> Self {
        let user: User = user.into();

        Self {
            team_id: data.team_id.into(),
            user,
            role: data.role,
            permissions: if override_permissions {
                None
            } else {
                if let Some(permissions) = data.permissions {
                    Some(Permissions::Project(permissions))
                } else if let Some(permissions) = data.organization_permissions {
                    Some(Permissions::Organization(permissions))
                } else {
                    None
                }
            },
            accepted: data.accepted,
            payouts_split: if override_permissions {
                None
            } else {
                Some(data.payouts_split)
            },
            ordering: data.ordering,
        }
    }
}
