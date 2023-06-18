use std::io::Cursor;

use pgp::Deserializable;
use thiserror::Error;

pub enum SigningKeyBody {
    PGP(pgp::SignedPublicKey),
    SSH(ssh_key::PublicKey),
}

#[derive(Error, Debug)]
pub enum KeyError {
    #[error("Error while deserializing PGP key")]
    Pgp(#[from] pgp::errors::Error),
    #[error("Error while deserializing SSH key")]
    SshKey(#[from] ssh_key::Error),
    #[error("`{0}` is not a valid key type")]
    UnknownKeyType(String),
}

impl SigningKeyBody {
    pub fn parse(
        key_type: &str,
        body: &str,
    ) -> Result<SigningKeyBody, KeyError> {
        match key_type {
            "pgp" => {
                let cursor = Cursor::new(key_type.as_bytes());
                let key = pgp::SignedPublicKey::from_armor_single(cursor)?.0;
                Ok(SigningKeyBody::PGP(key))
            }
            "openssh" => {
                let key = ssh_key::PublicKey::from_openssh(&body)?;
                Ok(SigningKeyBody::SSH(key))
            }
            _ => Err(KeyError::UnknownKeyType(key_type.to_string())),
        }
    }

    pub fn scrub(&mut self) {
        match self {
            SigningKeyBody::PGP(key) => {
                key.details.direct_signatures = vec![];
            }
            SigningKeyBody::SSH(key) => key.set_comment(""),
        }
    }

    pub fn type_str(&self) -> &'static str {
        match self {
            SigningKeyBody::PGP(..) => "pgp",
            SigningKeyBody::SSH(..) => "openssh",
        }
    }

    pub fn to_body(&self) -> String {
        match self {
            SigningKeyBody::PGP(key) => {
                key.to_armored_string(None).expect("PGP key writing failed")
            }
            SigningKeyBody::SSH(key) => key.to_string(),
        }
    }
}
