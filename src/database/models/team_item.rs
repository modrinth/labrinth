use serde::{Deserialize, Serialize};

/// A team of users who control a mod
#[derive(Serialize, Deserialize)]
pub struct Team {
    /// The id of the team
    pub id: i64,
    /// A list of the members of the team
    pub members: Vec<TeamMember>,
}

/// A member of a team
#[derive(Serialize, Deserialize, Clone)]
pub struct TeamMember {
    /// The ID of the user associated with the member
    pub user_id: i64,
    /// The name of the user
    pub name: String,
    pub role: String,
}
