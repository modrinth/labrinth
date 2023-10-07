#![allow(dead_code)]
use std::pin::Pin;

use actix_web::test::{self, TestRequest};
use futures::Future;
use itertools::Itertools;
use labrinth::models::teams::{ProjectPermissions, OrganizationPermissions};
use serde_json::json;

use crate::common::database::generate_random_name;

use super::{database::{USER_USER_ID, USER_USER_PAT}, environment::TestEnvironment, actix::{AppendsMultipart, MultipartSegmentData, MultipartSegment}};

// A reusable test type that works for any permissions test testing an endpoint that:
// - returns a known 'expected_failure_code' if the scope is not present (defaults to 401)
// - returns a 200-299 if the scope is present
// - returns failure and success JSON bodies for requests that are 200 (for performing non-simple follow-up tests on)
// This uses a builder format, so you can chain methods to set the parameters to non-defaults (most will probably be not need to be set).
pub struct PermissionsTest<'a> {
    test_env: &'a TestEnvironment,
    // Permissions expected to fail on this test. By default, this is all permissions except the success permissions.
    // (To ensure we have isolated the permissions we are testing)
    failure_project_permissions: Option<ProjectPermissions>,
    failure_organization_permissions: Option<OrganizationPermissions>,

    // User ID to use for the test user, and their PAT
    user_id : &'a str,
    user_pat : &'a str,

    // Setup function
    // This runs after the project/org/etc is created, but before the test route
    // This is useful for setting up the database for the test (eg: adding a thread to a project)
    setup_fn : Option<Box<dyn Fn(&TestEnvironment) -> Pin<Box<dyn Future<Output = ()>>> + Send>>,

    // The codes that is allow to be returned if the scope is not present.
    // (for instance, we might expect a 401, but not a 400)
    allowed_failure_codes: Vec<u16>,
}

pub struct PermissionsTestContext<'a> {
    pub test_env: &'a TestEnvironment,
    pub user_id: &'a str,
    pub user_pat: &'a str,
    pub project_id: Option<&'a str>,
    pub team_id: Option<&'a str>,
    pub organization_id: Option<&'a str>,
    pub organization_team_id: Option<&'a str>,
}

impl<'a> PermissionsTest<'a> {
    pub fn new(test_env: &'a TestEnvironment) -> Self {
        Self {
            test_env,
            failure_project_permissions: None,
            failure_organization_permissions: None,
            user_id: USER_USER_ID,
            user_pat: USER_USER_PAT,
            setup_fn: None,
            allowed_failure_codes: vec![401, 404],
        }
    }

    // Set non-standard failure permissions
    // If not set, it will be set to all permissions except the success permissions
    // (eg: if a combination of permissions is needed, but you want to make sure that the endpoint does not work with all-but-one of them)
    pub fn with_failure_permissions(mut self, failure_project_permissions: Option<ProjectPermissions>, failure_organization_permissions: Option<OrganizationPermissions> ) -> Self {
        self.failure_project_permissions = failure_project_permissions;
        self.failure_organization_permissions = failure_organization_permissions;
        self
    }

    // Set the user ID to use
    // (eg: a moderator, or friend)
    pub fn with_user(mut self, user_id: &'a str, user_pat : &'a str) -> Self {
        self.user_id = user_id;
        self.user_pat = user_pat;
        self
    }

    // If a non-standard code is expected.
    // (eg: perhaps 200 for a resource with hidden values deeper in)
    pub fn with_failure_codes(mut self, allowed_failure_codes: impl IntoIterator<Item = u16>) -> Self {
        self.allowed_failure_codes = allowed_failure_codes.into_iter().collect();
        self
    }

    pub async fn project_permissions_test<T>(
        &self,
        success_permissions: ProjectPermissions,
        req_gen: T,
    )
    -> Result<(), String>
    where
        T: Fn(&PermissionsTestContext) -> TestRequest,
    {
        let test_env = self.test_env;
        let failure_project_permissions = self.failure_project_permissions.unwrap_or(ProjectPermissions::all() ^ success_permissions);
        let test_context = PermissionsTestContext {
            test_env,
            user_id: self.user_id,
            user_pat: self.user_pat,
            project_id: None,
            team_id: None,
            organization_id: None,
            organization_team_id: None,
        };
    
        // TEST 1: Failure
        // Random user, unaffiliated with the project, with no permissions
        {
            let (project_id, team_id) = create_dummy_project(test_env).await;
    
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                println!("Body: {:?}", resp.response().body());
                return Err(format!(
                    "Test 1 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes.iter().map(|code| code.to_string()).join(","),
                    resp.status().as_u16()
                ));
            }
        }
        // TEST 2: Failure
        // User affiliated with the project, with failure permissions
        {
            let (project_id, team_id) = create_dummy_project(test_env).await;
            add_user_to_team(self.user_id, self.user_pat, &team_id, Some(failure_project_permissions), None, test_env).await;
        
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
    
            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {             
                println!("Body: {:?}", resp.response().body());
                return Err(format!(
                    "Test 2 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes.iter().map(|code| code.to_string()).join(","),
                    resp.status().as_u16()
                ));
            }
        }
    
