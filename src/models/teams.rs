use super::ids::Base62Id;
use crate::models::users::UserId;
use serde::{Deserialize, Serialize};

/// The ID of a team
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct TeamId(pub u64);

pub const OWNER_ROLE: &str = "Owner";

// TODO: permissions, role names, etc
/// A team of users who control a mod
#[derive(Serialize, Deserialize)]
pub struct Team {
    /// The id of the team
    pub id: TeamId,
    /// A list of the members of the team
    pub members: Vec<TeamMember>,
}

bitflags::bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct Permissions: u64 {
        const UPLOAD_VERSION = 0b00000001;
        const DELETE_VERSION = 0b00000010;
        const EDIT_DETAILS = 0b00000100;
        const EDIT_BODY = 0b00001000;
        const MANAGE_INVITES = 0b00010000;
        const REMOVE_MEMBER = 0b00100000;
        const EDIT_MEMBER = 0b01000000;
        const DELETE_MOD = 0b10000000;
        const ALL = 0b11111111;
    }
}

impl Default for Permissions {
    fn default() -> Permissions {
        Permissions::UPLOAD_VERSION | Permissions::DELETE_VERSION
    }
}

/// A member of a team
#[derive(Serialize, Deserialize, Clone)]
pub struct TeamMember {
    /// The ID of the user associated with the member
    pub user_id: UserId,
    /// The name of the user
    pub name: String,
    /// The role of the user in the team
    pub role: String,
    /// A bitflag containing the user's permissions in this team
    pub permissions: Permissions,
}
