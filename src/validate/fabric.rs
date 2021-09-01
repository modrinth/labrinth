use crate::validate::{SupportedGameVersions, ValidationError, ValidationResult};
use chrono::{DateTime, NaiveDateTime, Utc};
use std::io::Cursor;
use zip::ZipArchive;

pub struct FabricValidator {}

impl super::Validator for FabricValidator {
    fn get_file_extensions<'a>(&self) -> &'a [&'a str] {
        &["jar", "zip"]
    }

    fn get_project_types<'a>(&self) -> &'a [&'a str] {
        &["mod"]
    }

    fn get_supported_loaders<'a>(&self) -> &'a [&'a str] {
        &["fabric"]
    }

    fn get_supported_game_versions(&self) -> SupportedGameVersions {
        // Time since release of 18w49a, the first fabric version
        SupportedGameVersions::PastDate(DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp(1543969469, 0),
            Utc,
        ))
    }

    fn validate(
        &self,
        archive: &mut ZipArchive<Cursor<&[u8]>>,
    ) -> Result<ValidationResult, ValidationError> {
        archive.by_name("fabric.mod.json").map_err(|_| {
            ValidationError::InvalidInputError(
                "No fabric.mod.json present for Fabric file.".to_string(),
            )
        })?;

        if !archive
            .file_names()
            .any(|name| name.ends_with("refmap.json") || name.ends_with(".class"))
        {
            return Ok(ValidationResult::Warning(
                "Fabric mod file is a source file!".to_string(),
            ));
        }

        //TODO: Check if file is a dev JAR?

        Ok(ValidationResult::Pass)
    }
}