        // TEST 3: Success
        // User affiliated with the project, with the given permissions
        {
            let (project_id, team_id) = create_dummy_project(test_env).await;
            add_user_to_team(self.user_id, self.user_pat, &team_id, Some(success_permissions), None, test_env).await;
        
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
    
            let resp = test_env.call(request).await;
            println!("Body: {:?}", resp.response().body());
            if !resp.status().is_success() {
                return Err(format!(
                    "Test 3 failed. Expected success, got {}",
                    resp.status().as_u16()
                ));
            }
        }
    
        // TEST 4: Failure
        // Project has an organization
        // User affiliated with the project's org, with default failure permissions
        {
            let (project_id, team_id) = create_dummy_project(test_env).await;
            let (organization_id, organization_team_id) = create_dummy_org(test_env).await;
            add_project_to_org(test_env, &project_id, &organization_id).await;
            add_user_to_team(self.user_id, self.user_pat, &organization_team_id, Some(failure_project_permissions), None, test_env).await;
        
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
    
            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                println!("Body: {:?}", resp.response().body());
                return Err(format!(
                    "Test 4 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes.iter().map(|code| code.to_string()).join(","),
                    resp.status().as_u16()
                ));
            }
        }
    
        // TEST 5: Success
        // Project has an organization
        // User affiliated with the project's org, with the default success
        {
            let (project_id, team_id) = create_dummy_project(test_env).await;
            let (organization_id, organization_team_id) = create_dummy_org(test_env).await;
            add_project_to_org(test_env, &project_id, &organization_id).await;
            add_user_to_team(self.user_id, self.user_pat, &organization_team_id, Some(success_permissions), None, test_env).await;
        
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
    
            let resp = test_env.call(request).await;
            if !resp.status().is_success() {
                println!("Body: {:?}", resp.response().body());
                return Err(format!(
                    "Test 5 failed. Expected success, got {}",
                    resp.status().as_u16()
                ));
            }
        }
    
        // TEST 6: Failure
        // Project has an organization
        // User affiliated with the project's org (even can have successful permissions!) 
        // User overwritten on the project team with failure permissions
        {
            let (project_id, team_id) = create_dummy_project(test_env).await;
            let (organization_id, organization_team_id) = create_dummy_org(test_env).await;
            add_project_to_org(test_env, &project_id, &organization_id).await;
            add_user_to_team(self.user_id, self.user_pat, &organization_team_id, Some(success_permissions), None, test_env).await;
            add_user_to_team(self.user_id, self.user_pat, &team_id, Some(failure_project_permissions), None, test_env).await;
        
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
    
            let resp = test_env.call(request).await;
            if !self.allowed_failure_codes.contains(&resp.status().as_u16()) {
                println!("Body: {:?}", resp.response().body());
                return Err(format!(
                    "Test 6 failed. Expected failure codes {} got {}",
                    self.allowed_failure_codes.iter().map(|code| code.to_string()).join(","),
                    resp.status().as_u16()
                ));
            }
        }
        // TEST 7: Success
        // Project has an organization
        // User affiliated with the project's org with default failure permissions
        // User overwritten to the project with the success permissions
        {
            let (project_id, team_id) = create_dummy_project(test_env).await;
            let (organization_id, organization_team_id) = create_dummy_org(test_env).await;
            add_project_to_org(test_env, &project_id, &organization_id).await;
            add_user_to_team(self.user_id, self.user_pat, &organization_team_id, Some(failure_project_permissions), None, test_env).await;
            add_user_to_team(self.user_id, self.user_pat, &team_id, Some(success_permissions), None, test_env).await;
        
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                project_id: Some(&project_id),
                team_id: Some(&team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
    
            let resp = test_env.call(request).await;

            if !resp.status().is_success() {
                println!("Body: {:?}", resp.response().body());
                return Err(format!(
                    "Test 7 failed. Expected success, got {}",
                    resp.status().as_u16()
                ));
            }
        }
        Ok(())
    }

    pub async fn organization_permissions_tests<T>(
        &self,
        success_permissions: OrganizationPermissions,
        req_gen: T,
    )
    where
        T: Fn(&PermissionsTestContext) -> TestRequest,
    {
        let test_env = self.test_env;
        let failure_organization_permissions = self.failure_organization_permissions.unwrap_or(OrganizationPermissions::all() ^ success_permissions);
        let test_context = PermissionsTestContext {
            test_env,
            user_id: self.user_id,
            user_pat: self.user_pat,
            project_id: None, // Will be overwritten on each test
            team_id: None, // Will be overwritten on each test
            organization_id: None,
            organization_team_id: None,
        };

        // TEST 1: Failure
        // Random user, entirely unaffliaited with the organization
        {
            let (organization_id, organization_team_id) = create_dummy_org(test_env).await;
    
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                organization_id: Some(&organization_id),
                organization_team_id: Some(&organization_team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
            let resp = test_env.call(request).await;
            assert!(self.allowed_failure_codes.contains(&resp.status().as_u16()));
        }

        // TEST 2: Failure
        // User affiliated with the organization, with failure permissions
        {
            let (organization_id, organization_team_id) = create_dummy_org(test_env).await;
            add_user_to_team(self.user_id, self.user_pat, &organization_team_id, None, Some(failure_organization_permissions), test_env).await;
        
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                organization_id: Some(&organization_id),
                organization_team_id: Some(&organization_team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
    
            let resp = test_env.call(request).await;
            assert!(self.allowed_failure_codes.contains(&resp.status().as_u16()));
        }

        // TEST 3: Success
        // User affiliated with the organization, with the given permissions
        {
            let (organization_id, organization_team_id) = create_dummy_org(test_env).await;
            add_user_to_team(self.user_id, self.user_pat, &organization_team_id, None, Some(success_permissions), test_env).await;
        
            if let Some(ref setup_fn) = self.setup_fn {
                setup_fn(test_env).await;
            } 
    
            let request = req_gen(&PermissionsTestContext {
                organization_id: Some(&organization_id),
                organization_team_id: Some(&organization_team_id),
                ..test_context
            })
            .append_header(("Authorization", self.user_pat))
            .to_request();
    
            let resp = test_env.call(request).await;
            assert!(resp.status().is_success());
        }
    }
    
    

}

async fn create_dummy_project(test_env : &TestEnvironment) -> (String, String) {
        // Create a very simple project
        let slug = generate_random_name("test_project");
        let json_data = json!(
            {
                "title": &slug,
                "slug": &slug,
                "description": "Example description.",
                "body": "Example body.",
                "client_side": "required",
                "server_side": "optional",
                "is_draft": true,
                "initial_versions": [],
                "categories": [],
                "license_id": "MIT"
            }
        );
        let json_segment = MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
        };
        let req = test::TestRequest::post()
            .uri("/v2/project")
            .append_header(("Authorization", "mrp_patadmin")) // Admin so that user can be added to the project, and friend/enemy is unused
            .set_multipart([json_segment])
            .to_request();
        let resp = test_env.call(req).await;
        assert!(resp.status().is_success());    

        let req = test::TestRequest::get()
        .uri(&format!("/v2/project/{}", &slug))
        .append_header(("Authorization", "mrp_patadmin")) // Admin so that user can be added to the project, and friend/enemy is unused
        .to_request();
        let resp = test_env.call(req).await;
        assert!(resp.status().is_success()); 
        let success : serde_json::Value = test::read_body_json(resp).await;

        let project_id = success["id"].as_str().unwrap().to_string();
        let team_id = success["team"].as_str().unwrap().to_string();


        (project_id, team_id)
}

