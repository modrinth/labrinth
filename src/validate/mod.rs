use crate::models::projects::{GameVersion, Loader};
use std::io::Cursor;
use thiserror::Error;
use zip::ZipArchive;

mod pack;

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Unable to read Zip Archive: {0}")]
    ZipError(#[from] zip::result::ZipError),
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Error while validating JSON: {0}")]
    SerDeError(#[from] serde_json::Error),
    #[error("Invalid Input: {0}")]
    InvalidInputError(String),
}

pub enum ValidationResult {
    /// File should be marked as primary
    Pass,
    /// File should not be marked primary, the reason for which is inside the String
    Warning(String),
    /// File should be rejected, as it is in the incorrect format
    Fail,
}

pub trait Validator {
    fn get_file_extensions<'a>() -> Vec<&'a str>;
    fn get_project_types<'a>() -> Vec<&'a str>;
    fn get_supported_loaders() -> Vec<Loader>;
    fn get_supported_game_versions() -> Vec<GameVersion>;
    fn validate(
        archive: &mut ZipArchive<Cursor<&[u8]>>,
    ) -> Result<ValidationResult, ValidationError>;
}

//TODO: way of storing/registering validators

/// The return value is whether this file should be marked as primary or not, based on the analysis of the file
pub fn validate_file(
    data: &[u8],
    file_extension: &str,
    project_type: &str,
    loaders: Vec<Loader>,
    game_versions: Vec<GameVersion>,
) -> Result<ValidationResult, ValidationError> {
    let reader = std::io::Cursor::new(data);
    let mut zip = zip::ZipArchive::new(reader)?;

    //TODO: match inputs to specific validators

    Ok(ValidationResult::Pass)
}

//todo: fabric/forge validators for 1.8+ respectively
