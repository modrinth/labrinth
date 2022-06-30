use crate::validate::{
    SupportedGameVersions, ValidationError, ValidationResult,
};
use std::io::Cursor;
use zip::ZipArchive;

pub struct JavaPackValidator;

impl super::Validator for JavaPackValidator {
    fn get_file_extensions(&self) -> &[&str] {
        &["zip"]
    }

    fn get_project_types(&self) -> &[&str] {
        &["resourcepack", "datapack"]
    }

    fn get_supported_loaders(&self) -> &[&str] {
        &["java"]
    }

    fn get_supported_game_versions(&self) -> SupportedGameVersions {
        SupportedGameVersions::All
    }

    fn validate(
        &self,
        archive: &mut ZipArchive<Cursor<bytes::Bytes>>,
    ) -> Result<ValidationResult, ValidationError> {
        archive.by_name("pack.mcmeta").map_err(|_| {
            ValidationError::InvalidInput(
                "No pack.mcmeta present for pack file.".into(),
            )
        })?;

        Ok(ValidationResult::Pass)
    }
}

pub struct BedrockPackValidator;

impl super::Validator for BedrockPackValidator {
    fn get_file_extensions(&self) -> &[&str] {
        &["mcpack"]
    }

    fn get_project_types(&self) -> &[&str] {
        &["resourcepack", "datapack"]
    }

    fn get_supported_loaders(&self) -> &[&str] {
        &["bedrock"]
    }

    fn get_supported_game_versions(&self) -> SupportedGameVersions {
        SupportedGameVersions::All
    }

    fn validate(
        &self,
        archive: &mut ZipArchive<Cursor<bytes::Bytes>>,
    ) -> Result<ValidationResult, ValidationError> {
        archive.by_name("manifest.json").map_err(|_| {
            ValidationError::InvalidInput(
                "No manifest.json present for pack file.".into(),
            )
        })?;

        Ok(ValidationResult::Pass)
    }
}
