use crate::models::ids::Base62Id;
use crate::models::ids::UserId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    pub interval: PriceInterval,
    pub price: i32,
    pub currency_code: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PriceInterval {
    OneTime,
    /// For recurring payments. amount: 1 and duration: 'week' would result in a recurring payment
    /// every week
    Recurring {
        amount: usize,
        duration: PriceDuration,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PriceDuration {
    Day,
    Week,
    Month,
    Year,
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
    pub status: SubscriptionStatus,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub last_charge: Option<DateTime<Utc>>,
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
