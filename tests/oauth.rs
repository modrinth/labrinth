use crate::common::{database::FRIEND_USER_ID, dummy_data::DummyOAuthClientAlpha};
use actix_http::StatusCode;
use actix_web::{
    dev::ServiceResponse,
    test::{self},
};
use common::{
    asserts::assert_status, database::FRIEND_USER_PAT, environment::with_test_environment,
};
use labrinth::auth::oauth::{OAuthClientAccessRequest, TokenResponse};
use reqwest::header::{CACHE_CONTROL, LOCATION, PRAGMA};
use std::collections::HashMap;

mod common;

#[actix_rt::test]
async fn oauth_flow_happy_path() {
    with_test_environment(|env| async move {
        let DummyOAuthClientAlpha {
            valid_redirect_uri: base_redirect_uri,
            client_id,
            client_secret,
        } = env.dummy.unwrap().oauth_client_alpha.clone();

        // Initiate authorization
        let redirect_uri = format!("{}?foo=bar", base_redirect_uri);
        let original_state = "1234";
        let resp = env
            .v3
            .oauth_authorize(
                &client_id,
                "USER_READ NOTIFICATION_READ",
                &redirect_uri,
                original_state,
                FRIEND_USER_PAT,
            )
            .await;
        assert_status(&resp, StatusCode::OK);
        let access_request: OAuthClientAccessRequest = test::read_body_json(resp).await;

        // Accept the authorization request
        let resp = env
            .v3
            .oauth_accept(&access_request.flow_id, FRIEND_USER_PAT)
            .await;
        assert_status(&resp, StatusCode::FOUND);
        let query = get_redirect_location_query_params(&resp);

        let auth_code = query.get("code").unwrap();
        let state = query.get("state").unwrap();
        let foo = query.get("foo").unwrap();
        assert_eq!(state, original_state);
        assert_eq!(foo, "bar");

        // Get the token
        let resp = env
            .v3
            .oauth_token(
                auth_code.to_string(),
                Some(redirect_uri.clone()),
                client_id.to_string(),
                &client_secret,
            )
            .await;
        assert_status(&resp, StatusCode::OK);
        assert_eq!(resp.headers().get(CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(resp.headers().get(PRAGMA).unwrap(), "no-cache");
        let token_resp: TokenResponse = test::read_body_json(resp).await;

        // Validate that the token works
        env.v2
            .get_user_notifications_deserialized(FRIEND_USER_ID, &token_resp.access_token)
            .await;
    })
    .await;
}

fn get_redirect_location_query_params(
    response: &ServiceResponse,
) -> actix_web::web::Query<HashMap<String, String>> {
    let redirect_location = response.headers().get(LOCATION).unwrap().to_str().unwrap();
    actix_web::web::Query::<HashMap<String, String>>::from_query(
        redirect_location.split_once('?').unwrap().1,
    )
    .unwrap()
}
