use crate::models::ids::Base62Id;
use crate::models::ids::UserId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ProductId(pub u64);

#[derive(Serialize, Deserialize)]
pub struct Product {
    pub id: ProductId,
    pub metadata: ProductMetadata,
    pub prices: Vec<ProductPrice>,
    pub unitary: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProductMetadata {
    Midas,
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ProductPriceId(pub u64);

#[derive(Serialize, Deserialize)]
pub struct ProductPrice {
    pub id: ProductPriceId,
    pub product_id: ProductId,
    pub prices: Price,
    pub currency_code: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Price {
    OneTime {
        price: i32,
    },
    Recurring {
        intervals: HashMap<PriceDuration, i32>,
    },
}

#[derive(Serialize, Deserialize, Hash, Eq, PartialEq, Debug, Copy, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum PriceDuration {
    Monthly,
    Yearly,
}

impl PriceDuration {
    pub fn from_string(string: &str) -> PriceDuration {
        match string {
            "monthly" => PriceDuration::Monthly,
            "yearly" => PriceDuration::Yearly,
            _ => PriceDuration::Monthly,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PriceDuration::Monthly => "monthly",
            PriceDuration::Yearly => "yearly",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct UserSubscriptionId(pub u64);

#[derive(Serialize, Deserialize)]
pub struct UserSubscription {
    pub id: UserSubscriptionId,
    pub user_id: UserId,
    pub price_id: ProductPriceId,
    pub interval: PriceDuration,
    pub status: SubscriptionStatus,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub last_charge: Option<DateTime<Utc>>,
}

impl From<crate::database::models::user_subscription_item::UserSubscriptionItem>
    for UserSubscription
{
    fn from(x: crate::database::models::user_subscription_item::UserSubscriptionItem) -> Self {
        Self {
            id: x.id.into(),
            user_id: x.user_id.into(),
            price_id: x.price_id.into(),
            interval: x.interval,
            status: x.status,
            created: x.created,
            expires: x.expires,
            last_charge: x.last_charge,
        }
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionStatus {
    Active,
    PaymentProcessing,
    PaymentFailed,
    Cancelled,
}

impl SubscriptionStatus {
    pub fn from_string(string: &str) -> SubscriptionStatus {
        match string {
            "active" => SubscriptionStatus::Active,
            "payment-processing" => SubscriptionStatus::PaymentProcessing,
            "payment-failed" => SubscriptionStatus::PaymentFailed,
            "cancelled" => SubscriptionStatus::Cancelled,
            _ => SubscriptionStatus::Cancelled,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SubscriptionStatus::Active => "active",
            SubscriptionStatus::PaymentProcessing => "payment-processing",
            SubscriptionStatus::PaymentFailed => "payment-failed",
            SubscriptionStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ChargeId(pub u64);

#[derive(Serialize, Deserialize)]
pub struct Charge {
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

#[derive(Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ChargeStatus {
    // Open charges are for the next billing interval
    Open,
    Processing,
    Succeeded,
    Failed,
}

impl ChargeStatus {
    pub fn from_string(string: &str) -> ChargeStatus {
        match string {
            "processing" => ChargeStatus::Processing,
            "succeeded" => ChargeStatus::Succeeded,
            "failed" => ChargeStatus::Failed,
            "open" => ChargeStatus::Open,
            _ => ChargeStatus::Failed,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ChargeStatus::Processing => "processing",
            ChargeStatus::Succeeded => "succeeded",
            ChargeStatus::Failed => "failed",
            ChargeStatus::Open => "open",
        }
    }
}
