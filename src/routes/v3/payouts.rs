use crate::auth::get_user_from_headers;
use crate::database::redis::RedisPool;
use crate::models::ids::PayoutId;
use crate::models::pats::Scopes;
use crate::models::payouts::{PayoutMethod, PayoutStatus};
use crate::queue::payouts::PayoutsQueue;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse};
use hyper::Method;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("payout")
            .service(paypal_webhook)
            .service(tremendous_webhook)
            .service(user_payouts)
            .service(create_payout)
            .service(cancel_payout),
    );
}

#[post("_paypal")]
pub async fn paypal_webhook(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    payouts: web::Data<PayoutsQueue>,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse, ApiError> {
    let auth_algo = req
        .headers()
        .get("PAYPAL-AUTH-ALGO")
        .and_then(|x| x.to_str().ok())
        .ok_or_else(|| ApiError::InvalidInput("missing auth algo".to_string()))?;
    let cert_url = req
        .headers()
        .get("PAYPAL-CERT-URL")
        .and_then(|x| x.to_str().ok())
        .ok_or_else(|| ApiError::InvalidInput("missing cert url".to_string()))?;
    let transmission_id = req
        .headers()
        .get("PAYPAL-TRANSMISSION-ID")
        .and_then(|x| x.to_str().ok())
        .ok_or_else(|| ApiError::InvalidInput("missing transmission ID".to_string()))?;
    let transmission_sig = req
        .headers()
        .get("PAYPAL-TRANSMISSION-SIG")
        .and_then(|x| x.to_str().ok())
        .ok_or_else(|| ApiError::InvalidInput("missing transmission sig".to_string()))?;
    let transmission_time = req
        .headers()
        .get("PAYPAL-TRANSMISSION-TIME")
        .and_then(|x| x.to_str().ok())
        .ok_or_else(|| ApiError::InvalidInput("missing transmission time".to_string()))?;

    #[derive(Deserialize)]
    struct WebHookResponse {
        verification_status: String,
    }

    let payouts: WebHookResponse = payouts
        .make_paypal_request(
            Method::POST,
            "notifications/verify-webhook-signature",
            Some(json!({
                "auth_algo": auth_algo,
                "cert_url": cert_url,
                "transmission_id": transmission_id,
                "transmission_sig": transmission_sig,
                "transmission_time": transmission_time,
                "webhook_id": dotenvy::var("PAYPAL_WEBHOOK_ID")?,
                "webhook_event": body.0,
            })),
            None,
        )
        .await?;

    if &payouts.verification_status != "SUCCESS" {
        return Err(ApiError::InvalidInput(
            "Invalid webhook signature".to_string(),
        ));
    }

    println!("{}", payouts.verification_status);
    println!("{:?}", body.0);

    // TODO: Actually handle stuff here!!

    Ok(HttpResponse::NoContent().finish())
}

#[post("_tremendous")]
pub async fn tremendous_webhook(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    body: String,
) -> Result<HttpResponse, ApiError> {
    let signature = req.headers().get("Tremendous-Webhook-Signature");

    // TODO: finish this

    Ok(HttpResponse::NoContent().finish())
}

#[get("")]
pub async fn user_payouts(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PAYOUTS_READ]),
    )
    .await?
    .1;

    let payout_ids =
        crate::database::models::payout_item::Payout::get_all_for_user(user.id.into(), &**pool)
            .await?;
    let payouts =
        crate::database::models::payout_item::Payout::get_many(&payout_ids, &**pool).await?;

    // todo: historical payouts get
    Ok(HttpResponse::NoContent().finish())
}

#[derive(Deserialize)]
pub struct Withdrawal {
    amount: Decimal,
    method: PayoutMethod,
}

#[post("")]
pub async fn create_payout(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    body: web::Json<Withdrawal>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PAYOUTS_WRITE]),
    )
    .await?
    .1;

    // todo: payment withdraw (paypal, tremendous)

    Ok(HttpResponse::NoContent().finish())
}

#[delete("{id}")]
pub async fn cancel_payout(
    info: web::Path<(PayoutId,)>,
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    payouts: web::Data<PayoutsQueue>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PAYOUTS_WRITE]),
    )
    .await?
    .1;

    let id = info.into_inner().0;
    let payout = crate::database::models::payout_item::Payout::get(id.into(), &**pool).await?;

    if let Some(payout) = payout {
        if payout.user_id != user.id.into() && !user.role.is_admin() {
            return Ok(HttpResponse::NotFound().finish());
        }

        if let Some(platform_id) = payout.platform_id {
            if let Some(method) = payout.method {
                if payout.status == PayoutStatus::Success {
                    return Err(ApiError::InvalidInput(
                        "Payout cannot be cancelled!".to_string(),
                    ));
                }

                match method {
                    PayoutMethod::Venmo | PayoutMethod::PayPal => {
                        payouts
                            .make_paypal_request::<(), ()>(
                                Method::POST,
                                &format!("payments/payouts-item/{}/cancel", platform_id),
                                None,
                                None,
                            )
                            .await?;

                        Ok(HttpResponse::NoContent().finish())
                    }
                    PayoutMethod::Tremendous => {
                        // TODO: support tremendous here
                        // paypal: https://api-m.paypal.com/v1/payments/payouts-item/{payout_item_id}/cancel
                        Ok(HttpResponse::NoContent().finish())
                    }
                    PayoutMethod::Unknown => Err(ApiError::InvalidInput(
                        "Payout cannot be cancelled!".to_string(),
                    )),
                }
            } else {
                Err(ApiError::InvalidInput(
                    "Payout cannot be cancelled!".to_string(),
                ))
            }
        } else {
            Err(ApiError::InvalidInput(
                "Payout cannot be cancelled!".to_string(),
            ))
        }
    } else {
        Ok(HttpResponse::NotFound().finish())
    }
}

// todo: maybe gift card list + filtering? (tremendous)