async fn create_dummy_org(test_env : &TestEnvironment) -> (String, String) {
    // Create a very simple organization
    let name = generate_random_name("test_org");
    let req = test::TestRequest::post()
        .uri("/v2/organization")
        .append_header(("Authorization", "mrp_patadmin")) // Admin so that user can be added to the project, and friend/enemy is unused
        .set_json(json!({
            "title": &name,
            "slug": &name,
            "description": "Example description.",
        }))
        .to_request();
    let resp = test_env.call(req).await;
    assert!(resp.status().is_success());    

    let req = test::TestRequest::get()
    .uri(&format!("/v2/organization/{}", &name))
    .append_header(("Authorization", "mrp_patadmin")) // Admin so that user can be added to the project, and friend/enemy is unused
    .to_request();
    let resp = test_env.call(req).await;
    assert!(resp.status().is_success());    
    let success : serde_json::Value = test::read_body_json(resp).await;

    let organizaion_id = success["id"].as_str().unwrap().to_string();
    let team_id = success["team_id"].as_str().unwrap().to_string();

    (organizaion_id, team_id)
}

async fn add_project_to_org(test_env : &TestEnvironment, project_id : &str, organization_id : &str) {
    let req = test::TestRequest::post()
    .uri(&format!("/v2/organization/{organization_id}/projects"))
    .append_header(("Authorization", "mrp_patadmin")) // Admin so that user can be added to the project, and friend/enemy is unused
    .set_json(json!({
        "project_id" : project_id,
    }))
    .to_request();
    let resp = test_env.call(req).await;
    assert!(resp.status().is_success());    
}

async fn add_user_to_team(user_id : &str, user_pat : &str, team_id : &str, permissions : Option<ProjectPermissions>, organization_permissions : Option<OrganizationPermissions>, test_env : &TestEnvironment) {
    // Send invitation to user
    let req = test::TestRequest::post()
    .uri(&format!("/v2/team/{team_id}/members"))
    .append_header(("Authorization", "mrp_patadmin")) // Admin so that user can be added to the project, and friend/enemy is unused
    .set_json(json!({
        "user_id" : user_id,
        "permissions" : permissions.map(|p| p.bits()).unwrap_or_default(),
        "organization_permissions" : organization_permissions.map(|p| p.bits()),
    }))
    .to_request();
    let resp = test_env.call(req).await;
    assert!(resp.status().is_success());    

    // Accept invitation
    let req = test::TestRequest::post()
    .uri(&format!("/v2/team/{team_id}/join"))
    .append_header(("Authorization", user_pat))
    .to_request();
    let resp = test_env.call(req).await;
    assert!(resp.status().is_success());
}


