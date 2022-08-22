use serde::Serialize;
use crate::models::settings::FrontendTheme;

#[derive(Serialize)]
pub struct UserSettings {
    pub tos_agreed: bool,
    pub public_email: bool,
    pub public_github: bool,
    pub theme: FrontendTheme,
}
