use crate::validate::{SupportedGameVersions, ValidationError, ValidationResult};
use chrono::{DateTime, NaiveDateTime, Utc};
use std::io::{Cursor, Read};
use zip::ZipArchive;

pub struct FabricValidator;

impl super::Validator for FabricValidator {
    fn get_file_extensions(&self) -> &[&str] {
        &["jar", "zip"]
    }

    fn get_project_types(&self) -> &[&str] {
        &["mod"]
    }

    fn get_supported_loaders(&self) -> &[&str] {
        &["fabric"]
    }

    fn get_supported_game_versions(&self) -> SupportedGameVersions {
        // Time since release of 18w49a, the first fabric version
        SupportedGameVersions::PastDate(DateTime::from_utc(
            NaiveDateTime::from_timestamp_opt(1543969469, 0).unwrap(),
            Utc,
        ))
    }

    fn validate(
        &self,
        archive: &mut ZipArchive<Cursor<bytes::Bytes>>,
    ) -> Result<ValidationResult, ValidationError> {
        if archive.by_name("fabric.mod.json").is_err() {
            return Ok(ValidationResult::Warning(
                "No fabric.mod.json present for Fabric file.",
            ));
        }

        let manifest = {
            let mut file = if let Ok(file) = archive.by_name("META-INF/MANIFEST.MF") {
                file
            } else {
                return Ok(ValidationResult::Warning("Java JAR manifest is missing."));
            };

            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            contents
        };

        if let Some(line) = manifest.lines().find(|&l| l.starts_with("Fabric-Jar-Type")) {
            if line == "classes" {
                Ok(ValidationResult::Pass)
            }
        }

        Ok(ValidationResult::Warning("Fabric mod file is not recognized as a 'classes' file"))
    }
}
