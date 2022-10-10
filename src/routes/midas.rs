use crate::routes::ApiError;
use crate::util::auth::get_user_from_headers;
use actix_web::{post, web, HttpRequest, HttpResponse};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac, NewMac};
use itertools::Itertools;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct CheckoutData {
    pub price_id: String,
}

#[post("/_stripe-init-checkout")]
pub async fn init_checkout(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    data: web::Json<CheckoutData>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let client = reqwest::Client::new();

    #[derive(Deserialize)]
    struct Session {
        url: Option<String>,
    }

    let session = client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .header("Authorization", format!("Bearer {}", dotenv::var("STRIPE_TOKEN")?),)
        .form(&[
            ("mode", "subscription"),
            ("line_items[0][price]", &*data.price_id),
            ("line_items[0][quantity]", "1"),
            ("success_url", "https://modrinth.com/welcome-to-midas"),
            ("cancel_url", "https://modrinth.com/midas"),
            ("metadata[user_id]", &user.id.to_string()),
        ])
        .send()
        .await
        .unwrap()
        .json::<Session>()
        .await
        .unwrap();

    Ok(HttpResponse::Ok().json(json!(
        {
           "url": session.url
        }
    )))
}

#[post("/_stripe-init-portal")]
pub async fn init_customer_portal(
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let customer_id = sqlx::query!(
        "
        SELECT u.stripe_customer_id
        FROM users u
        WHERE u.id = $1
        ",
        user.id.0 as i64,
    )
    .fetch_optional(&**pool)
    .await?
    .map(|x| x.stripe_customer_id)
    .flatten()
    .ok_or_else(|| {
        ApiError::InvalidInput(
            "User is not linked to stripe account!".to_string(),
        )
    })?;

    let client = reqwest::Client::new();

    #[derive(Deserialize)]
    struct Session {
        url: Option<String>,
    }

    let session = client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .header(
            "Authorization",
            format!("Bearer {}", dotenv::var("STRIPE_TOKEN")?),
        )
        .form(&[
            ("customer", &*customer_id),
            ("return_url", "https://modrinth.com/settings/billing"),
        ])
        .send()
        .await
        .unwrap()
        .json::<Session>()
        .await
        .unwrap();

    Ok(HttpResponse::Ok().json(json!(
        {
           "url": session.url
        }
    )))
}

#[post("/_stripe-webook")]
pub async fn handle_stripe_webhook(
    body: String,
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    if let Some(signature_raw) = req
        .headers()
        .get("Stripe-Signature")
        .map(|x| x.to_str().ok())
        .flatten()
    {
        let mut timestamp = None;
        let mut signature = None;
        for val in signature_raw.split(",") {
            let key_val = val.split("=").collect_vec();

            if key_val.len() == 2 {
                if key_val[0] == "v1" {
                    signature = hex::decode(key_val[1]).ok()
                } else if key_val[0] == "t" {
                    timestamp = key_val[1].parse::<i64>().ok()
                }
            }
        }

        if let Some(timestamp) = timestamp {
            if let Some(signature) = signature {
                type HmacSha256 = Hmac<sha2::Sha256>;

                let mut key = HmacSha256::new_from_slice(dotenv::var("STRIPE_WEBHOOK_SECRET")?.as_bytes()).map_err(|_| {
                    ApiError::Crypto(
                        "Unable to initialize HMAC instance due to invalid key length!".to_string(),
                    )
                })?;

                key.update(format!("{}.{}", timestamp, body).as_bytes());

                key.verify(&signature).map_err(|_| {
                    ApiError::Crypto(
                        "Unable to verify webhook signature!".to_string(),
                    )
                })?;

                if timestamp < (Utc::now() - Duration::minutes(5)).timestamp()
                    || timestamp
                        > (Utc::now() + Duration::minutes(5)).timestamp()
                {
                    return Err(ApiError::Crypto(
                        "Webhook signature expired!".to_string(),
                    ));
                }
            } else {
                return Err(ApiError::Crypto("Missing signature!".to_string()));
            }
        } else {
            return Err(ApiError::Crypto("Missing timestamp!".to_string()));
        }
    } else {
        return Err(ApiError::Crypto("Missing signature header!".to_string()));
    }

    #[derive(Deserialize)]
    struct StripeWebhookBody {
        #[serde(rename = "type")]
        type_: String,
        data: serde_json::Value,
    }


    let webhook : StripeWebhookBody = serde_json::from_str(&*body)?;

    let data_string =

    match &*webhook.type_ {
        "checkout.session.completed" => {
            // set stripe_customer_id
        },
        "invoice.paid" => {
            // set midas_expires in db with stripe customer id
        },
        "invoice.payment_failed" => {

        },
        _ => {}
    };



    println!("{}", body);

    Ok(HttpResponse::NoContent().body(""))
}
