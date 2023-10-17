use std::collections::HashMap;
use std::sync::Arc;
use actix_web::test;
use common::dummy_data::TestFile;
use common::request_data;
use labrinth::models::ids::base62_impl::parse_base62;
use labrinth::models::projects::Project;
use labrinth::search::SearchResults;
use serde_json::json;
use futures::stream::StreamExt;
use crate::common::database::*;
use crate::common::dummy_data::DUMMY_CATEGORIES;
use crate::common::{actix::AppendsMultipart, environment::TestEnvironment};

// importing common module.
mod common;

#[actix_rt::test]
async fn search_projects() {
    // Test setup and dummy data
    let test_env = TestEnvironment::build_with_dummy().await;
    let test_name = test_env.db.database_name.clone();
    // Add dummy projects of various categories for searchability
    let mut project_creation_futures = vec![];

    let create_async_future = |id: u64,  pat: &'static str, is_modpack : bool, modify_json : Box<dyn Fn(&mut serde_json::Value)>| {
        let test_env = test_env.clone();
        let slug = format!("{test_name}-searchable-project-{id}");

        let jar = if is_modpack { 
            TestFile::build_random_mrpack()
        } else { 
            TestFile::build_random_jar() 
        };
        let mut basic_project_json = request_data::get_public_project_creation_data_json(&slug, &jar);
        modify_json(&mut basic_project_json);

        let basic_project_multipart =
            request_data::get_public_project_creation_data_multipart(&basic_project_json, &jar);
        // Add a project- simple, should work.
        let req = test::TestRequest::post()
            .uri("/v2/project")
            .append_header(("Authorization", pat))
            .set_multipart(basic_project_multipart)
            .to_request();

         async move {
            let resp = test_env.call(req).await;
            assert_eq!(resp.status(), 200);

            let project : Project = test::read_body_json(resp).await;

            // Approve, so that the project is searchable
            let req = test::TestRequest::patch()
                .uri(&format!("/v2/project/{project_id}", project_id = project.id))
                .append_header(("Authorization", MOD_USER_PAT))
                .set_json(json!({
                    "status": "approved"
                }))
                .to_request();

            let resp = test_env.call(req).await;
            assert_eq!(resp.status(), 204);
            (project.id.0, id)
        }};

    let id = 0;
    let modify_json = | json : &mut serde_json::Value| {
        json["categories"] = json!(DUMMY_CATEGORIES[4..6]);
        json["server_side"] = json!("required");
        json["license_id"] = json!("LGPL-3.0-or-later");
    };
    project_creation_futures.push(create_async_future(id, USER_USER_PAT, false, Box::new(modify_json)));

    let id = 1;
    let modify_json = | json : &mut serde_json::Value| {
        json["categories"] = json!(DUMMY_CATEGORIES[0..2]);
        json["client_side"] = json!("optional");
    };
    project_creation_futures.push(create_async_future(id, USER_USER_PAT, false, Box::new(modify_json)));

    let id = 2;
    let modify_json = | json : &mut serde_json::Value| {
        json["categories"] = json!(DUMMY_CATEGORIES[0..2]);
        json["server_side"] = json!("required");
        json["title"] = json!("Mysterious Project");
    };
    project_creation_futures.push(create_async_future(id, USER_USER_PAT, false, Box::new(modify_json)));

    let id = 3;
    let modify_json = | json : &mut serde_json::Value| {
        json["categories"] = json!(DUMMY_CATEGORIES[0..3]);
        json["server_side"] = json!("required");
        json["initial_versions"][0]["version_number"] = json!("1.2.4");
        json["title"] = json!("Mysterious Project");
        json["license_id"] = json!("LicenseRef-All-Rights-Reserved"); // closed source
    };
    project_creation_futures.push(create_async_future(id, FRIEND_USER_PAT, false, Box::new(modify_json)));

    let id = 4;
    let modify_json = | json : &mut serde_json::Value| {
        json["categories"] = json!(DUMMY_CATEGORIES[0..3]);
        json["client_side"] = json!("optional");
        json["initial_versions"][0]["version_number"] = json!("1.2.5");
    };
    project_creation_futures.push(create_async_future(id, USER_USER_PAT, true, Box::new(modify_json)));

    let id = 5;
    let modify_json = | json : &mut serde_json::Value| {
        json["categories"] = json!(DUMMY_CATEGORIES[5..6]);
        json["client_side"] = json!("optional");
        json["initial_versions"][0]["version_number"] = json!("1.2.5");
        json["license_id"] = json!("LGPL-3.0-or-later");
    };
    project_creation_futures.push(create_async_future(id, USER_USER_PAT, false, Box::new(modify_json)));

    let id = 6;
    let modify_json = | json : &mut serde_json::Value| {
        json["categories"] = json!(DUMMY_CATEGORIES[5..6]);
        json["client_side"] = json!("optional");
        json["server_side"] = json!("required");
        json["license_id"] = json!("LGPL-3.0-or-later");
    };
    project_creation_futures.push(create_async_future(id, FRIEND_USER_PAT, false, Box::new(modify_json)));

    // Await all project creation
    // Returns a mapping of:
    // project id -> test id
    let id_conversion : Arc<HashMap<u64, u64>> = Arc::new(futures::future::join_all(project_creation_futures).await.into_iter().collect());

    // Pairs of:
    // 1. vec of search facets
    // 2. expected project ids to be returned by this search
    let pairs = vec![
        (json!([
            ["categories:fabric"]
        ]), vec![0,1,2,3,4,5,6
        ]),
        (json!([
            ["categories:forge"]
        ]), vec![]),
        (json!([
            ["categories:fabric", "categories:forge"]
            ]), vec![0,1,2,3,4,5,6]),
        (json!([
            ["categories:fabric"],
            ["categories:forge"]
            ]), vec![]),
        (json!([
            ["categories:fabric"],
            [&format!("categories:{}", DUMMY_CATEGORIES[0])],
            ]), vec![1,2,3,4]),
        (json!([
            ["project_type:modpack"]
        ]), vec![4]),
        (json!([
            ["client_side:required"]
            ]), vec![0,2,3]),
        (json!([
            ["server_side:required"]
            ]), vec![0,2,3,6]),
        (json!([
            ["open_source:true"]
            ]), vec![0,1,2,4,5,6]),
        (json!([
            ["license:MIT"]
            ]), vec![1,2,4]),
        (json!([
            [r#"title:'Mysterious Project'"#]
            ]), vec![2,3]),
        (json!([
            ["author:user"]
            ]), vec![0,1,2,4,5])
    ];
    // TODO: versions, game versions

    // Untested:
    // - downloads                      (not varied)
    // - color                          (not varied)
    // - created_timestamp              (not varied)
    // - modified_timestamp             (not varied)

    // Forcibly reset the search index
    let req = test::TestRequest::post()
        .uri("/v2/admin/_force_reindex")
        .append_header(("Modrinth-Admin", dotenvy::var("LABRINTH_ADMIN_KEY").unwrap()))
        .to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);
    
    // Test searches
    let stream = futures::stream::iter(pairs);
    stream.for_each_concurrent(10, |(facets, mut expected_project_ids)| {
        let test_env = test_env.clone();
        let id_conversion = id_conversion.clone();
        let test_name = test_name.clone();
        async move {
            let req = test::TestRequest::get()
                .uri(&format!("/v2/search?query={test_name}&facets={facets}", facets=urlencoding::encode(&facets.to_string())))
                .append_header(("Authorization", USER_USER_PAT))
                .set_json(&facets)
                .to_request();
            let resp = test_env.call(req).await;
            let status = resp.status();
            assert_eq!(status, 200);
            let projects : SearchResults = test::read_body_json(resp).await;
            let mut found_project_ids : Vec<u64> = projects.hits.into_iter().map(|p| id_conversion[&parse_base62(&p.project_id).unwrap()]).collect();
            expected_project_ids.sort();
            found_project_ids.sort();
            assert_eq!(found_project_ids, expected_project_ids);
        }
    }).await;

    // Cleanup test db
    test_env.cleanup().await;
}