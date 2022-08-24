use crate::database::models::UserId;
use crate::models::settings::FrontendTheme;
use serde::Serialize;

#[derive(Serialize)]
pub struct UserSettings {
    pub tos_agreed: bool,
    pub public_email: bool,
    pub public_github: bool,
    pub theme: FrontendTheme,
}
