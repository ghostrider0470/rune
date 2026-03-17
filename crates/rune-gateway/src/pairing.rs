//! Ed25519 device pairing with challenge-response flow backed by persistent storage.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use uuid::Uuid;

use rune_store::models::{NewPairedDevice, NewPairingRequest, PairedDeviceRow, PairingRequestRow};
use rune_store::repos::DeviceRepo;

/// Errors that can occur during pairing operations.
#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error("pairing request not found: {0}")]
    RequestNotFound(Uuid),

    #[error("pairing request expired: {0}")]
    RequestExpired(Uuid),

    #[error("device not found: {0}")]
    DeviceNotFound(Uuid),

    #[error("device_name must not be empty")]
    EmptyDeviceName,

    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),

    #[error("invalid signature: {0}")]
    InvalidSignature(String),

    #[error("challenge response verification failed")]
    VerificationFailed,

    #[error("a device with this public key is already paired")]
    DuplicatePublicKey,

    #[error("store error: {0}")]
    Store(String),
}

/// Role assigned to a paired device, governing API access scope.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Admin,
    Operator,
    ReadOnly,
}

impl DeviceRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Operator => "operator",
            Self::ReadOnly => "read_only",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "admin" => Self::Admin,
            "read_only" => Self::ReadOnly,
            _ => Self::Operator,
        }
    }
}

/// A device that has completed pairing and holds a valid bearer token.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: DeviceRole,
    pub scopes: Vec<String>,
    pub token: String,
    pub token_expires_at: DateTime<Utc>,
    pub paired_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// A paired device without the raw token.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredPairedDevice {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: DeviceRole,
    pub scopes: Vec<String>,
    pub token_hash: String,
    pub token_expires_at: DateTime<Utc>,
    pub paired_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// A pending pairing request awaiting operator approval.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairingRequest {
    pub id: Uuid,
    pub device_name: String,
    pub public_key: String,
    pub challenge: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Generate a 32-byte random challenge, returned as 64 hex characters.
fn generate_challenge() -> String {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let mut buf = Vec::with_capacity(32);
    buf.extend_from_slice(a.as_bytes());
    buf.extend_from_slice(b.as_bytes());
    hex::encode(&buf[..32])
}

/// Generate a 48-byte random bearer token, returned as 96 hex characters.
fn generate_token() -> String {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let c = Uuid::new_v4();
    let mut buf = Vec::with_capacity(48);
    buf.extend_from_slice(a.as_bytes());
    buf.extend_from_slice(b.as_bytes());
    buf.extend_from_slice(c.as_bytes());
    hex::encode(buf)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn decode_public_key(hex_key: &str) -> Result<VerifyingKey, PairingError> {
    let bytes = hex::decode(hex_key)
        .map_err(|e| PairingError::InvalidPublicKey(format!("hex decode: {e}")))?;
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| PairingError::InvalidPublicKey("expected 32 bytes".into()))?;
    VerifyingKey::from_bytes(&key_bytes)
        .map_err(|e| PairingError::InvalidPublicKey(format!("ed25519: {e}")))
}

fn verify_signature(
    public_key_hex: &str,
    message_hex: &str,
    signature_hex: &str,
) -> Result<(), PairingError> {
    let vk = decode_public_key(public_key_hex)?;
    let message = hex::decode(message_hex)
        .map_err(|e| PairingError::InvalidSignature(format!("message hex: {e}")))?;
    let sig_bytes = hex::decode(signature_hex)
        .map_err(|e| PairingError::InvalidSignature(format!("signature hex: {e}")))?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| PairingError::InvalidSignature("expected 64 bytes".into()))?;
    let signature = Signature::from_bytes(&sig_array);
    vk.verify(&message, &signature)
        .map_err(|_| PairingError::VerificationFailed)
}

fn default_scopes() -> Vec<String> {
    vec![
        "sessions:read".into(),
        "sessions:write".into(),
        "status:read".into(),
    ]
}

fn scopes_to_json(scopes: &[String]) -> serde_json::Value {
    serde_json::Value::Array(
        scopes
            .iter()
            .cloned()
            .map(serde_json::Value::String)
            .collect(),
    )
}

fn json_to_scopes(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.as_str().map(str::to_string))
        .collect()
}

fn map_request_row(row: PairingRequestRow) -> PairingRequest {
    PairingRequest {
        id: row.id,
        device_name: row.device_name,
        public_key: row.public_key,
        challenge: row.challenge,
        created_at: row.created_at,
        expires_at: row.expires_at,
    }
}

fn map_device_row(row: PairedDeviceRow) -> StoredPairedDevice {
    StoredPairedDevice {
        id: row.id,
        name: row.name,
        public_key: row.public_key,
        role: DeviceRole::parse(&row.role),
        scopes: json_to_scopes(&row.scopes),
        token_hash: row.token_hash,
        token_expires_at: row.token_expires_at,
        paired_at: row.paired_at,
        last_seen_at: row.last_seen_at,
    }
}

/// Persistent device registry.
#[derive(Clone)]
pub struct DeviceRegistry {
    repo: Arc<dyn DeviceRepo>,
}

impl std::fmt::Debug for DeviceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceRegistry").finish_non_exhaustive()
    }
}

impl DeviceRegistry {
    pub fn new(repo: Arc<dyn DeviceRepo>) -> Self {
        Self { repo }
    }

