#![allow(dead_code)]
use actix_web::test::{self, TestRequest};
use itertools::Itertools;
use labrinth::models::teams::{OrganizationPermissions, ProjectPermissions};
use serde_json::json;

use crate::common::{database::{generate_random_name, ADMIN_USER_PAT}, api_common::ApiTeams};

use super::{
    api_v3::ApiV3,
    database::{ENEMY_USER_PAT, USER_USER_ID, USER_USER_PAT},
    environment::TestEnvironment, api_common::{Api, ApiProject},
};

// A reusable test type that works for any permissions test testing an endpoint that:
// - returns a known 'expected_failure_code' if the scope is not present (defaults to 401)
// - returns a 200-299 if the scope is present
// - returns failure and success JSON bodies for requests that are 200 (for performing non-simple follow-up tests on)
// This uses a builder format, so you can chain methods to set the parameters to non-defaults (most will probably be not need to be set).
pub struct PermissionsTest<'a, A : Api> {
    test_env: &'a TestEnvironment<A>,
    // Permissions expected to fail on this test. By default, this is all permissions except the success permissions.
    // (To ensure we have isolated the permissions we are testing)
    failure_project_permissions: Option<ProjectPermissions>,
    failure_organization_permissions: Option<OrganizationPermissions>,

    // User ID to use for the test user, and their PAT
    user_id: &'a str,
    user_pat: &'a str,

    // Whether or not the user ID should be removed from the project/organization team after the test
    // (This is mostly reelvant if you are also using an existing project/organization, and want to do
    // multiple tests with the same user.
    remove_user: bool,

    //  ID to use for the test project (project, organization)
    // By default, create a new project or organization to test upon.
    // However, if we want, we can use an existing project or organization.
    // (eg: if we want to test a specific project, or a project with a specific state)
    project_id: Option<String>,
    project_team_id: Option<String>,
    organization_id: Option<String>,
    organization_team_id: Option<String>,

    // The codes that is allow to be returned if the scope is not present.
    // (for instance, we might expect a 401, but not a 400)
    allowed_failure_codes: Vec<u16>,
}

pub struct PermissionsTestContext<'a> {
    // pub test_env: &'a TestEnvironment<A>,
    pub user_id: &'a str,
    pub user_pat: &'a str,
    pub project_id: Option<&'a str>,
    pub team_id: Option<&'a str>,
    pub organization_id: Option<&'a str>,
    pub organization_team_id: Option<&'a str>,
}

impl<'a, A : Api> PermissionsTest<'a, A> {
    pub fn new(test_env: &'a TestEnvironment<A>) -> Self {
        Self {
            test_env,
            failure_project_permissions: None,
            failure_organization_permissions: None,
            user_id: USER_USER_ID,
            user_pat: USER_USER_PAT,
            remove_user: false,
            project_id: None,
            organization_id: None,
            project_team_id: None,
            organization_team_id: None,
            allowed_failure_codes: vec![401, 404],
        }
    }

    // Set non-standard failure permissions
    // If not set, it will be set to all permissions except the success permissions
    // (eg: if a combination of permissions is needed, but you want to make sure that the endpoint does not work with all-but-one of them)
    pub fn with_failure_permissions(
        mut self,
        failure_project_permissions: Option<ProjectPermissions>,
        failure_organization_permissions: Option<OrganizationPermissions>,
    ) -> Self {
        self.failure_project_permissions = failure_project_permissions;
        self.failure_organization_permissions = failure_organization_permissions;
        self
    }

    // Set the user ID to use
    // (eg: a moderator, or friend)
    // remove_user: Whether or not the user ID should be removed from the project/organization team after the test
    pub fn with_user(mut self, user_id: &'a str, user_pat: &'a str, remove_user: bool) -> Self {
        self.user_id = user_id;
        self.user_pat = user_pat;
        self.remove_user = remove_user;
        self
    }

    // If a non-standard code is expected.
    // (eg: perhaps 200 for a resource with hidden values deeper in)
    pub fn with_failure_codes(
        mut self,
        allowed_failure_codes: impl IntoIterator<Item = u16>,
    ) -> Self {
        self.allowed_failure_codes = allowed_failure_codes.into_iter().collect();
        self
    }

