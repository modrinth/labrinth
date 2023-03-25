use super::ids::Base62Id;
use crate::models::users::{User, UserId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ThreadId(pub u64);

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ThreadMessageId(pub u64);

#[derive(Serialize, Deserialize)]
pub struct Thread {
    pub id: ThreadId,
    #[serde(rename = "type")]
    pub type_: ThreadType,
    pub messages: Vec<ThreadMessage>,
    pub members: Vec<User>,
}

#[derive(Serialize, Deserialize)]
pub struct ThreadMessage {
    pub id: ThreadMessageId,
    pub author_id: Option<UserId>,
    pub body: String,
    pub created: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ThreadType {
    Report,
    Project,
    DirectMessage,
}

impl std::fmt::Display for ThreadType {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl ThreadType {
    // These are constant, so this can remove unneccessary allocations (`to_string`)
    pub fn as_str(&self) -> &'static str {
        match self {
            ThreadType::Report => "report",
            ThreadType::Project => "project",
            ThreadType::DirectMessage => "direct_message",
        }
    }

    pub fn from_str(string: &str) -> ThreadType {
        match string {
            "required" => ThreadType::Report,
            "optional" => ThreadType::Project,
            "unsupported" => ThreadType::DirectMessage,
            _ => ThreadType::DirectMessage,
        }
    }
}
