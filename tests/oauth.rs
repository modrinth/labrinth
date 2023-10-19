use std::collections::HashMap;

use actix_http::StatusCode;
use actix_web::test::{self, TestRequest};
use common::{
    asserts::assert_status,
    database::{FRIEND_USER_PAT, USER_USER_PAT},
    environment::with_test_environment,
};
use labrinth::{
    auth::oauth::{AcceptOAuthClientScopes, OAuthClientAccessRequest, TokenRequest, TokenResponse},
    models::pats::Scopes,
};
use reqwest::header::{AUTHORIZATION, CACHE_CONTROL, LOCATION, PRAGMA};

use crate::common::database::FRIEND_USER_ID;

mod common;

#[actix_rt::test]
async fn oauth_flow() {
    with_test_environment(|env| async move {
        let base_redirect_uri = "https://modrinth.com/test".to_string();
        let client_creation = env
            .v3
            .add_oauth_client(
                "test_client".to_string(),
                Scopes::all(),
                vec![base_redirect_uri.clone()],
                USER_USER_PAT,
            )
            .await;
        let client_id_str = serde_json::to_value(client_creation.client.id)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        // Initiate authorization
        let redirect_uri = format!("{}?foo=bar", base_redirect_uri);
        let original_state = "1234";
        let uri = &format!(
            "/v3/auth/oauth/authorize?client_id={}&redirect_uri={}\
                &scope={}&state={original_state}",
            urlencoding::encode(&client_id_str),
            urlencoding::encode(&redirect_uri),
            urlencoding::encode("USER_READ NOTIFICATION_READ")
        )
        .to_string();
        let req = TestRequest::get()
            .uri(&uri)
            .append_header((AUTHORIZATION, FRIEND_USER_PAT))
            .to_request();
        let resp = env.call(req).await;
        assert_status(&resp, StatusCode::OK);
        let access_request: OAuthClientAccessRequest = test::read_body_json(resp).await;

        // Accept the authorization request
        let resp = env
            .call(
                TestRequest::post()
                    .uri("/v3/auth/oauth/accept")
                    .append_header((AUTHORIZATION, FRIEND_USER_PAT))
                    .set_json(AcceptOAuthClientScopes {
                        flow: access_request.flow_id,
                    })
                    .to_request(),
            )
            .await;
        assert_status(&resp, StatusCode::FOUND);
        let redirect_location = resp.headers().get(LOCATION).unwrap().to_str().unwrap();
        let query = actix_web::web::Query::<HashMap<String, String>>::from_query(
            redirect_location.split_once('?').unwrap().1,
        )
        .unwrap();

        println!("redirect location: {}, {:#?}", redirect_location, query.0);
        let auth_code = query.get("code").unwrap();
        let state = query.get("state").unwrap();
        let foo = query.get("foo").unwrap();
        assert_eq!(state, original_state);
        assert_eq!(foo, "bar");

        // Get the token
        let resp = env
            .call(
                TestRequest::post()
                    .uri("/v3/auth/oauth/token")
                    .append_header((AUTHORIZATION, client_creation.client_secret))
                    .set_form(TokenRequest {
                        grant_type: "authorization_code".to_string(),
                        code: auth_code.to_string(),
                        redirect_uri: Some(redirect_uri.clone()),
                        client_id: client_id_str.to_string(),
                    })
                    .to_request(),
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
