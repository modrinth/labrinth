use crate::validate::{SupportedGameVersions, ValidationError, ValidationResult};
use std::io::Cursor;
use zip::ZipArchive;

pub struct BukkitValidator;

impl super::Validator for BukkitValidator {
    fn get_file_extensions(&self) -> &[&str] {
        &["jar"]
    }

    fn get_project_types(&self) -> &[&str] {
        &["plugin"]
    }

    fn get_supported_loaders(&self) -> &[&str] {
        &["bukkit", "spigot", "paper", "purpur"]
    }

    fn get_supported_game_versions(&self) -> SupportedGameVersions {
        SupportedGameVersions::All
    }

    fn validate(
        &self,
        archive: &mut ZipArchive<Cursor<bytes::Bytes>>,
    ) -> Result<ValidationResult, ValidationError> {
        // TODO: Add schema validation in order to make sure that it won't error out
        archive.by_name("plugin.yml").map_err(|_| {
            ValidationError::InvalidInputError(
                "No plugin.yml file is present in your file.".into(),
            )
        })?;

        Ok(ValidationResult::Pass)
    }
}
