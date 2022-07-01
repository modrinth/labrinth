use crate::database::models::categories::ReportType;
use crate::database::models::report_item::Report;
use crate::database::models::{
    generate_report_id, ProjectId, UserId, VersionId,
};
use censor::Censor;
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

pub const CENSOR: Censor = Censor::Standard + Censor::Sex;

pub fn censor_check(
    text: &str,
    project: Option<ProjectId>,
    version: Option<VersionId>,
    user: Option<UserId>,
    report_text: String,
    transaction: &mut Transaction<Postgres>,
) {
    if CENSOR.check(text) {
        let report_type =
            ReportType::get_id("inappropriate", &mut *transaction)
                .await?
                .expect("No database entry for 'inappropriate' report type");
        Report {
            id: generate_report_id(&mut *transaction).await?,
            report_type_id: report_type,
            project_id: project,
            version_id: version,
            user_id: user,
            body: report_text,
            reporter: None,
            created: OffsetDateTime::now_utc(),
        }
        .insert(&mut *transaction)
        .await?;
    }
}
