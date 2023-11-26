use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::{
    ids::{NotificationId, UserId},
    notifications::{Notification, NotificationAction, NotificationBody},
};

#[derive(Serialize, Deserialize)]
pub struct LegacyNotification {
    pub id: NotificationId,
    pub user_id: UserId,
    pub read: bool,
    pub created: DateTime<Utc>,
    pub body: NotificationBody,

    // DEPRECATED: use body field instead
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub title: String,
    pub text: String,
    pub link: String,
    pub actions: Vec<LegacyNotificationAction>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LegacyNotificationAction {
    pub title: String,
    /// The route to call when this notification action is called. Formatted HTTP Method, route
    pub action_route: (String, String),
}

impl LegacyNotification {
    pub fn from(notification: Notification) -> Self {
        Self {
            id: notification.id,
            user_id: notification.user_id,
            read: notification.read,
            created: notification.created,
            body: notification.body,
            type_: notification.type_,
            title: notification.name,
            text: notification.text,
            link: notification.link,
            actions: notification
                .actions
                .into_iter()
                .map(LegacyNotificationAction::from)
                .collect(),
        }
    }
}

impl LegacyNotificationAction {
    pub fn from(notification_action: NotificationAction) -> Self {
        Self {
            title: notification_action.name,
            action_route: notification_action.action_route,
        }
    }
}
