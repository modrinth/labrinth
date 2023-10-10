use crate::routes::ApiError;
use crate::util::env::parse_var;
use crate::{database::redis::RedisPool, models::projects::MonetizationStatus};
use chrono::{DateTime, Datelike, Duration, Utc, Weekday};
use hex::ToHex;
use hmac::{Hmac, Mac, NewMac};
use reqwest::Method;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use sqlx::PgPool;
use std::collections::HashMap;

pub struct PayoutsQueue {
    access_key: String,
    secret_key: String,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccountUser {
    Business { name: String },
    Individual { first: String, last: String },
}

impl Default for PayoutsQueue {
    fn default() -> Self {
        Self::new()
    }
}

// Batches payouts and handles token refresh
impl PayoutsQueue {
    pub fn new() -> Self {
        PayoutsQueue {
            access_key: dotenvy::var("TROLLEY_ACCESS_KEY").expect("missing trolley access key"),
            secret_key: dotenvy::var("TROLLEY_SECRET_KEY").expect("missing trolley secret key"),
        }
    }

    pub async fn make_trolley_request<T: Serialize, X: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<T>,
    ) -> Result<X, ApiError> {
        let timestamp = Utc::now().timestamp();

        let mut mac: Hmac<Sha256> = Hmac::new_from_slice(self.secret_key.as_bytes())
            .map_err(|_| ApiError::Payments("error initializing HMAC".to_string()))?;
        mac.update(
            if let Some(body) = &body {
                format!(
                    "{}\n{}\n{}\n{}\n",
                    timestamp,
                    method.as_str(),
                    path,
                    serde_json::to_string(&body)?
                )
            } else {
                format!("{}\n{}\n{}\n\n", timestamp, method.as_str(), path)
            }
            .as_bytes(),
        );
        let request_signature = mac.finalize().into_bytes().encode_hex::<String>();

        let client = reqwest::Client::new();

        let mut request = client
            .request(method, format!("https://api.trolley.com{path}"))
            .header(
                "Authorization",
                format!("prsign {}:{}", self.access_key, request_signature),
            )
            .header("X-PR-Timestamp", timestamp);

        if let Some(body) = body {
            request = request.json(&body);
        }

        let resp = request
            .send()
            .await
            .map_err(|_| ApiError::Payments("could not communicate with Trolley".to_string()))?;

        let value = resp.json::<Value>().await.map_err(|_| {
            ApiError::Payments("could not retrieve Trolley response body".to_string())
        })?;

        if let Some(obj) = value.as_object() {
            if !obj.get("ok").and_then(|x| x.as_bool()).unwrap_or(true) {
                #[derive(Deserialize)]
                struct TrolleyError {
                    field: Option<String>,
                    message: String,
                }

                if let Some(array) = obj.get("errors") {
                    let err = serde_json::from_value::<Vec<TrolleyError>>(array.clone()).map_err(
                        |_| {
                            ApiError::Payments(
                                "could not retrieve Trolley error json body".to_string(),
                            )
                        },
                    )?;

                    if let Some(first) = err.into_iter().next() {
                        return Err(ApiError::Payments(if let Some(field) = &first.field {
                            format!("error - field: {field} message: {}", first.message)
                        } else {
                            first.message
                        }));
                    }
                }

                return Err(ApiError::Payments(
                    "could not retrieve Trolley error body".to_string(),
                ));
            }
        }

        Ok(serde_json::from_value(value)?)
    }

