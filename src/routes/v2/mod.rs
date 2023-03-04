mod admin;
mod auth;
mod midas;
mod moderation;
mod notifications;
pub(crate) mod project_creation;
mod projects;
mod reports;
mod statistics;
mod tags;
mod teams;
mod users;
mod version_creation;
mod version_file;
mod versions;

pub use auth::config as auth_config;
pub use tags::config as tags_config;

pub use super::ApiError;
use actix_web::web;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("v2")
            .configure(auth_config)
            .configure(tags_config)
            .configure(projects_config)
            .configure(versions_config)
            .configure(teams_config)
            .configure(users_config)
            .configure(moderation_config)
            .configure(reports_config)
            .configure(notifications_config)
            .configure(statistics_config)
            .configure(admin_config)
            .configure(midas_config),
    );
}

pub fn projects_config(cfg: &mut web::ServiceConfig) {
    cfg.service(projects::project_search);
    cfg.service(projects::projects_get);
    cfg.service(projects::projects_edit);
    cfg.service(projects::random_projects_get);
    cfg.service(project_creation::project_create);

    cfg.service(
        web::scope("project")
            .service(projects::project_get)
            .service(projects::project_get_check)
            .service(projects::project_delete)
            .service(projects::project_edit)
            .service(projects::project_icon_edit)
            .service(projects::delete_project_icon)
            .service(projects::add_gallery_item)
            .service(projects::edit_gallery_item)
            .service(projects::delete_gallery_item)
            .service(projects::project_follow)
            .service(projects::project_unfollow)
            .service(projects::project_schedule)
            .service(teams::team_members_get_project)
            .service(
                web::scope("{project_id}")
                    .service(versions::version_list)
                    .service(projects::dependency_list)
                    .service(versions::version_project_get),
            ),
    );
}

pub fn versions_config(cfg: &mut web::ServiceConfig) {
    cfg.service(versions::versions_get);
    cfg.service(version_creation::version_create);
    cfg.service(
        web::scope("version")
            .service(versions::version_get)
            .service(versions::version_delete)
            .service(version_creation::upload_file_to_version)
            .service(versions::version_edit)
            .service(versions::version_schedule),
    );
    cfg.service(
        web::scope("version_file")
            .service(version_file::delete_file)
            .service(version_file::get_version_from_hash)
            .service(version_file::download_version)
            .service(version_file::get_update_from_hash),
    );

    cfg.service(
        web::scope("version_files")
            .service(version_file::get_versions_from_hashes)
            .service(version_file::download_files)
            .service(version_file::update_files),
    );
}

pub fn users_config(cfg: &mut web::ServiceConfig) {
    cfg.service(users::user_auth_get);

    cfg.service(users::users_get);
    cfg.service(
        web::scope("user")
            .service(users::user_get)
            .service(users::projects_list)
            .service(users::user_delete)
            .service(users::user_edit)
            .service(users::user_icon_edit)
            .service(users::user_notifications)
            .service(users::user_follows)
            .service(users::user_payouts)
            .service(users::user_payouts_request),
    );
}

pub fn teams_config(cfg: &mut web::ServiceConfig) {
    cfg.service(teams::teams_get);

    cfg.service(
        web::scope("team")
            .service(teams::team_members_get)
            .service(teams::edit_team_member)
            .service(teams::transfer_ownership)
            .service(teams::add_team_member)
            .service(teams::join_team)
            .service(teams::remove_team_member),
    );
}

pub fn notifications_config(cfg: &mut web::ServiceConfig) {
    cfg.service(notifications::notifications_get);
    cfg.service(notifications::notifications_delete);

    cfg.service(
        web::scope("notification")
            .service(notifications::notification_get)
            .service(notifications::notification_delete),
    );
}

pub fn moderation_config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("moderation")
            .service(moderation::get_projects)
            .service(moderation::ban_user)
            .service(moderation::unban_user),
    );
}

pub fn reports_config(cfg: &mut web::ServiceConfig) {
    cfg.service(reports::reports);
    cfg.service(reports::report_create);
    cfg.service(reports::delete_report);
}

pub fn statistics_config(cfg: &mut web::ServiceConfig) {
    cfg.service(statistics::get_stats);
}

pub fn admin_config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("admin")
            .service(admin::count_download)
            .service(admin::process_payout),
    );
}

pub fn midas_config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("midas")
            .service(midas::init_checkout)
            .service(midas::init_customer_portal)
            .service(midas::handle_stripe_webhook),
    );
}
