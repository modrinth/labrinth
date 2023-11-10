use common::environment::TestEnvironment;

mod common;

#[actix_rt::test]
async fn get_tags() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v3;

    // TODO:
    panic!("There is currently no v3 tags test.");

    test_env.cleanup().await;
}