    // If an existing project or organization is intended to be used
    // We will not create a new project, and will use the given project ID
    // (But will still add the user to the project's team)
    pub fn with_existing_project(mut self, project_id: &str, team_id: &str) -> Self {
        self.project_id = Some(project_id.to_string());
        self.project_team_id = Some(team_id.to_string());
        self
    }
    pub fn with_existing_organization(mut self, organization_id: &str, team_id: &str) -> Self {
        self.organization_id = Some(organization_id.to_string());
        self.organization_team_id = Some(team_id.to_string());
        self
    }

    pub async fn simple_project_permissions_test<T>(
        &self,
        success_permissions: ProjectPermissions,
        req_gen: T,
    ) -> Result<(), String>
    where
        T: Fn(&PermissionsTestContext) -> TestRequest,
    {
        let test_env = self.test_env;
        let failure_project_permissions = self
            .failure_project_permissions
            .unwrap_or(ProjectPermissions::all() ^ success_permissions);
        let test_context = PermissionsTestContext {
            user_id: self.user_id,
            user_pat: self.user_pat,
            project_id: None,
            team_id: None,
            organization_id: None,
            organization_team_id: None,
        };

        let (project_id, team_id) = if self.project_id.is_some() && self.project_team_id.is_some() {
            (
                self.project_id.clone().unwrap(),
                self.project_team_id.clone().unwrap(),
            )
        } else {
            create_dummy_project(&test_env.setup_api).await
        };

        add_user_to_team(
            self.user_id,
            self.user_pat,
            &team_id,
            Some(failure_project_permissions),
            None,
            &test_env.setup_api,
        )
        .await;

        // Failure test- not logged in
        let request = req_gen(&PermissionsTestContext {
            project_id: Some(&project_id),
            team_id: Some(&team_id),
            ..test_context
        })
        .to_request();

        let resp = test_env.call(request).await;
        if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
            return Err(format!(
                "Failure permissions test failed. Expected failure codes {} got {}",
                self.allowed_failure_codes
                    .iter()
                    .map(|code| code.to_string())
                    .join(","),
                resp.status().as_u16()
            ));
        }

        // Failure test- logged in on a non-team user
        let request = req_gen(&PermissionsTestContext {
            project_id: Some(&project_id),
            team_id: Some(&team_id),
            ..test_context
        })
        .append_header(("Authorization", ENEMY_USER_PAT))
        .to_request();

