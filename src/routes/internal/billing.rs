use crate::auth::{get_user_from_headers, send_email};
use crate::database::models::{
    generate_user_subscription_id, product_item, user_subscription_item,
};
use crate::database::redis::RedisPool;
use crate::models::billing::{
    PriceDuration, PriceInterval, Product, ProductMetadata, ProductPrice, SubscriptionStatus,
    UserSubscription,
};
use crate::models::ids::base62_impl::{parse_base62, to_base62};
use crate::models::pats::Scopes;
use crate::models::users::Badges;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use chrono::{Duration, Utc};
use serde_with::serde_derive::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::str::FromStr;
use stripe::{
    CreateCustomer, CreatePaymentIntent, CreatePaymentIntentAutomaticPaymentMethods,
    CreatePaymentIntentAutomaticPaymentMethodsAllowRedirects, CreateSetupIntent,
    CreateSetupIntentAutomaticPaymentMethods,
    CreateSetupIntentAutomaticPaymentMethodsAllowRedirects, Currency, CustomerId,
    CustomerInvoiceSettings, CustomerPaymentMethodRetrieval, EventObject, EventType, ListCharges,
    PaymentIntentOffSession, PaymentIntentStatus, PaymentMethodId, SetupIntent, UpdateCustomer,
    Webhook,
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("billing")
            .service(products)
            .service(subscriptions)
            .service(user_customer)
            .service(cancel_subscription)
            .service(payment_methods)
            .service(add_payment_method_flow)
            .service(edit_payment_method)
            .service(remove_payment_method)
            .service(charges)
            .service(initiate_payment)
            .service(stripe_webhook),
    );
}

// TODO: cache this
#[get("products")]
pub async fn products(pool: web::Data<PgPool>) -> Result<HttpResponse, ApiError> {
    let products = product_item::ProductItem::get_all(&**pool).await?;
    let prices = product_item::ProductPriceItem::get_all_products_prices(
        &products.iter().map(|x| x.id).collect::<Vec<_>>(),
        &**pool,
    )
    .await?;

    let products = products
        .into_iter()
        .map(|x| Product {
            id: x.id.into(),
            metadata: x.metadata,
            prices: prices
                .remove(&x.id)
                .map(|x| x.1)
                .unwrap_or_default()
                .into_iter()
                .map(|x| ProductPrice {
                    id: x.id.into(),
                    product_id: x.product_id.into(),
                    interval: x.interval,
                    price: x.price,
                    currency_code: x.currency_code,
                })
                .collect(),
            unitary: x.unitary,
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(products))
}

#[get("subscriptions")]
pub async fn subscriptions(
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
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let subscriptions =
        user_subscription_item::UserSubscriptionItem::get_all_user(user.id.into(), &**pool)
            .await?
            .into_iter()
            .map(|x| UserSubscription {
                id: x.id.into(),
                user_id: x.user_id.into(),
                price_id: x.price_id.into(),
                status: x.status,
                created: x.created,
                expires: x.expires,
                last_charge: x.last_charge,
            })
            .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(subscriptions))
}

#[patch("subscription/{id}")]
pub async fn cancel_subscription(
    req: HttpRequest,
    info: web::Path<(crate::models::ids::UserSubscriptionId,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let (id,) = info.into_inner();

    if let Some(mut subscription) =
        user_subscription_item::UserSubscriptionItem::get(id.into(), &**pool).await?
    {
        if subscription.user_id != user.id.into() || !user.role.is_admin() {
            return Err(ApiError::NotFound);
        }

        let mut transaction = pool.begin().await?;

        subscription.status = SubscriptionStatus::Cancelled;
        subscription.upsert(&mut transaction).await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::NotFound)
    }
}

#[get("customer")]
pub async fn user_customer(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    stripe_client: web::Data<stripe::Client>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let customer_id = get_or_create_customer(&user, &*stripe_client, &pool, &redis).await?;
    let customer = stripe::Customer::retrieve(&stripe_client, &customer_id, &[]).await?;

    Ok(HttpResponse::Ok().json(customer))
}

#[get("payments")]
pub async fn charges(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    stripe_client: web::Data<stripe::Client>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    if let Some(customer_id) = user
        .stripe_customer_id
        .as_ref()
        .and_then(|x| stripe::CustomerId::from_str(x).ok())
    {
        let charges = stripe::Charge::list(
            &stripe_client,
            &ListCharges {
                customer: Some(customer_id),
                limit: Some(100),
                ..Default::default()
            },
        )
        .await?;

        Ok(HttpResponse::Ok().json(charges.data))
    } else {
        Ok(HttpResponse::NoContent().finish())
    }
}

