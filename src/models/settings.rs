use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter, Result};
use FrontendTheme::*;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum FrontendTheme {
    System,
    Light,
    Dark,
    OLED,
}

impl Display for FrontendTheme {
    fn fmt(&self, fmt: &mut Formatter) -> Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl FrontendTheme {
    pub fn from_str(string: &str) -> FrontendTheme {
        match string {
            "light" => Light,
            "dark" => Dark,
            "oled" => OLED,
            _ => System,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            System => "system",
            Light => "light",
            Dark => "dark",
            OLED => "oled",
        }
    }
}
