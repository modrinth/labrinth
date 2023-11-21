use crate::models::ids::{Base62Id, UserId};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct PayoutId(pub u64);

#[derive(Serialize, Deserialize, Clone)]
pub struct Payout {
    pub id: PayoutId,
    pub user_id: UserId,
    pub status: PayoutStatus,
    pub created: DateTime<Utc>,
    pub amount: Decimal,

    pub fee: Option<Decimal>,
    pub method: Option<PayoutMethod>,
    /// the address this payout was sent to: ex: email, paypal email, venmo handle
    pub method_address: Option<String>,
    pub platform_id: Option<String>,
}

impl Payout {
    pub fn from(data: crate::database::models::payout_item::Payout) -> Self {
        Self {
            id: data.id.into(),
            user_id: data.user_id.into(),
            status: data.status,
            created: data.created,
            amount: data.amount,
            fee: data.fee,
            method: data.method,
            method_address: data.method_address,
            platform_id: data.platform_id,
        }
    }
}

#[derive(Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PayoutMethod {
    Venmo,
    PayPal,
    Tremendous,
    Unknown,
}

impl std::fmt::Display for PayoutMethod {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl PayoutMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            PayoutMethod::Venmo => "venmo",
            PayoutMethod::PayPal => "paypal",
            PayoutMethod::Tremendous => "tremendous",
            PayoutMethod::Unknown => "unknown",
        }
    }

    pub fn from_string(string: &str) -> PayoutMethod {
        match string {
            "venmo" => PayoutMethod::Venmo,
            "paypal" => PayoutMethod::PayPal,
            "tremendous" => PayoutMethod::Tremendous,
            _ => PayoutMethod::Unknown,
        }
    }
}

#[derive(Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PayoutStatus {
    Success,
    Processing,
    Cancelled,
    Unknown,
}

impl std::fmt::Display for PayoutStatus {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl PayoutStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PayoutStatus::Success => "success",
            PayoutStatus::Processing => "processing",
            PayoutStatus::Cancelled => "cancelled",
            PayoutStatus::Unknown => "unknown",
        }
    }

    pub fn from_string(string: &str) -> PayoutStatus {
        match string {
            "success" => PayoutStatus::Success,
            "processing" => PayoutStatus::Processing,
            "cancelled" => PayoutStatus::Cancelled,
            _ => PayoutStatus::Unknown,
        }
    }
}
