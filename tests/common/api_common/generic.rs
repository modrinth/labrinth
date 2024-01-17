use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use axum_test::TestResponse;
use labrinth::models::{
    projects::{ProjectId, VersionType},
    teams::{OrganizationPermissions, ProjectPermissions},
};

use crate::common::{/*api_v2::ApiV2,*/ api_v3::ApiV3, dummy_data::TestFile};

use super::{
    models::{CommonProject, CommonVersion},
    request_data::{ImageData, ProjectCreationRequestData},
    Api, ApiProject, ApiTags, ApiTeams, ApiUser, ApiVersion,
};

#[derive(Clone)]
pub enum GenericApi {
    // V2(ApiV2),
    V3(ApiV3),
}

macro_rules! delegate_api_variant {
    (
        $(#[$meta:meta])*
        impl $impl_name:ident for $struct_name:ident {
            $(
                [$method_name:ident, $ret:ty, $($param_name:ident: $param_type:ty),*]
            ),* $(,)?
        }

    ) => {
        $(#[$meta])*
        impl $impl_name for $struct_name {
            $(
                async fn $method_name(&self, $($param_name: $param_type),*) -> $ret {
                    match self {
                        //$struct_name::V2(api) => api.$method_name($($param_name),*).await,
                        $struct_name::V3(api) => api.$method_name($($param_name),*).await,
                    }
                }
            )*
        }
    };
}

#[async_trait(?Send)]
impl Api for GenericApi {
    async fn reset_search_index(&self) -> TestResponse {
        match self {
            // Self::V2(api) => api.reset_search_index().await,
            Self::V3(api) => api.reset_search_index().await,
        }
    }

    fn get_test_server(&self) -> Arc<axum_test::TestServer> {
        match self {
            // Self::V2(api) => api.get_test_server(),
            Self::V3(api) => api.get_test_server(),
        }
    }
}

delegate_api_variant!(
    #[async_trait(?Send)]
    impl ApiProject for GenericApi {
        [add_public_project, (CommonProject, Vec<CommonVersion>), slug: &str, version_jar: Option<TestFile>, modify_json: Option<json_patch::Patch>, pat: Option<&str>],
        [get_public_project_creation_data_json, serde_json::Value, slug: &str, version_jar: Option<&TestFile>],
        [create_project, TestResponse, creation_data: ProjectCreationRequestData, pat: Option<&str>],
        [remove_project, TestResponse, project_slug_or_id: &str, pat: Option<&str>],
        [get_project, TestResponse, id_or_slug: &str, pat: Option<&str>],
        [get_project_deserialized_common, CommonProject, id_or_slug: &str, pat: Option<&str>],
        [get_projects, TestResponse, ids_or_slugs: &[&str], pat: Option<&str>],
        [get_project_dependencies, TestResponse, id_or_slug: &str, pat: Option<&str>],
        [get_user_projects, TestResponse, user_id_or_username: &str, pat: Option<&str>],
        [get_user_projects_deserialized_common, Vec<CommonProject>, user_id_or_username: &str, pat: Option<&str>],
        [edit_project, TestResponse, id_or_slug: &str, patch: serde_json::Value, pat: Option<&str>],
        [edit_project_bulk, TestResponse, ids_or_slugs: &[&str], patch: serde_json::Value, pat: Option<&str>],
        [edit_project_icon, TestResponse, id_or_slug: &str, icon: Option<ImageData>, pat: Option<&str>],
        [add_gallery_item, TestResponse, id_or_slug: &str, image: ImageData,  featured: bool, title: Option<String>, description: Option<String>, ordering: Option<i32>, pat: Option<&str>],
        [remove_gallery_item, TestResponse, id_or_slug: &str, image_url: &str, pat: Option<&str>],
        [edit_gallery_item, TestResponse, id_or_slug: &str, image_url: &str, patch: HashMap<String, String>, pat: Option<&str>],
        [create_report, TestResponse, report_type: &str, id: &str, item_type: crate::common::api_common::models::CommonItemType, body: &str, pat: Option<&str>],
        [get_report, TestResponse, id: &str, pat: Option<&str>],
        [get_reports, TestResponse, ids: &[&str], pat: Option<&str>],
        [get_user_reports, TestResponse, pat: Option<&str>],
        [edit_report, TestResponse, id: &str, patch: serde_json::Value, pat: Option<&str>],
        [delete_report, TestResponse, id: &str, pat: Option<&str>],
        [get_thread, TestResponse, id: &str, pat: Option<&str>],
        [get_threads, TestResponse, ids: &[&str], pat: Option<&str>],
        [write_to_thread, TestResponse, id: &str, r#type : &str, message: &str, pat: Option<&str>],
        [get_moderation_inbox, TestResponse, pat: Option<&str>],
        [read_thread, TestResponse, id: &str, pat: Option<&str>],
        [delete_thread_message, TestResponse, id: &str, pat: Option<&str>],
    }
);

delegate_api_variant!(
    #[async_trait(?Send)]
    impl ApiTags for GenericApi {
        [get_loaders, TestResponse,],
        [get_loaders_deserialized_common, Vec<crate::common::api_common::models::CommonLoaderData>,],
        [get_categories, TestResponse,],
        [get_categories_deserialized_common, Vec<crate::common::api_common::models::CommonCategoryData>,],
    }
);

delegate_api_variant!(
    #[async_trait(?Send)]
    impl ApiTeams for GenericApi {
        [get_team_members, TestResponse, team_id: &str, pat: Option<&str>],
        [get_team_members_deserialized_common, Vec<crate::common::api_common::models::CommonTeamMember>, team_id: &str, pat: Option<&str>],
        [get_teams_members, TestResponse, ids: &[&str], pat: Option<&str>],
        [get_project_members, TestResponse, id_or_slug: &str, pat: Option<&str>],
        [get_project_members_deserialized_common, Vec<crate::common::api_common::models::CommonTeamMember>, id_or_slug: &str, pat: Option<&str>],
        [get_organization_members, TestResponse, id_or_title: &str, pat: Option<&str>],
        [get_organization_members_deserialized_common, Vec<crate::common::api_common::models::CommonTeamMember>, id_or_title: &str, pat: Option<&str>],
        [join_team, TestResponse, team_id: &str, pat: Option<&str>],
        [remove_from_team, TestResponse, team_id: &str, user_id: &str, pat: Option<&str>],
        [edit_team_member, TestResponse, team_id: &str, user_id: &str, patch: serde_json::Value, pat: Option<&str>],
        [transfer_team_ownership, TestResponse, team_id: &str, user_id: &str, pat: Option<&str>],
        [get_user_notifications, TestResponse, user_id: &str, pat: Option<&str>],
        [get_user_notifications_deserialized_common, Vec<crate::common::api_common::models::CommonNotification>, user_id: &str, pat: Option<&str>],
        [get_notification, TestResponse, notification_id: &str, pat: Option<&str>],
        [get_notifications, TestResponse, ids: &[&str], pat: Option<&str>],
        [mark_notification_read, TestResponse, notification_id: &str, pat: Option<&str>],
        [mark_notifications_read, TestResponse, ids: &[&str], pat: Option<&str>],
        [add_user_to_team, TestResponse, team_id: &str, user_id: &str, project_permissions: Option<ProjectPermissions>, organization_permissions: Option<OrganizationPermissions>, pat: Option<&str>],
        [delete_notification, TestResponse, notification_id: &str, pat: Option<&str>],
        [delete_notifications, TestResponse, ids: &[&str], pat: Option<&str>],
    }
);

delegate_api_variant!(
    #[async_trait(?Send)]
    impl ApiUser for GenericApi {
        [get_user, TestResponse, id_or_username: &str, pat: Option<&str>],
        [get_current_user, TestResponse, pat: Option<&str>],
        [edit_user, TestResponse, id_or_username: &str, patch: serde_json::Value, pat: Option<&str>],
        [delete_user, TestResponse, id_or_username: &str, pat: Option<&str>],
    }
);

delegate_api_variant!(
    #[async_trait(?Send)]
    impl ApiVersion for GenericApi {
        [add_public_version, TestResponse, project_id: ProjectId, version_number: &str, version_jar: TestFile, ordering: Option<i32>, modify_json: Option<json_patch::Patch>, pat: Option<&str>],
        [add_public_version_deserialized_common, CommonVersion, project_id: ProjectId, version_number: &str, version_jar: TestFile, ordering: Option<i32>, modify_json: Option<json_patch::Patch>, pat: Option<&str>],
        [get_version, TestResponse, id_or_slug: &str, pat: Option<&str>],
        [get_version_deserialized_common, CommonVersion, id_or_slug: &str, pat: Option<&str>],
        [get_versions, TestResponse, ids_or_slugs: Vec<String>, pat: Option<&str>],
        [get_versions_deserialized_common, Vec<CommonVersion>, ids_or_slugs: Vec<String>, pat: Option<&str>],
        [download_version_redirect, TestResponse, hash: &str, algorithm: &str, pat: Option<&str>],
        [edit_version, TestResponse, id_or_slug: &str, patch: serde_json::Value, pat: Option<&str>],
        [get_version_from_hash, TestResponse, id_or_slug: &str, hash: &str, pat: Option<&str>],
        [get_version_from_hash_deserialized_common, CommonVersion, id_or_slug: &str, hash: &str, pat: Option<&str>],
        [get_versions_from_hashes, TestResponse, hashes: &[&str], algorithm: &str, pat: Option<&str>],
        [get_versions_from_hashes_deserialized_common, HashMap<String, CommonVersion>, hashes: &[&str],        algorithm: &str,        pat: Option<&str>],
        [get_update_from_hash, TestResponse, hash: &str, algorithm: &str, loaders: Option<Vec<String>>,game_versions: Option<Vec<String>>, version_types: Option<Vec<String>>, pat: Option<&str>],
        [get_update_from_hash_deserialized_common, CommonVersion, hash: &str,        algorithm: &str,loaders: Option<Vec<String>>,game_versions: Option<Vec<String>>,version_types: Option<Vec<String>>,        pat: Option<&str>],
        [update_files, TestResponse, algorithm: &str,        hashes: Vec<String>,        loaders: Option<Vec<String>>,        game_versions: Option<Vec<String>>,        version_types: Option<Vec<String>>,        pat: Option<&str>],
        [update_files_deserialized_common, HashMap<String, CommonVersion>, algorithm: &str,        hashes: Vec<String>,        loaders: Option<Vec<String>>,        game_versions: Option<Vec<String>>,        version_types: Option<Vec<String>>,        pat: Option<&str>],
        [get_project_versions, TestResponse, project_id_slug: &str,        game_versions: Option<Vec<String>>,loaders: Option<Vec<String>>,featured: Option<bool>,        version_type: Option<VersionType>,        limit: Option<usize>,        offset: Option<usize>,pat: Option<&str>],
        [get_project_versions_deserialized_common, Vec<CommonVersion>, project_id_slug: &str, game_versions: Option<Vec<String>>, loaders: Option<Vec<String>>,featured: Option<bool>,version_type: Option<VersionType>,limit: Option<usize>,offset: Option<usize>,pat: Option<&str>],
        [edit_version_ordering, TestResponse, version_id: &str,ordering: Option<i32>,pat: Option<&str>],
        [upload_file_to_version, TestResponse, version_id: &str, file: &TestFile, pat: Option<&str>],
        [remove_version, TestResponse, version_id: &str, pat: Option<&str>],
        [remove_version_file, TestResponse, hash: &str, pat: Option<&str>],
    }
);