        let resp = test_env.call(request).await;
        if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
            return Err(format!(
                "Failure permissions test failed. Expected failure codes {} got {}",
                self.allowed_failure_codes
                    .iter()
                    .map(|code| code.to_string())
                    .join(","),
                resp.status().as_u16()
            ));
        }

        // Failure test- logged in with EVERY non-relevant permission
        let request = req_gen(&PermissionsTestContext {
            project_id: Some(&project_id),
            team_id: Some(&team_id),
            ..test_context
        })
        .append_header(("Authorization", self.user_pat))
        .to_request();

        let resp = test_env.call(request).await;
        if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
            return Err(format!(
                "Failure permissions test failed. Expected failure codes {} got {}",
                self.allowed_failure_codes
                    .iter()
                    .map(|code| code.to_string())
                    .join(","),
                resp.status().as_u16()
            ));
        }

        // Patch user's permissions to success permissions
        modify_user_team_permissions(
            self.user_id,
            &team_id,
            Some(success_permissions),
            None,
            &test_env.setup_api,
        )
        .await;

        // Successful test
        let request = req_gen(&PermissionsTestContext {
            project_id: Some(&project_id),
            team_id: Some(&team_id),
            ..test_context
        })
        .append_header(("Authorization", self.user_pat))
        .to_request();

        let resp = test_env.call(request).await;
        if !resp.status().is_success() {
            return Err(format!(
                "Success permissions test failed. Expected success, got {}",
                resp.status().as_u16()
            ));
        }

        // If the remove_user flag is set, remove the user from the project
        // Relevant for existing projects/users
        if self.remove_user {
            remove_user_from_team(self.user_id, &team_id, &test_env.setup_api).await;
        }
        Ok(())
    }

    pub async fn simple_organization_permissions_test<T>(
        &self,
        success_permissions: OrganizationPermissions,
        req_gen: T,
    ) -> Result<(), String>
    where
        T: Fn(&PermissionsTestContext) -> TestRequest,
    {
        let test_env = self.test_env;
        let failure_organization_permissions = self
            .failure_organization_permissions
            .unwrap_or(OrganizationPermissions::all() ^ success_permissions);
        let test_context = PermissionsTestContext {
            user_id: self.user_id,
            user_pat: self.user_pat,
            project_id: None,
            team_id: None,
            organization_id: None,
            organization_team_id: None,
        };

        let (organization_id, team_id) =
            if self.organization_id.is_some() && self.organization_team_id.is_some() {
                (
                    self.organization_id.clone().unwrap(),
                    self.organization_team_id.clone().unwrap(),
                )
            } else {
                create_dummy_org(&test_env.setup_api).await
            };

        add_user_to_team(
            self.user_id,
            self.user_pat,
            &team_id,
            None,
            Some(failure_organization_permissions),
            &test_env.setup_api,
        )
        .await;

        // Failure test
        let request = req_gen(&PermissionsTestContext {
            organization_id: Some(&organization_id),
            team_id: Some(&team_id),
            ..test_context
        })
        .append_header(("Authorization", self.user_pat))
        .to_request();

        let resp = test_env.call(request).await;
        if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
            return Err(format!(
                "Failure permissions test failed. Expected failure codes {} got {}",
                self.allowed_failure_codes
                    .iter()
                    .map(|code| code.to_string())
                    .join(","),
                resp.status().as_u16()
            ));
        }

        // Patch user's permissions to success permissions
        modify_user_team_permissions(
            self.user_id,
            &team_id,
            None,
            Some(success_permissions),
            &test_env.setup_api,
        )
        .await;

        // Successful test
        let request = req_gen(&PermissionsTestContext {
            organization_id: Some(&organization_id),
            team_id: Some(&team_id),
            ..test_context
        })
        .append_header(("Authorization", self.user_pat))
        .to_request();

        let resp = test_env.call(request).await;
        if !resp.status().is_success() {
            return Err(format!(
                "Success permissions test failed. Expected success, got {}",
                resp.status().as_u16()
            ));
        }

        // If the remove_user flag is set, remove the user from the organization
        // Relevant for existing projects/users
        if self.remove_user {
            remove_user_from_team(self.user_id, &team_id, &test_env.setup_api).await;
        }
        Ok(())
    }

    pub async fn full_project_permissions_test<T>(
        &self,
        success_permissions: ProjectPermissions,
        req_gen: T,
    ) -> Result<(), String>
    where
        T: Fn(&PermissionsTestContext) -> TestRequest,
    {
        let test_env = self.test_env;
        let failure_project_permissions = self
            .failure_project_permissions
            .unwrap_or(ProjectPermissions::all() ^ success_permissions);
        let test_context = PermissionsTestContext {
            user_id: self.user_id,
            user_pat: self.user_pat,
            project_id: None,
            team_id: None,
            organization_id: None,
            organization_team_id: None,
        };

        // TEST 1: User not logged in - no PAT.
        // This should always fail, regardless of permissions
        // (As we are testing permissions-based failures)
        let test_1 = async {
            let (project_id, team_id) = create_dummy_project(&test_env.setup_api).await;

            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                return Err(format!(
                    "Test 1 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes
                        .iter()
                        .map(|code| code.to_string())
                        .join(","),
                    resp.status().as_u16()
                ));
            }

            let p =
                get_project_permissions(self.user_id, self.user_pat, &project_id, &test_env.setup_api).await;
            if p != ProjectPermissions::empty() {
                return Err(format!(
                    "Test 1 failed. Expected no permissions, got {:?}",
                    p
                ));
            }

            Ok(())
        };

        // TEST 2: Failure
        // Random user, unaffiliated with the project, with no permissions
        let test_2 = async {
            let (project_id, team_id) = create_dummy_project(&test_env.setup_api).await;

            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                return Err(format!(
                    "Test 2 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes
                        .iter()
                        .map(|code| code.to_string())
                        .join(","),
                    resp.status().as_u16()
                ));
            }

            let p =
                get_project_permissions(self.user_id, self.user_pat, &project_id, &test_env.setup_api).await;
            if p != ProjectPermissions::empty() {
                return Err(format!(
                    "Test 2 failed. Expected no permissions, got {:?}",
                    p
                ));
            }

            Ok(())
        };

        // TEST 3: Failure
        // User affiliated with the project, with failure permissions
        let test_3 = async {
            let (project_id, team_id) = create_dummy_project(&test_env.setup_api).await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &team_id,
                Some(failure_project_permissions),
                None,
                &test_env.setup_api,
            )
            .await;

            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();

            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                return Err(format!(
                    "Test 3 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes
                        .iter()
                        .map(|code| code.to_string())
                        .join(","),
                    resp.status().as_u16()
                ));
            }

            let p =
                get_project_permissions(self.user_id, self.user_pat, &project_id, &test_env.setup_api).await;
            if p != failure_project_permissions {
                return Err(format!(
                    "Test 3 failed. Expected {:?}, got {:?}",
                    failure_project_permissions, p
                ));
            }

            Ok(())
        };

        // TEST 4: Success
        // User affiliated with the project, with the given permissions
        let test_4 = async {
            let (project_id, team_id) = create_dummy_project(&test_env.setup_api).await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &team_id,
                Some(success_permissions),
                None,
                &test_env.setup_api,
            )
            .await;

            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();

            let resp = test_env.call(request).await;
            if !resp.status().is_success() {
                return Err(format!(
                    "Test 4 failed. Expected success, got {}",
                    resp.status().as_u16()
                ));
            }

            let p =
                get_project_permissions(self.user_id, self.user_pat, &project_id, &test_env.setup_api).await;
            if p != success_permissions {
                return Err(format!(
                    "Test 4 failed. Expected {:?}, got {:?}",
                    success_permissions, p
                ));
            }

            Ok(())
        };

        // TEST 5: Failure
        // Project has an organization
        // User affiliated with the project's org, with default failure permissions
        let test_5 = async {
            let (project_id, team_id) = create_dummy_project(&test_env.setup_api).await;
            let (organization_id, organization_team_id) = create_dummy_org(&test_env.setup_api).await;
            add_project_to_org(&test_env.setup_api, &project_id, &organization_id).await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &organization_team_id,
                Some(failure_project_permissions),
                None,
                &test_env.setup_api,
            )
            .await;

            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();

            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                return Err(format!(
                    "Test 5 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes
                        .iter()
                        .map(|code| code.to_string())
                        .join(","),
                    resp.status().as_u16()
                ));
            }

            let p =
                get_project_permissions(self.user_id, self.user_pat, &project_id, &test_env.setup_api).await;
            if p != failure_project_permissions {
                return Err(format!(
                    "Test 5 failed. Expected {:?}, got {:?}",
                    failure_project_permissions, p
                ));
            }

            Ok(())
        };

        // TEST 6: Success
        // Project has an organization
        // User affiliated with the project's org, with the default success
        let test_6 = async {
            let (project_id, team_id) = create_dummy_project(&test_env.setup_api).await;
            let (organization_id, organization_team_id) = create_dummy_org(&test_env.setup_api).await;
            add_project_to_org(&test_env.setup_api, &project_id, &organization_id).await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &organization_team_id,
                Some(success_permissions),
                None,
                &test_env.setup_api,
            )
            .await;

            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();

            let resp = test_env.call(request).await;
            if !resp.status().is_success() {
                return Err(format!(
                    "Test 6 failed. Expected success, got {}",
                    resp.status().as_u16()
                ));
            }

            let p =
                get_project_permissions(self.user_id, self.user_pat, &project_id, &test_env.setup_api).await;
            if p != success_permissions {
                return Err(format!(
                    "Test 6 failed. Expected {:?}, got {:?}",
                    success_permissions, p
                ));
            }

            Ok(())
        };

        // TEST 7: Failure
        // Project has an organization
        // User affiliated with the project's org (even can have successful permissions!)
        // User overwritten on the project team with failure permissions
        let test_7 = async {
            let (project_id, team_id) = create_dummy_project(&test_env.setup_api).await;
            let (organization_id, organization_team_id) = create_dummy_org(&test_env.setup_api).await;
            add_project_to_org(&test_env.setup_api, &project_id, &organization_id).await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &organization_team_id,
                Some(success_permissions),
                None,
                &test_env.setup_api,
            )
            .await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &team_id,
                Some(failure_project_permissions),
                None,
                &test_env.setup_api,
            )
            .await;

            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();

            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                return Err(format!(
                    "Test 7 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes
                        .iter()
                        .map(|code| code.to_string())
                        .join(","),
                    resp.status().as_u16()
                ));
            }

            let p =
                get_project_permissions(self.user_id, self.user_pat, &project_id, &test_env.setup_api).await;
            if p != failure_project_permissions {
                return Err(format!(
                    "Test 7 failed. Expected {:?}, got {:?}",
                    failure_project_permissions, p
                ));
            }

            Ok(())
        };

        // TEST 8: Success
        // Project has an organization
        // User affiliated with the project's org with default failure permissions
        // User overwritten to the project with the success permissions
        let test_8 = async {
            let (project_id, team_id) = create_dummy_project(&test_env.setup_api).await;
            let (organization_id, organization_team_id) = create_dummy_org(&test_env.setup_api).await;
            add_project_to_org(&test_env.setup_api, &project_id, &organization_id).await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &organization_team_id,
                Some(failure_project_permissions),
                None,
                &test_env.setup_api,
            )
            .await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &team_id,
                Some(success_permissions),
                None,
                &test_env.setup_api,
            )
            .await;

            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();

            let resp = test_env.call(request).await;

            if !resp.status().is_success() {
                return Err(format!(
                    "Test 8 failed. Expected success, got {}",
                    resp.status().as_u16()
                ));
            }

            let p =
                get_project_permissions(self.user_id, self.user_pat, &project_id, &test_env.setup_api).await;
            if p != success_permissions {
                return Err(format!(
                    "Test 8 failed. Expected {:?}, got {:?}",
                    success_permissions, p
                ));
            }

            Ok(())
        };

        tokio::try_join!(test_1, test_2, test_3, test_4, test_5, test_6, test_7, test_8)
            .map_err(|e| e)?;

        Ok(())
    }

    pub async fn full_organization_permissions_tests<T>(
        &self,
        success_permissions: OrganizationPermissions,
        req_gen: T,
    ) -> Result<(), String>
    where
        T: Fn(&PermissionsTestContext) -> TestRequest,
    {
        let test_env = self.test_env;
        let failure_organization_permissions = self
            .failure_organization_permissions
            .unwrap_or(OrganizationPermissions::all() ^ success_permissions);
        let test_context = PermissionsTestContext {
            user_id: self.user_id,
            user_pat: self.user_pat,
            project_id: None, // Will be overwritten on each test
            team_id: None,    // Will be overwritten on each test
            organization_id: None,
            organization_team_id: None,
        };

        // TEST 1: Failure
        // Random user, entirely unaffliaited with the organization
        let test_1 = async {
            let (organization_id, organization_team_id) = create_dummy_org(&test_env.setup_api).await;

            let request = req_gen(&PermissionsTestContext {
                organization_id: Some(&organization_id),
                organization_team_id: Some(&organization_team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                return Err(format!(
                    "Test 1 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes
                        .iter()
                        .map(|code| code.to_string())
                        .join(","),
                    resp.status().as_u16()
                ));
            }

            let p = get_organization_permissions(
                self.user_id,
                self.user_pat,
                &organization_id,
                &test_env.setup_api,
            )
            .await;
            if p != OrganizationPermissions::empty() {
                return Err(format!(
                    "Test 1 failed. Expected no permissions, got {:?}",
                    p
                ));
            }
            Ok(())
        };

        // TEST 2: Failure
        // User affiliated with the organization, with failure permissions
        let test_2 = async {
            let (organization_id, organization_team_id) = create_dummy_org(&test_env.setup_api).await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &organization_team_id,
                None,
                Some(failure_organization_permissions),
                &test_env.setup_api,
            )
            .await;

            let request = req_gen(&PermissionsTestContext {
                organization_id: Some(&organization_id),
                organization_team_id: Some(&organization_team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();

            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                return Err(format!(
                    "Test 2 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes
                        .iter()
                        .map(|code| code.to_string())
                        .join(","),
                    resp.status().as_u16()
                ));
            }

            let p = get_organization_permissions(
                self.user_id,
                self.user_pat,
                &organization_id,
                &test_env.setup_api,
            )
            .await;
            if p != failure_organization_permissions {
                return Err(format!(
                    "Test 2 failed. Expected {:?}, got {:?}",
                    failure_organization_permissions, p
                ));
            }
            Ok(())
        };

        // TEST 3: Success
        // User affiliated with the organization, with the given permissions
        let test_3 = async {
            let (organization_id, organization_team_id) = create_dummy_org(&test_env.setup_api).await;
            add_user_to_team(
                self.user_id,
                self.user_pat,
                &organization_team_id,
                None,
                Some(success_permissions),
                &test_env.setup_api,
            )
            .await;

            let request = req_gen(&PermissionsTestContext {
                organization_id: Some(&organization_id),
                organization_team_id: Some(&organization_team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();

            let resp = test_env.call(request).await;
            if !resp.status().is_success() {
                return Err(format!(
                    "Test 3 failed. Expected success, got {}",
                    resp.status().as_u16()
                ));
            }

            let p = get_organization_permissions(
                self.user_id,
                self.user_pat,
                &organization_id,
                &test_env.setup_api,
            )
            .await;
            if p != success_permissions {
                return Err(format!(
                    "Test 3 failed. Expected {:?}, got {:?}",
                    success_permissions, p
                ));
            }
            Ok(())
        };

        tokio::try_join!(test_1, test_2, test_3,).map_err(|e| e)?;

        Ok(())
    }
}

async fn create_dummy_project(setup_api: &ApiV3) -> (String, String) {
    // Create a very simple project
    let slug = generate_random_name("test_project");

    let (project, _) = setup_api.add_public_project(&slug, None, None, ADMIN_USER_PAT).await;
    let project_id = project.id.to_string();
    let team_id = project.team.to_string();

    (project_id, team_id)
}

async fn create_dummy_org(setup_api: &ApiV3) -> (String, String) {
    // Create a very simple organization
    let name = generate_random_name("test_org");

    let resp = setup_api
        .create_organization(&name, "Example description.", ADMIN_USER_PAT)
        .await;
    assert!(resp.status().is_success());

    let organization = setup_api
        .get_organization_deserialized(&name, ADMIN_USER_PAT)
        .await;
    let organizaion_id = organization.id.to_string();
    let team_id = organization.team_id.to_string();

    (organizaion_id, team_id)
}

async fn add_project_to_org(setup_api: &ApiV3, project_id: &str, organization_id: &str) {
    let resp = setup_api
        .organization_add_project(organization_id, project_id, ADMIN_USER_PAT)
        .await;
    assert!(resp.status().is_success());
}

async fn add_user_to_team(
    user_id: &str,
    user_pat: &str,
    team_id: &str,
    project_permissions: Option<ProjectPermissions>,
    organization_permissions: Option<OrganizationPermissions>,
    setup_api: &ApiV3,
) {
    // Invite user
    let resp = setup_api
        .add_user_to_team(
            team_id,
            user_id,
            project_permissions,
            organization_permissions,
            ADMIN_USER_PAT,
        )
        .await;
    assert!(resp.status().is_success());

    // Accept invitation
    let resp = setup_api.join_team(team_id, user_pat).await;
    assert!(resp.status().is_success());
}

async fn modify_user_team_permissions(
    user_id: &str,
    team_id: &str,
    permissions: Option<ProjectPermissions>,
    organization_permissions: Option<OrganizationPermissions>,
    setup_api: &ApiV3,
) {

    // Send invitation to user
    let resp = setup_api
        .edit_team_member(
            team_id,
            user_id,
            json!({
                "permissions" : permissions.map(|p| p.bits()),
                "organization_permissions" : organization_permissions.map(|p| p.bits()),
            }),
            ADMIN_USER_PAT,
        )
        .await;
    assert!(resp.status().is_success());
}

async fn remove_user_from_team(user_id: &str, team_id: &str, setup_api: &ApiV3) {
    // Send invitation to user
    let resp = setup_api.remove_from_team(team_id, user_id, ADMIN_USER_PAT).await;
    assert!(resp.status().is_success());
}

async fn get_project_permissions(
    user_id: &str,
    user_pat: &str,
    project_id: &str,
    setup_api: &ApiV3,
) -> ProjectPermissions {
    let resp = setup_api.get_project_members(project_id, user_pat).await;
    let permissions = if resp.status().as_u16() == 200 {
        let value: serde_json::Value = test::read_body_json(resp).await;
        value
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["user"]["id"].as_str().unwrap() == user_id)
            .map(|member| member["permissions"].as_u64().unwrap())
            .unwrap_or_default()
    } else {
        0
    };

    ProjectPermissions::from_bits_truncate(permissions)
}

async fn get_organization_permissions(
    user_id: &str,
    user_pat: &str,
    organization_id: &str,
    setup_api: &ApiV3
) -> OrganizationPermissions {
    let resp = setup_api
        .get_organization_members(organization_id, user_pat)
        .await;
    let permissions = if resp.status().as_u16() == 200 {
        let value: serde_json::Value = test::read_body_json(resp).await;
        value
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["user"]["id"].as_str().unwrap() == user_id)
            .map(|member| member["organization_permissions"].as_u64().unwrap())
            .unwrap_or_default()
    } else {
        0
    };

    OrganizationPermissions::from_bits_truncate(permissions)
}
