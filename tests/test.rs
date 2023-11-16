use common::{environment::{TestEnvironment, with_test_environment_both}, dummy_data::DummyData};
use futures::Future;

use crate::common::database::USER_USER_PAT;


mod common;

#[actix_rt::test]
async fn v2_test() {
    with_test_environment_both(|api, dummy_data : DummyData| async move {
        let alpha_project_id = &dummy_data.project_alpha.project_id;
        let alpha_project_slug = &dummy_data.project_alpha.project_slug;
    
        // TODO: This will become the framework for tests that use both v2 and v3.
    }).await;
}
