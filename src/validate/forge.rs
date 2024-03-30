use crate::validate::{SupportedGameVersions, ValidationError, ValidationResult};
use chrono::{TimeZone, Utc};
use std::io::Cursor;
use zip::ZipArchive;

pub struct ForgeValidator;

impl super::Validator for ForgeValidator {
    fn get_file_extensions(&self) -> &[&str] {
        &["jar", "zip"]
    }

    fn get_project_types(&self) -> &[&str] {
        &["mod"]
    }

    fn get_supported_loaders(&self) -> &[&str] {
        &["forge"]
    }

    fn get_supported_game_versions(&self) -> SupportedGameVersions {
        // Time since release of 1.13, the first forge version which uses the new TOML system
        SupportedGameVersions::PastDate(Utc.timestamp_opt(1540122067, 0).unwrap())
    }

    fn validate(
        &self,
        _archive: &mut ZipArchive<Cursor<bytes::Bytes>>,
    ) -> Result<ValidationResult, ValidationError> {
        // if archive.by_name("META-INF/mods.toml").is_err() {
        //     return Ok(ValidationResult::Warning(
        //         "No mods.toml present for Forge file.",
        //     ));
        // }

        Ok(ValidationResult::Pass)
    }
}

pub struct LegacyForgeValidator;

impl super::Validator for LegacyForgeValidator {
    fn get_file_extensions(&self) -> &[&str] {
        &["jar", "zip"]
    }

    fn get_project_types(&self) -> &[&str] {
        &["mod"]
    }

    fn get_supported_loaders(&self) -> &[&str] {
        &["forge"]
    }

    fn get_supported_game_versions(&self) -> SupportedGameVersions {
        // Times between versions 1.5.2 to 1.12.2, which all use the legacy way of defining mods
        SupportedGameVersions::Range(
            Utc.timestamp_opt(1366818300, 0).unwrap(),
            Utc.timestamp_opt(1505810340, 0).unwrap(),
        )
    }

    fn validate(
        &self,
        archive: &mut ZipArchive<Cursor<bytes::Bytes>>,
    ) -> Result<ValidationResult, ValidationError> {
        if archive.by_name("mcmod.info").is_err() {
            return Ok(ValidationResult::Warning(
                "Forge mod file does not contain mcmod.info!",
            ));
        };

        //TODO: Check if file is a dev JAR?

        Ok(ValidationResult::Pass)
    }
}