#[post("payment_method")]
pub async fn add_payment_method_flow(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    stripe_client: web::Data<stripe::Client>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let customer = get_or_create_customer(&user, &*stripe_client, &pool, &redis).await?;

    let intent = SetupIntent::create(
        &stripe_client,
        CreateSetupIntent {
            customer: Some(customer),
            automatic_payment_methods: Some(CreateSetupIntentAutomaticPaymentMethods {
                allow_redirects: Some(
                    CreateSetupIntentAutomaticPaymentMethodsAllowRedirects::Never,
                ),
                enabled: true,
            }),
            ..Default::default()
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "client_secret": intent.client_secret
    })))
}

#[derive(Deserialize)]
pub struct EditPaymentMethod {
    pub primary: bool,
}

#[patch("payment_method/{id}")]
pub async fn edit_payment_method(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    stripe_client: web::Data<stripe::Client>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let (id,) = info.into_inner();

    let payment_method_id = if let Ok(id) = PaymentMethodId::from_str(&id) {
        id
    } else {
        return Err(ApiError::NotFound);
    };

    let customer = get_or_create_customer(&user, &*stripe_client, &pool, &redis).await?;

    let payment_method =
        stripe::PaymentMethod::retrieve(&stripe_client, &payment_method_id, &[]).await?;

    if payment_method
        .customer
        .map(|x| x.id() == customer)
        .unwrap_or(false)
        || user.role.is_admin()
    {
        stripe::Customer::update(
            &stripe_client,
            &customer,
            UpdateCustomer {
                invoice_settings: Some(CustomerInvoiceSettings {
                    default_payment_method: Some(payment_method.id.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await?;

        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(ApiError::NotFound)
    }
}

#[delete("payment_method/{id}")]
pub async fn remove_payment_method(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    stripe_client: web::Data<stripe::Client>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let (id,) = info.into_inner();

    let payment_method_id = if let Ok(id) = PaymentMethodId::from_str(&id) {
        id
    } else {
        return Err(ApiError::NotFound);
    };

    let customer = get_or_create_customer(&user, &*stripe_client, &pool, &redis).await?;

    let payment_method =
        stripe::PaymentMethod::retrieve(&stripe_client, &payment_method_id, &[]).await?;

    let user_subscriptions =
        user_subscription_item::UserSubscriptionItem::get_all_user(user.id.into(), &**pool).await?;

    if user_subscriptions
        .iter()
        .any(|x| x.status != SubscriptionStatus::Cancelled)
    {
        let customer = stripe::Customer::retrieve(&stripe_client, &customer, &[]).await?;

        if customer
            .invoice_settings
            .and_then(|x| {
                x.default_payment_method
                    .map(|x| x.id() == payment_method_id)
            })
            .unwrap_or(false)
        {
            return Err(ApiError::InvalidInput(
                "You may not remove the default payment method if you have active subscriptions!"
                    .to_string(),
            ));
        }
    }

    if payment_method
        .customer
        .map(|x| x.id() == customer)
        .unwrap_or(false)
        || user.role.is_admin()
    {
        stripe::PaymentMethod::detach(&stripe_client, &payment_method_id).await?;

        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(ApiError::NotFound)
    }
}

#[get("payment_methods")]
pub async fn payment_methods(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    stripe_client: web::Data<stripe::Client>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    if let Some(customer_id) = user
        .stripe_customer_id
        .as_ref()
        .and_then(|x| stripe::CustomerId::from_str(x).ok())
    {
        let methods = stripe::Customer::retrieve_payment_methods(
            &stripe_client,
            &customer_id,
            CustomerPaymentMethodRetrieval {
                limit: Some(100),
                ..Default::default()
            },
        )
        .await?;

        Ok(HttpResponse::Ok().json(methods.data))
    } else {
        Ok(HttpResponse::NoContent().finish())
    }
}

#[derive(Deserialize)]
pub struct PaymentRequest {
    pub price_id: crate::models::ids::ProductPriceId,
    pub payment_method_id: Option<String>,
}

// TODO: Change to use confirmation tokens once async_stripe supports api
#[post("payment")]
pub async fn initiate_payment(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    stripe_client: web::Data<stripe::Client>,
    payment_request: web::Json<PaymentRequest>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let price = product_item::ProductPriceItem::get(payment_request.price_id.into(), &**pool)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("Specified product price could not be found!".to_string())
        })?;

    let product = product_item::ProductItem::get(price.product_id, &**pool)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("Specified product could not be found!".to_string())
        })?;

    let customer_id = get_or_create_customer(&user, &*stripe_client, &pool, &redis).await?;

    let mut intent = CreatePaymentIntent::new(
        price.price as i64,
        Currency::from_str(&price.currency_code).unwrap_or(Currency::USD),
    );

    let mut metadata = HashMap::new();
    metadata.insert("modrinth_user_id".to_string(), to_base62(user.id.0));
    metadata.insert("modrinth_price_id".to_string(), to_base62(user.id.0));

    if product.unitary {
        let user_subscriptions =
            user_subscription_item::UserSubscriptionItem::get_all_user(user.id.into(), &**pool)
                .await?;

        let user_products = product_item::ProductPriceItem::get_many(
            &user_subscriptions
                .iter()
                .map(|x| x.price_id)
                .collect::<Vec<_>>(),
            &**pool,
        )
        .await?;

        if let Some(product) = user_products
            .into_iter()
            .find(|x| x.product_id == product.id)
        {
            if let Some(subscription) = user_subscriptions
                .into_iter()
                .find(|x| x.price_id == product.id)
            {
                if subscription.status == SubscriptionStatus::Cancelled
                    || subscription.status == SubscriptionStatus::PaymentFailed
                {
                    metadata.insert("modrinth_subscription_id".to_string(), to_base62(user.id.0));
                } else {
                    return Err(ApiError::InvalidInput(
                        "You are already subscribed to this product!".to_string(),
                    ));
                }
            }
        }
    }

    if let PriceInterval::Recurring { .. } = price.interval {
        if !metadata.contains_key("modrinth_subscription_id") {
            let mut transaction = pool.begin().await?;
            let user_subscription_id = generate_user_subscription_id(&mut transaction).await?;
        }
    }

    intent.metadata = Some(metadata);

    intent.automatic_payment_methods = Some(CreatePaymentIntentAutomaticPaymentMethods {
        allow_redirects: Some(CreatePaymentIntentAutomaticPaymentMethodsAllowRedirects::Never),
        enabled: true,
    });

    if let Some(payment_method) = payment_request
        .payment_method_id
        .clone()
        .and_then(|x| stripe::PaymentMethodId::from_str(&x).ok())
    {
        intent.payment_method = Some(payment_method);
        intent.confirm = Some(false);
        intent.off_session = Some(PaymentIntentOffSession::Exists(true))
    }

    intent.receipt_email = user.email.as_deref();

    let payment_intent = stripe::PaymentIntent::create(&stripe_client, intent).await?;

    if payment_intent.status == PaymentIntentStatus::Succeeded {
        return Ok(HttpResponse::NoContent().finish());
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "status": payment_intent.status,
        "client_secret": payment_intent.client_secret
    })))
}

