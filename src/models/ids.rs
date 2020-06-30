use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ModId(pub u64);

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct UserId(pub u64);

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct VersionId(pub u64);

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct TeamId(pub u64);

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Base62Id(pub u64);

#[derive(Error, Debug)]
pub enum EncodingError {
    #[error("Invalid base62 encoding")]
    InvalidBase62(char),
}

macro_rules! from_base62id {
    ($($struct:ty, $con:expr;)+) => {
        $(
            impl From<Base62Id> for $struct {
                fn from(id: Base62Id) -> $struct {
                    $con(id.0)
                }
            }
            impl From<$struct> for Base62Id {
                fn from(id: $struct) -> Base62Id {
                    Base62Id(id.0)
                }
            }
        )+
    };
}

from_base62id! {
    ModId, ModId;
    UserId, UserId;
    VersionId, VersionId;
    TeamId, TeamId;
}

mod base62_impl {
    use serde::de::{self, Deserializer, Visitor};
    use serde::ser::Serializer;
    use serde::{Deserialize, Serialize};

    use super::{Base62Id, EncodingError};

    impl<'de> Deserialize<'de> for Base62Id {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct Base62Visitor;

            impl<'de> Visitor<'de> for Base62Visitor {
                type Value = Base62Id;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a base62 string id")
                }

                fn visit_str<E>(self, string: &str) -> Result<Base62Id, E>
                where
                    E: de::Error,
                {
                    parse_base62(string).map(Base62Id).map_err(E::custom)
                }
            }

            deserializer.deserialize_str(Base62Visitor)
        }
    }

    impl Serialize for Base62Id {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&to_base62(self.0))
        }
    }

    const BASE62_CHARS: [u8; 62] = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'A', b'B', b'C', b'D', b'E',
        b'F', b'G', b'H', b'I', b'J', b'K', b'L', b'M', b'N', b'O', b'P', b'Q', b'R', b'S', b'T',
        b'U', b'V', b'W', b'X', b'Y', b'Z', b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i',
        b'j', b'k', b'l', b'm', b'n', b'o', b'p', b'q', b'r', b's', b't', b'u', b'v', b'w', b'x',
        b'y', b'z',
    ];

    fn to_base62(mut num: u64) -> String {
        let length = (num as f64).log(62.0).ceil() as usize;
        let mut output = String::with_capacity(length);

        while num > 0 {
            output.push(BASE62_CHARS[(num % 62) as usize] as char);
            num /= 62;
        }
        output
    }

    fn parse_base62(string: &str) -> Result<u64, EncodingError> {
        let mut num = 0;
        for c in string.chars().rev() {
            num *= 62;
            if c.is_ascii_digit() {
                num += (c as u8 - b'0') as u64;
            } else if c.is_ascii_uppercase() {
                num += 10 + (c as u8 - b'A') as u64;
            } else if c.is_ascii_lowercase() {
                num += 36 + (c as u8 - b'a') as u64;
            } else {
                return Err(EncodingError::InvalidBase62(c));
            }
        }
        Ok(num)
    }
}