    pub async fn send_payout(
        &mut self,
        recipient: &str,
        amount: Decimal,
    ) -> Result<(String, Option<String>), ApiError> {
        #[derive(Deserialize)]
        struct TrolleyReq {
            batch: Batch,
        }

        #[derive(Deserialize)]
        struct Batch {
            id: String,
            payments: BatchPayments,
        }

        #[derive(Deserialize)]
        struct Payment {
            id: String,
        }

        #[derive(Deserialize)]
        struct BatchPayments {
            payments: Vec<Payment>,
        }

        let res = self
            .make_trolley_request::<_, TrolleyReq>(
                Method::POST,
                "/v1/batches/",
                Some(json!({
                    "currency": "USD",
                    "description": "labrinth payout",
                    "payments": [{
                        "recipient": {
                            "id": recipient
                        },
                        "amount": amount.to_string(),
                        "currency": "USD",
                        "memo": "Modrinth ad revenue payout"
                    }],
                })),
            )
            .await?;

        self.make_trolley_request::<Value, Value>(
            Method::POST,
            &format!("/v1/batches/{}/start-processing", res.batch.id),
            None,
        )
        .await?;

        let payment_id = res.batch.payments.payments.into_iter().next().map(|x| x.id);

        Ok((res.batch.id, payment_id))
    }

    // listen to webhooks for batch status, account status
    pub async fn register_recipient(
        &self,
        email: &str,
        user: AccountUser,
    ) -> Result<String, ApiError> {
        #[derive(Deserialize)]
        struct TrolleyReq {
            recipient: Recipient,
        }

        #[derive(Deserialize)]
        struct Recipient {
            id: String,
        }

        let id = self
            .make_trolley_request::<_, TrolleyReq>(
                Method::POST,
                "/v1/recipients/",
                Some(match user {
                    AccountUser::Business { name } => json!({
                        "type": "business",
                        "email": email,
                        "name": name,
                    }),
                    AccountUser::Individual { first, last } => json!({
                        "type": "individual",
                        "firstName": first,
                        "lastName": last,
                        "email": email,
                    }),
                }),
            )
            .await?;

        Ok(id.recipient.id)
    }

    pub async fn update_recipient_email(&self, id: &str, email: &str) -> Result<(), ApiError> {
        self.make_trolley_request::<_, Value>(
            Method::PATCH,
            &format!("/v1/recipients/{}", id),
            Some(json!({
                "email": email,
            })),
        )
        .await?;

        Ok(())
    }
}

