use crate::database::models::{
    ChargeId, DatabaseError, ProductPriceId, UserId, UserSubscriptionId,
};
use crate::models::billing::{ChargeStatus, PriceDuration};
use chrono::{DateTime, Utc};
use std::convert::TryFrom;

pub struct ChargeItem {
    pub id: ChargeId,
    pub user_id: UserId,
    pub price_id: ProductPriceId,
    pub amount: i64,
    pub currency_code: String,
    pub subscription_id: Option<UserSubscriptionId>,
    pub interval: Option<PriceDuration>,
    pub status: ChargeStatus,
    pub due: DateTime<Utc>,
    pub last_attempt: Option<DateTime<Utc>>,
}

struct ChargeResult {
    id: i64,
    user_id: i64,
    price_id: i64,
    amount: i64,
    currency_code: String,
    subscription_id: Option<i64>,
    interval: Option<String>,
    status: String,
    due: DateTime<Utc>,
    last_attempt: Option<DateTime<Utc>>,
}

impl TryFrom<ChargeResult> for ChargeItem {
    type Error = serde_json::Error;

    fn try_from(r: ChargeResult) -> Result<Self, Self::Error> {
        Ok(ChargeItem {
            id: ChargeId(r.id),
            user_id: UserId(r.user_id),
            price_id: ProductPriceId(r.price_id),
            amount: r.amount,
            currency_code: r.currency_code,
            subscription_id: r.subscription_id.map(UserSubscriptionId),
            interval: r.interval.map(|x| serde_json::from_str(&x)).transpose()?,
            status: serde_json::from_str(&r.status)?,
            due: r.due,
            last_attempt: r.last_attempt,
        })
    }
}

macro_rules! select_charges_with_predicate {
    ($predicate:tt, $param:ident) => {
        sqlx::query_as!(
            ChargeResult,
            r#"
            SELECT id, user_id, price_id, amount, currency_code, subscription_id, interval, status, due, last_attempt
            FROM charges
            "#
                + $predicate,
            $param
        )
    };
}

impl ChargeItem {
    pub async fn insert(
        &self,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<ChargeId, DatabaseError> {
        sqlx::query!(
            r#"
            INSERT INTO charges (id, user_id, price_id, amount, currency_code, subscription_id, interval, status, due, last_attempt)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            self.id.0,
            self.user_id.0,
            self.price_id.0,
            self.amount,
            self.currency_code,
            self.subscription_id.map(|x| x.0),
            self.interval.map(|x| x.as_str()),
            self.status.as_str(),
            self.due,
            self.last_attempt,
        )
        .execute(exec)
        .await?;

        Ok(self.id)
    }

    pub async fn get(
        id: ChargeId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Option<ChargeItem>, DatabaseError> {
        let res = select_charges_with_predicate!("WHERE id = $1", id)
            .fetch_optional(exec)
            .await?;

        Ok(res.map(|r| r.try_into()))
    }

    pub async fn get_from_user(
        user_id: UserId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Vec<ChargeItem>, DatabaseError> {
        let res = select_charges_with_predicate!("WHERE user_id = $1", user_id)
            .fetch_all(exec)
            .await?;

        Ok(res
            .into_iter()
            .map(|r| r.try_into())
            .collect::<Result<Vec<_>, serde_json::Error>>()?)
    }

    pub async fn get_chargeable(
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Vec<ChargeItem>, DatabaseError> {
        let res = select_charges_with_predicate!("WHERE (status = 'open' AND due < NOW()) OR (status = 'failed' AND last_attempt < NOW() - INTERVAL '2 days')")
            .fetch_all(exec)
            .await?;

        Ok(res
            .into_iter()
            .map(|r| r.try_into())
            .collect::<Result<Vec<_>, serde_json::Error>>()?)
    }
}
