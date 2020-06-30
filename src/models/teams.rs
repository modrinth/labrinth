use super::ids::*;
use serde::{Deserialize, Serialize};

// TODO: permissions, role names, etc
#[derive(Serialize, Deserialize)]
pub struct Team {
    id: TeamId,
    members: Vec<TeamMember>,
}

#[derive(Serialize, Deserialize)]
pub struct TeamMember {
    user_id: UserId,
    name: String,
}
