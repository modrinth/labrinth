use super::ids::Base62Id;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct UserId(pub u64);

pub const DELETED_USER: UserId = UserId(127155982985829);

bitflags::bitflags! {
    #[derive(Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct Badges: u64 {
        const MIDAS = 1 << 0;
        const EARLY_MODPACK_ADOPTER = 1 << 1;
        const EARLY_RESPACK_ADOPTER = 1 << 2;
        const EARLY_PLUGIN_ADOPTER = 1 << 3;
        const ALPHA_TESTER = 1 << 4;
        const CONTRIBUTOR = 1 << 5;
        const TRANSLATOR = 1 << 6;

        const ALL = 0b1111111;
        const NONE = 0b0;
    }
}

impl Default for Badges {
    fn default() -> Badges {
        Badges::NONE
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
    pub id: UserId,
    pub github_id: Option<u64>,
    pub username: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub created: DateTime<Utc>,
    pub role: Role,
    pub badges: Badges,
    pub settings: Option<UserSettings>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserSettings {
    pub public_github: bool,
    pub theme: FrontendTheme,
    pub locale: String,
}

use crate::database::models::user_item::User as DBUser;
impl From<DBUser> for User {
    fn from(data: DBUser) -> Self {
        Self {
            id: data.id.into(),
            github_id: data.github_id.map(|i| i as u64),
            username: data.username,
            name: data.name,
            email: data.email,
            avatar_url: data.avatar_url,
            bio: data.bio,
            created: data.created,
            role: Role::from_string(&*data.role),
            badges: data.badges,
            settings: data.settings,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Developer,
    Moderator,
    Admin,
}

impl std::fmt::Display for Role {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.write_str(self.as_str())
    }
}

impl Role {
    pub fn from_string(string: &str) -> Role {
        match string {
            "admin" => Role::Admin,
            "moderator" => Role::Moderator,
            _ => Role::Developer,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Developer => "developer",
            Role::Moderator => "moderator",
            Role::Admin => "admin",
        }
    }

    pub fn is_mod(&self) -> bool {
        match self {
            Role::Developer => false,
            Role::Moderator | Role::Admin => true,
        }
    }

    pub fn is_admin(&self) -> bool {
        match self {
            Role::Developer | Role::Moderator => false,
            Role::Admin => true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FrontendTheme {
    System,
    Light,
    Dark,
    Oled,
}

impl std::fmt::Display for FrontendTheme {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

use FrontendTheme::*;
impl FrontendTheme {
    pub fn from_str(string: &str) -> FrontendTheme {
        match string {
            "light" => Light,
            "dark" => Dark,
            "oled" => Oled,
            _ => System,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            System => "system",
            Light => "light",
            Dark => "dark",
            Oled => "oled",
        }
    }
}