#[post("_stripe")]
pub async fn stripe_webhook(
    req: HttpRequest,
    payload: String,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    stripe_client: web::Data<stripe::Client>,
) -> Result<HttpResponse, ApiError> {
    let stripe_signature = req
        .headers()
        .get("Stripe-Signature")
        .and_then(|x| x.to_str().ok())
        .unwrap_or_default();

    if let Ok(event) = Webhook::construct_event(
        &*payload,
        stripe_signature,
        &*dotenvy::var("STRIPE_WEBHOOK_SECRET")?,
    ) {
        struct PaymentIntentMetadata {
            user: crate::database::models::User,
            user_subscription_id: Option<crate::database::models::ids::UserSubscriptionId>,
            user_subscription: Option<user_subscription_item::UserSubscriptionItem>,
            product: product_item::ProductItem,
            product_price: product_item::ProductPriceItem,
        }

        async fn get_payment_intent_metadata(
            metadata: HashMap<String, String>,
            pool: &PgPool,
            redis: &RedisPool,
        ) -> Result<PaymentIntentMetadata, ApiError> {
            if let Some(user_id) = metadata
                .get("modrinth_user_id")
                .and_then(|x| parse_base62(x).ok())
                .map(|x| crate::database::models::ids::UserId(x as i64))
            {
                let user =
                    crate::database::models::user_item::User::get_id(user_id, pool, &redis).await?;

                if let Some(user) = user {
                    let (user_subscription_id, user_subscription) = if let Some(subscription_id) =
                        metadata
                            .get("modrinth_subscription_id")
                            .and_then(|x| parse_base62(x).ok())
                            .map(|x| crate::database::models::ids::UserSubscriptionId(x as i64))
                    {
                        let subscription = user_subscription_item::UserSubscriptionItem::get(
                            subscription_id,
                            pool,
                        )
                        .await?;

                        (Some(subscription_id), subscription)
                    } else {
                        (None, None)
                    };

                    if let Some(price_id) = metadata
                        .get("modrinth_price_id")
                        .and_then(|x| parse_base62(x).ok())
                        .map(|x| crate::database::models::ids::ProductPriceId(x as i64))
                    {
                        let price = product_item::ProductPriceItem::get(price_id, pool).await?;

                        if let Some(product_price) = price {
                            let product =
                                product_item::ProductItem::get(product_price.product_id, pool)
                                    .await?;

                            if let Some(product) = product {
                                return Ok(PaymentIntentMetadata {
                                    user,
                                    user_subscription_id,
                                    user_subscription,
                                    product,
                                    product_price,
                                });
                            }
                        }
                    }
                }
            }

            Err(ApiError::InvalidInput(
                "Webhook missing required webhook metadata!".to_string(),
            ))
        }

        match event.type_ {
            EventType::PaymentIntentSucceeded => {
                if let EventObject::PaymentIntent(payment_intent) = event.data.object {
                    let metadata =
                        get_payment_intent_metadata(payment_intent.metadata, &pool, &redis).await?;

                    let mut transaction = pool.begin().await?;

                    if let PriceInterval::Recurring { amount, duration } =
                        metadata.product_price.interval
                    {
                        if let Some(subscription_id) = metadata.user_subscription_id {
                            let duration = match duration {
                                PriceDuration::Day => Duration::days(amount as i64),
                                PriceDuration::Week => Duration::days((amount * 7) as i64),
                                PriceDuration::Month => Duration::days((amount * 30) as i64),
                                PriceDuration::Year => Duration::days((amount * 365) as i64),
                            };

                            if let Some(mut user_subscription) = metadata.user_subscription {
                                user_subscription.expires = Utc::now() + duration;
                                user_subscription.status = SubscriptionStatus::Active;
                                user_subscription.upsert(&mut transaction).await?;
                            } else {
                                user_subscription_item::UserSubscriptionItem {
                                    id: subscription_id,
                                    user_id: metadata.user.id,
                                    price_id: metadata.product_price.id,
                                    created: Utc::now(),
                                    expires: Utc::now() + duration,
                                    last_charge: None,
                                    status: SubscriptionStatus::Active,
                                }
                                .upsert(&mut transaction)
                                .await?;
                            }
                        }
                    }

                    // Provision subscription
                    match metadata.product.metadata {
                        ProductMetadata::Midas => {
                            let badges = metadata.user.badges & Badges::MIDAS;

                            sqlx::query!(
                                "
                                UPDATE users
                                SET badges = $1
                                WHERE (id = $2)
                                ",
                                badges.bits() as i64,
                                metadata.user.id as crate::database::models::ids::UserId,
                            )
                            .execute(&mut *transaction)
                            .await?;
                        }
                    }

                    transaction.commit().await?;
                }
            }
            EventType::PaymentIntentProcessing => {
                if let EventObject::PaymentIntent(payment_intent) = event.data.object {
                    let metadata =
                        get_payment_intent_metadata(payment_intent.metadata, &pool, &redis).await?;

                    let mut transaction = pool.begin().await?;

                    if let PriceInterval::Recurring { .. } = metadata.product_price.interval {
                        if let Some(subscription_id) = metadata.user_subscription_id {
                            if let Some(mut user_subscription) = metadata.user_subscription {
                                user_subscription.status = SubscriptionStatus::PaymentProcessing;
                                user_subscription.upsert(&mut transaction).await?;
                            } else {
                                user_subscription_item::UserSubscriptionItem {
                                    id: subscription_id,
                                    user_id: metadata.user.id,
                                    price_id: metadata.product_price.id,
                                    created: Utc::now(),
                                    expires: Utc::now(),
                                    last_charge: None,
                                    status: SubscriptionStatus::PaymentProcessing,
                                }
                                .upsert(&mut transaction)
                                .await?;
                            }
                        }
                    }

                    transaction.commit().await?;
                }
            }
            EventType::PaymentIntentPaymentFailed => {
                if let EventObject::PaymentIntent(payment_intent) = event.data.object {
                    let metadata =
                        get_payment_intent_metadata(payment_intent.metadata, &pool, &redis).await?;

                    let mut transaction = pool.begin().await?;

                    if let PriceInterval::Recurring { .. } = metadata.product_price.interval {
                        if let Some(subscription_id) = metadata.user_subscription_id {
                            if let Some(mut user_subscription) = metadata.user_subscription {
                                user_subscription.last_charge = Some(Utc::now());
                                user_subscription.status = SubscriptionStatus::PaymentFailed;
                                user_subscription.upsert(&mut transaction).await?;
                            } else {
                                user_subscription_item::UserSubscriptionItem {
                                    id: subscription_id,
                                    user_id: metadata.user.id,
                                    price_id: metadata.product_price.id,
                                    created: Utc::now(),
                                    expires: Utc::now(),
                                    last_charge: Some(Utc::now()),
                                    status: SubscriptionStatus::PaymentFailed,
                                }
                                .upsert(&mut transaction)
                                .await?;
                            }
                        }
                    }

                    if let Some(email) = metadata.user.email {
                        let money = rusty_money::Money::from_minor(
                            metadata.product_price.price as i64,
                            rusty_money::iso::find(&metadata.product_price.currency_code)
                                .unwrap_or(rusty_money::iso::USD),
                        );

                        send_email(
                            email,
                            "[Action Required] Payment Failed for Modrinth",
                            &format!("Our attempt to collect payment for {money} from the payment card on file was unsuccessful."),
                            "Please visit the following link below to update your payment method or contact your card provider. If the button does not work, you can copy the link and paste it into your browser.",
                            Some(("Update billing settings", &format!("{}/{}", dotenvy::var("SITE_URL")?,  dotenvy::var("SITE_BILLING_PATH")?))),
                        )?;
                    }

                    transaction.commit().await?;
                }
            }
            EventType::SetupIntentSucceeded => {
                if let EventObject::SetupIntent(setup_intent) = event.data.object {
                    if let Some(customer_id) = setup_intent.customer.map(|x| x.id()) {
                        if let Some(payment_method_id) = setup_intent.payment_method.map(|x| x.id())
                        {
                            let customer =
                                stripe::Customer::retrieve(&stripe_client, &customer_id, &[])
                                    .await?;

                            if !customer
                                .invoice_settings
                                .map(|x| x.default_payment_method.is_some())
                                .unwrap_or(false)
                            {
                                stripe::Customer::update(
                                    &stripe_client,
                                    &customer_id,
                                    UpdateCustomer {
                                        invoice_settings: Some(CustomerInvoiceSettings {
                                            default_payment_method: Some(
                                                payment_method_id.to_string(),
                                            ),
                                            ..Default::default()
                                        }),
                                        ..Default::default()
                                    },
                                )
                                .await?;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    } else {
        return Err(ApiError::InvalidInput(
            "Webhook signature validation failed!".to_string(),
        ));
    }

    Ok(HttpResponse::Ok().finish())
}

async fn get_or_create_customer(
    user: &crate::models::users::User,
    client: &stripe::Client,
    pool: &PgPool,
    redis: &RedisPool,
) -> Result<CustomerId, ApiError> {
    if let Some(customer_id) = user
        .stripe_customer_id
        .as_ref()
        .and_then(|x| stripe::CustomerId::from_str(x).ok())
    {
        Ok(customer_id)
    } else {
        let mut metadata = HashMap::new();
        metadata.insert("modrinth_user_id".to_string(), to_base62(user.id.0));

        let customer = stripe::Customer::create(
            client,
            CreateCustomer {
                email: user.email.as_deref(),
                metadata: Some(metadata),
                ..Default::default()
            },
        )
        .await?;

        sqlx::query!(
            "
            UPDATE users
            SET stripe_customer_id = $1
            WHERE id = $2
            ",
            customer.id.as_str(),
            user.id.0 as i64
        )
        .execute(&*pool)
        .await?;

        crate::database::models::user_item::User::clear_caches(&[(user.id.into(), None)], &redis)
            .await?;

        Ok(customer.id)
    }
}

pub async fn task(pool: &PgPool, redis: &RedisPool) -> Result<(), ApiError> {
    Ok(())

    // TODO: scheduler for charging recurring payments

    // if subscription is cancelled and expired, unprovision
    // if subscription is payment failed and expired, unprovision
    // if subscription is payment failed and last attempt is > 4 days ago, try again to charge
    // if subscription is active and expired, attempt to charge and set as processing

    // get all users
    // get all user customers

    // Un provision subscription
    //                     match metadata.product.metadata {
    //                         ProductMetadata::Midas => {
    //                             let badges = metadata.user.badges - Badges::MIDAS;
    //
    //                             sqlx::query!(
    //                                 "
    //                                 UPDATE users
    //                                 SET badges = $1
    //                                 WHERE (id = $2)
    //                                 ",
    //                                 badges.bits() as i64,
    //                                 metadata.user.id as crate::database::models::ids::UserId,
    //                             )
    //                             .execute(&mut *transaction)
    //                             .await?;
    //                         }
    //                     }
}
