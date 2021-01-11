pub mod format;
use thiserror::Error;
use std::io::Read;

#[derive(Error, Debug)]
pub enum PackValidationError {
    #[error("Unable to read Zip Archive: {0}")]
    ZipError(#[from] zip::result::ZipError),
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Error while reading pack JSON: {0}")]
    SerDeError(#[from] serde_json::Error),
    #[error("Invalid Input: {0}")]
    InvalidInputError(String),
}

pub fn validate_format(buffer: &[u8]) -> Result<format::PackFormat, PackValidationError> {
    let reader = std::io::Cursor::new(buffer);
    let mut zip = zip::ZipArchive::new(reader)?;

    let mut file = zip.by_name("index.json")?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let pack : format::PackFormat = serde_json::from_str(&*contents)?;

    // TODO: Implement games
    if pack.game != "minecraft".to_string() {
        return Err(PackValidationError::InvalidInputError(format!("Game {0} does not exist!", pack.game)))
    }

    Ok(pack)
}