pub async fn process_payout(
    pool: &PgPool,
    redis: &RedisPool,
    client: &clickhouse::Client,
) -> Result<(), ApiError> {
    let start: DateTime<Utc> = DateTime::from_utc(
        (Utc::now() - Duration::days(1))
            .date_naive()
            .and_hms_nano_opt(0, 0, 0, 0)
            .unwrap_or_default(),
        Utc,
    );

    let results = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM payouts_values WHERE created = $1)",
        start,
    )
    .fetch_one(pool)
    .await?;

    if results.exists.unwrap_or(false) {
        return Ok(());
    }

    let end = start + Duration::days(1);
    #[derive(Deserialize, clickhouse::Row)]
    struct ProjectMultiplier {
        pub page_views: u64,
        pub project_id: u64,
    }

    let (views_values, views_sum, downloads_values, downloads_sum) = futures::future::try_join4(
        client
            .query(
                r#"
                SELECT COUNT(id) page_views, project_id
                FROM views
                WHERE (recorded BETWEEN ? AND ?) AND (project_id != 0)
                GROUP BY project_id
                ORDER BY page_views DESC
                "#,
            )
            .bind(start.timestamp())
            .bind(end.timestamp())
            .fetch_all::<ProjectMultiplier>(),
        client
            .query("SELECT COUNT(id) FROM views WHERE (recorded BETWEEN ? AND ?) AND (project_id != 0)")
            .bind(start.timestamp())
            .bind(end.timestamp())
            .fetch_one::<u64>(),
        client
            .query(
                r#"
                SELECT COUNT(id) page_views, project_id
                FROM downloads
                WHERE (recorded BETWEEN ? AND ?) AND (user_id != 0)
                GROUP BY project_id
                ORDER BY page_views DESC
                "#,
            )
            .bind(start.timestamp())
            .bind(end.timestamp())
            .fetch_all::<ProjectMultiplier>(),
        client
            .query("SELECT COUNT(id) FROM downloads WHERE (recorded BETWEEN ? AND ?) AND (user_id != 0)")
            .bind(start.timestamp())
            .bind(end.timestamp())
            .fetch_one::<u64>(),
    )
        .await?;

    let mut transaction = pool.begin().await?;

    struct PayoutMultipliers {
        sum: u64,
        values: HashMap<u64, u64>,
    }

    let mut views_values = views_values
        .into_iter()
        .map(|x| (x.project_id, x.page_views))
        .collect::<HashMap<u64, u64>>();
    let downloads_values = downloads_values
        .into_iter()
        .map(|x| (x.project_id, x.page_views))
        .collect::<HashMap<u64, u64>>();

    for (key, value) in downloads_values.iter() {
        let counter = views_values.entry(*key).or_insert(0);
        *counter += *value;
    }

    let multipliers: PayoutMultipliers = PayoutMultipliers {
        sum: downloads_sum + views_sum,
        values: views_values,
    };

    struct Project {
        // user_id, payouts_split
        team_members: Vec<(i64, Decimal)>,
    }

    let mut projects_map: HashMap<i64, Project> = HashMap::new();

    use futures::TryStreamExt;

    sqlx::query!(
        "
        SELECT m.id id, tm.user_id user_id, tm.payouts_split payouts_split
        FROM mods m
        INNER JOIN team_members tm on m.team_id = tm.team_id AND tm.accepted = TRUE
        WHERE m.id = ANY($1) AND m.monetization_status = $2
        ",
        &multipliers
            .values
            .keys()
            .map(|x| *x as i64)
            .collect::<Vec<i64>>(),
        MonetizationStatus::Monetized.as_str(),
    )
    .fetch_many(&mut *transaction)
    .try_for_each(|e| {
        if let Some(row) = e.right() {
            if let Some(project) = projects_map.get_mut(&row.id) {
                project.team_members.push((row.user_id, row.payouts_split));
            } else {
                projects_map.insert(
                    row.id,
                    Project {
                        team_members: vec![(row.user_id, row.payouts_split)],
                    },
                );
            }
        }

        futures::future::ready(Ok(()))
    })
    .await?;

    let amount = Decimal::from(parse_var::<u64>("PAYOUTS_BUDGET").unwrap_or(0));

    let days = Decimal::from(28);
    let weekdays = Decimal::from(20);
    let weekend_bonus = Decimal::from(5) / Decimal::from(4);

    let weekday_amount = amount / (weekdays + (weekend_bonus) * (days - weekdays));
    let weekend_amount = weekday_amount * weekend_bonus;

    let payout = match start.weekday() {
        Weekday::Sat | Weekday::Sun => weekend_amount,
        _ => weekday_amount,
    };

    let mut clear_cache_users = Vec::new();
    for (id, project) in projects_map {
        if let Some(value) = &multipliers.values.get(&(id as u64)) {
            let project_multiplier: Decimal =
                Decimal::from(**value) / Decimal::from(multipliers.sum);

            let sum_splits: Decimal = project.team_members.iter().map(|x| x.1).sum();

            if sum_splits > Decimal::ZERO {
                for (user_id, split) in project.team_members {
                    let payout: Decimal = payout * project_multiplier * (split / sum_splits);

                    if payout > Decimal::ZERO {
                        sqlx::query!(
                            "
                            INSERT INTO payouts_values (user_id, mod_id, amount, created)
                            VALUES ($1, $2, $3, $4)
                            ",
                            user_id,
                            id,
                            payout,
                            start
                        )
                        .execute(&mut *transaction)
                        .await?;

                        sqlx::query!(
                            "
                            UPDATE users
                            SET balance = balance + $1
                            WHERE id = $2
                            ",
                            payout,
                            user_id
                        )
                        .execute(&mut *transaction)
                        .await?;

                        clear_cache_users.push(user_id);
                    }
                }
            }
        }
    }

    if !clear_cache_users.is_empty() {
        crate::database::models::User::clear_caches(
            &clear_cache_users
                .into_iter()
                .map(|x| (crate::database::models::UserId(x), None))
                .collect::<Vec<_>>(),
            redis,
        )
        .await?;
    }

    transaction.commit().await?;

    Ok(())
}
