use super::ids::*;
use serde::{Deserialize, Serialize};

// TODO: permissions, role names, etc
/// A team of users who control a mod
#[derive(Serialize, Deserialize)]
pub struct Team {
    /// The id of the team
    pub id: TeamId,
    /// A list of the members of the team
    pub members: Vec<TeamMember>,
}

/// A member of a team
#[derive(Serialize, Deserialize)]
pub struct TeamMember {
    /// The ID of the user associated with the member
    pub user_id: UserId,
    /// The name of the user
    pub name: String,
}
