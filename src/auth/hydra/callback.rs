//! Main authentication flow for Hydra
use crate::auth::hydra::stages::{access_token, bearer_token, player_info, xbl_signin, xsts_token};
use crate::auth::hydra::HydraError;
use crate::database::models::flow_item::Flow;
use crate::database::models::DatabaseError;
use crate::util::env::parse_var;

use actix_web::{get, web, HttpResponse};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct Query {
    pub code: String,
    pub state: String,
}

#[get("auth-redirect")]
pub async fn route(
    info: web::Query<Query>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
) -> Result<HttpResponse, HydraError> {
    let flow = Flow::get(&info.state, &redis).await?;

    // Extract cookie header from request
    if let Some(Flow::MinecraftAuth { user_id }) = flow {
        Flow::remove(&info.state, &redis).await?;
        let public_url = parse_var::<String>("SELF_ADDR").unwrap();
        let client_id = parse_var::<String>("MICROSOFT_CLIENT_ID").unwrap();
        let client_secret = parse_var::<String>("MICROSOFT_CLIENT_ID").unwrap();

        let code = &info.code;

        // Fetch token
        let access_token =
            access_token::fetch_token(public_url, code, &client_id, &client_secret).await?;

        // Get xbl token from oauth token
        let xbl_token = xbl_signin::login_xbl(&access_token.access_token).await?;

        // Get xsts token from xbl token
        let xsts_response = xsts_token::fetch_token(&xbl_token.token).await?;

        return match xsts_response {
            xsts_token::XSTSResponse::Unauthorized(err) => Err(HydraError::Authorization(format!(
                "Error getting XBox Live token: {}",
                err
            ))),
            xsts_token::XSTSResponse::Success { token: xsts_token } => {
                // Get xsts bearer token from xsts token
                let bearer_token = bearer_token::fetch_bearer(&xsts_token, &xbl_token.uhs)
                    .await
                    .map_err(|err| {
                        HydraError::Authorization(format!("Error getting bearer token: {}", err))
                    })?;

                // Get player info from bearer token
                let player_info = player_info::fetch_info(&bearer_token).await.map_err(|_err| {
                    HydraError::Authorization("No Minecraft account for profile. Make sure you own the game and have set a username through the official Minecraft launcher."
                .to_string())
                })?;

                sqlx::query!(
                    r#"
                    UPDATE users
                    SET minecraft_id = $1, minecraft_username = $2
                    WHERE id = $3
                    "#,
                    player_info.id,
                    player_info.name,
                    user_id.0 as i64,
                )
                .execute(&**pool)
                .await
                .map_err(DatabaseError::from)?;

                Ok(HttpResponse::Ok().finish())
            }
        };
    }

    Ok(HttpResponse::NotFound().finish())
}
