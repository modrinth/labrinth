//! Login route for Hydra, redirects to the Microsoft login page before going to the redirect route
use crate::auth::get_user_from_headers;
use crate::models::pats::Scopes;
use crate::parse_var;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::{auth::hydra::stages, database::models::flow_item::Flow};
use actix_web::{get, web, HttpRequest, HttpResponse};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct Query {
    pub id: Option<String>,
}

#[derive(Serialize)]
pub struct AuthorizationInit {
    pub url: String,
}

#[get("login")]
pub async fn route(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?
    .1;

    let flow = Flow::MinecraftAuth {
        user_id: current_user.id.into(),
    };
    flow.insert(Duration::minutes(30), &redis).await?;

    let public_url = parse_var::<String>("SELF_ADDR").unwrap();
    let client_id = parse_var::<String>("MICROSOFT_CLIENT_ID").unwrap();

    let flow_id = "1";
    let url = format!(
        "https://login.live.com/oauth20_authorize.srf?client_id={client_id}&response_type=code&redirect_uri={}&scope={}&state={flow_id}&prompt=select_account&cobrandid=8058f65d-ce06-4c30-9559-473c9275a65d",
        urlencoding::encode(&format!("{}/{}", public_url, stages::access_token::ROUTE_NAME)),
        urlencoding::encode("XboxLive.signin offline_access")
    );
    Ok(HttpResponse::TemporaryRedirect()
        .append_header(("Location", &*url))
        .json(AuthorizationInit { url }))
}