    pub async fn request_pairing(
        &self,
        device_name: String,
        public_key_hex: String,
    ) -> Result<PairingRequest, PairingError> {
        if device_name.trim().is_empty() {
            return Err(PairingError::EmptyDeviceName);
        }
        decode_public_key(&public_key_hex)?;

        if self
            .repo
            .find_device_by_public_key(&public_key_hex)
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?
            .is_some()
        {
            return Err(PairingError::DuplicatePublicKey);
        }

        let now = Utc::now();
        let row = NewPairingRequest {
            id: Uuid::now_v7(),
            device_name: device_name.clone(),
            public_key: public_key_hex,
            challenge: generate_challenge(),
            created_at: now,
            expires_at: now + Duration::minutes(5),
        };

        let created = self
            .repo
            .create_pairing_request(row)
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?;

        info!(request_id = %created.id, device_name = %device_name, "pairing request created");
        Ok(map_request_row(created))
    }

    pub async fn approve_pairing(
        &self,
        request_id: Uuid,
        challenge_response_hex: String,
        role: Option<DeviceRole>,
        scopes: Option<Vec<String>>,
    ) -> Result<PairedDevice, PairingError> {
        let request = self
            .repo
            .take_pairing_request(request_id)
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?
            .ok_or(PairingError::RequestNotFound(request_id))?;

        if Utc::now() > request.expires_at {
            warn!(request_id = %request_id, "pairing request expired");
            return Err(PairingError::RequestExpired(request_id));
        }

        if self
            .repo
            .find_device_by_public_key(&request.public_key)
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?
            .is_some()
        {
            return Err(PairingError::DuplicatePublicKey);
        }

        verify_signature(
            &request.public_key,
            &request.challenge,
            &challenge_response_hex,
        )?;

        let now = Utc::now();
        let token = generate_token();
        let token_hash = hash_token(&token);
        let resolved_role = role.unwrap_or(DeviceRole::Operator);
        let resolved_scopes = scopes.unwrap_or_else(default_scopes);

        let created = self
            .repo
            .create_device(NewPairedDevice {
                id: Uuid::now_v7(),
                name: request.device_name.clone(),
                public_key: request.public_key,
                role: resolved_role.as_str().to_string(),
                scopes: scopes_to_json(&resolved_scopes),
                token_hash,
                token_expires_at: now + Duration::days(30),
                paired_at: now,
                created_at: now,
            })
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?;

        info!(device_id = %created.id, device_name = %created.name, "device paired successfully");

        Ok(PairedDevice {
            id: created.id,
            name: created.name,
            public_key: created.public_key,
            role: DeviceRole::parse(&created.role),
            scopes: json_to_scopes(&created.scopes),
            token,
            token_expires_at: created.token_expires_at,
            paired_at: created.paired_at,
            last_seen_at: created.last_seen_at,
        })
    }

    pub async fn reject_pairing(&self, request_id: Uuid) -> Result<(), PairingError> {
        let deleted = self
            .repo
            .delete_pairing_request(request_id)
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?;
        if !deleted {
            return Err(PairingError::RequestNotFound(request_id));
        }
        info!(request_id = %request_id, "pairing request rejected");
        Ok(())
    }

    pub async fn list_pending(&self) -> Result<Vec<PairingRequest>, PairingError> {
        self.repo
            .list_pending_requests()
            .await
            .map(|rows| rows.into_iter().map(map_request_row).collect())
            .map_err(|e| PairingError::Store(e.to_string()))
    }

    pub async fn list_devices(&self) -> Result<Vec<StoredPairedDevice>, PairingError> {
        self.repo
            .list_devices()
            .await
            .map(|rows| rows.into_iter().map(map_device_row).collect())
            .map_err(|e| PairingError::Store(e.to_string()))
    }

    pub async fn revoke_device(&self, device_id: Uuid) -> Result<(), PairingError> {
        let deleted = self
            .repo
            .delete_device(device_id)
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?;
        if !deleted {
            return Err(PairingError::DeviceNotFound(device_id));
        }
        info!(device_id = %device_id, "device revoked");
        Ok(())
    }

    pub async fn rotate_token(&self, device_id: Uuid) -> Result<PairedDevice, PairingError> {
        let current = self
            .repo
            .find_device_by_id(device_id)
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?;
        let token = generate_token();
        let updated = self
            .repo
            .update_token(
                device_id,
                &hash_token(&token),
                Utc::now() + Duration::days(30),
            )
            .await
            .map_err(|e| PairingError::Store(e.to_string()))?;
        info!(device_id = %device_id, "device token rotated");
        Ok(PairedDevice {
            id: updated.id,
            name: updated.name,
            public_key: updated.public_key,
            role: DeviceRole::parse(&current.role),
            scopes: json_to_scopes(&current.scopes),
            token,
            token_expires_at: updated.token_expires_at,
            paired_at: updated.paired_at,
            last_seen_at: updated.last_seen_at,
        })
    }

    pub async fn validate_token(&self, token: &str) -> Option<StoredPairedDevice> {
        let token_hash = hash_token(token);
        let device = self
            .repo
            .find_device_by_token_hash(&token_hash)
            .await
            .ok()??;
        if Utc::now() > device.token_expires_at {
            return None;
        }
        let _ = self.repo.touch_last_seen(device.id, Utc::now()).await;
        Some(map_device_row(device))
    }

    pub async fn prune_expired_requests(&self) -> Result<usize, PairingError> {
        self.repo
            .prune_expired_requests()
            .await
            .map_err(|e| PairingError::Store(e.to_string()))
    }
}
