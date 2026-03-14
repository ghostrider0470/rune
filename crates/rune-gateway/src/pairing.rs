//! Ed25519 device pairing with challenge-response flow.
//!
//! Devices initiate pairing by submitting a name and Ed25519 public key.
//! The registry issues a random challenge nonce which the device must sign
//! with its private key.  Upon successful verification the device receives
//! a bearer token scoped to its assigned role.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

// ── Error type ───────────────────────────────────────────────────────────────

/// Errors that can occur during pairing operations.
#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error("pairing request not found: {0}")]
    RequestNotFound(Uuid),

    #[error("pairing request expired: {0}")]
    RequestExpired(Uuid),

    #[error("device not found: {0}")]
    DeviceNotFound(Uuid),

    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),

    #[error("invalid signature: {0}")]
    InvalidSignature(String),

    #[error("challenge response verification failed")]
    VerificationFailed,
}

// ── Domain types ─────────────────────────────────────────────────────────────

/// Role assigned to a paired device, governing API access scope.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Admin,
    Operator,
    ReadOnly,
}

/// A device that has completed pairing and holds a valid bearer token.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: Uuid,
    pub name: String,
    /// Hex-encoded Ed25519 public key (64 hex chars = 32 bytes).
    pub public_key: String,
    pub role: DeviceRole,
    /// Fine-grained scopes the device is allowed to exercise.
    pub scopes: Vec<String>,
    /// Bearer token issued upon pairing.  Only returned in full at
    /// pairing/rotation time; list endpoints mask it.
    pub token: String,
    pub token_expires_at: DateTime<Utc>,
    pub paired_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// A pending pairing request awaiting operator approval.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairingRequest {
    pub id: Uuid,
    pub device_name: String,
    /// Hex-encoded Ed25519 public key supplied by the device.
    pub public_key: String,
    /// Hex-encoded random challenge nonce (32 bytes = 64 hex chars).
    pub challenge: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Generate a 32-byte random challenge, returned as 64 hex characters.
///
/// Uses three concatenated UUIDv4 values (48 random bytes) and truncates
/// to 32 bytes so the result is always exactly 64 hex chars.
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

/// Decode a hex-encoded Ed25519 public key (64 hex chars) into a
/// [`VerifyingKey`].
fn decode_public_key(hex_key: &str) -> Result<VerifyingKey, PairingError> {
    let bytes = hex::decode(hex_key)
        .map_err(|e| PairingError::InvalidPublicKey(format!("hex decode: {e}")))?;
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| PairingError::InvalidPublicKey("expected 32 bytes".into()))?;
    VerifyingKey::from_bytes(&key_bytes)
        .map_err(|e| PairingError::InvalidPublicKey(format!("ed25519: {e}")))
}

/// Verify an Ed25519 signature over the given message.
///
/// * `public_key_hex` - 64 hex chars (32 bytes) Ed25519 verifying key
/// * `message_hex`    - hex-encoded message that was signed
/// * `signature_hex`  - 128 hex chars (64 bytes) Ed25519 signature
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

// ── Device Registry ──────────────────────────────────────────────────────────

/// Thread-safe, in-memory registry of paired devices and pending pairing
/// requests.
#[derive(Debug)]
pub struct DeviceRegistry {
    devices: RwLock<HashMap<Uuid, PairedDevice>>,
    pending: RwLock<HashMap<Uuid, PairingRequest>>,
}

impl DeviceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
        }
    }

    // ── Pairing flow ─────────────────────────────────────────────────────

    /// Initiate a pairing request.
    ///
    /// The caller supplies the device name and its Ed25519 public key
    /// (hex-encoded, 64 chars).  The registry validates the key format,
    /// generates a random challenge nonce, and returns the pending request
    /// which expires after 5 minutes.
    pub async fn request_pairing(
        &self,
        device_name: String,
        public_key_hex: String,
    ) -> Result<PairingRequest, PairingError> {
        // Validate key format early so callers get immediate feedback.
        decode_public_key(&public_key_hex)?;

        let now = Utc::now();
        let request = PairingRequest {
            id: Uuid::now_v7(),
            device_name: device_name.clone(),
            public_key: public_key_hex,
            challenge: generate_challenge(),
            created_at: now,
            expires_at: now + Duration::minutes(5),
        };

        info!(
            request_id = %request.id,
            device_name = %device_name,
            "pairing request created"
        );

        self.pending
            .write()
            .await
            .insert(request.id, request.clone());
        Ok(request)
    }

    /// Approve a pending pairing request.
    ///
    /// The device must supply the Ed25519 signature (hex-encoded, 128 chars)
    /// of the original challenge nonce.  The registry verifies the signature
    /// using the public key attached to the request, then issues a bearer
    /// token valid for 30 days.
    ///
    /// The request is consumed regardless of outcome.
    pub async fn approve_pairing(
        &self,
        request_id: Uuid,
        challenge_response_hex: String,
    ) -> Result<PairedDevice, PairingError> {
        let request = self
            .pending
            .write()
            .await
            .remove(&request_id)
            .ok_or(PairingError::RequestNotFound(request_id))?;

        // Check expiry.
        if Utc::now() > request.expires_at {
            warn!(request_id = %request_id, "pairing request expired");
            return Err(PairingError::RequestExpired(request_id));
        }

        // Verify the Ed25519 signature of the challenge.
        verify_signature(
            &request.public_key,
            &request.challenge,
            &challenge_response_hex,
        )?;

        let now = Utc::now();
        let device = PairedDevice {
            id: Uuid::now_v7(),
            name: request.device_name.clone(),
            public_key: request.public_key,
            role: DeviceRole::Operator, // default; promote via separate API
            scopes: vec![
                "sessions:read".into(),
                "sessions:write".into(),
                "status:read".into(),
            ],
            token: generate_token(),
            token_expires_at: now + Duration::days(30),
            paired_at: now,
            last_seen_at: None,
        };

        info!(
            device_id = %device.id,
            device_name = %device.name,
            "device paired successfully"
        );

        self.devices.write().await.insert(device.id, device.clone());
        Ok(device)
    }

    /// Reject and remove a pending pairing request.
    pub async fn reject_pairing(&self, request_id: Uuid) -> Result<(), PairingError> {
        self.pending
            .write()
            .await
            .remove(&request_id)
            .ok_or(PairingError::RequestNotFound(request_id))?;

        info!(request_id = %request_id, "pairing request rejected");
        Ok(())
    }

    /// List all pending pairing requests.
    pub async fn list_pending(&self) -> Vec<PairingRequest> {
        self.pending.read().await.values().cloned().collect()
    }

    // ── Device management ────────────────────────────────────────────────

    /// List all paired devices.  Tokens are **not** masked here; the route
    /// handler is responsible for redacting before serialisation.
    pub async fn list_devices(&self) -> Vec<PairedDevice> {
        self.devices.read().await.values().cloned().collect()
    }

    /// Remove a paired device, effectively revoking its access.
    pub async fn revoke_device(&self, device_id: Uuid) -> Result<(), PairingError> {
        self.devices
            .write()
            .await
            .remove(&device_id)
            .ok_or(PairingError::DeviceNotFound(device_id))?;

        info!(device_id = %device_id, "device revoked");
        Ok(())
    }

    /// Rotate the bearer token for a device, returning the updated entry
    /// with the new (full) token.  The old token is immediately invalidated.
    pub async fn rotate_token(&self, device_id: Uuid) -> Result<PairedDevice, PairingError> {
        let mut devices = self.devices.write().await;
        let device = devices
            .get_mut(&device_id)
            .ok_or(PairingError::DeviceNotFound(device_id))?;

        device.token = generate_token();
        device.token_expires_at = Utc::now() + Duration::days(30);

        info!(device_id = %device_id, "device token rotated");
        Ok(device.clone())
    }

    /// Validate a bearer token and return the associated device if valid
    /// and not expired.  Also updates `last_seen_at`.
    pub async fn validate_token(&self, token: &str) -> Option<PairedDevice> {
        let mut devices = self.devices.write().await;
        let device = devices.values_mut().find(|d| d.token == token)?;

        if Utc::now() > device.token_expires_at {
            return None;
        }

        device.last_seen_at = Some(Utc::now());
        Some(device.clone())
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// End-to-end pairing: request -> sign challenge -> approve.
    #[tokio::test]
    async fn full_pairing_flow() {
        let registry = DeviceRegistry::new();

        // Generate a keypair for the device.
        let signing_key = SigningKey::from_bytes(&{
            let mut seed = [0u8; 32];
            // Use a deterministic seed for reproducibility.
            seed[0] = 42;
            seed[1] = 7;
            seed
        });
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.as_bytes());

        // Step 1: request pairing.
        let req = registry
            .request_pairing("test-phone".into(), public_key_hex.clone())
            .await
            .expect("request_pairing failed");

        assert_eq!(req.device_name, "test-phone");
        assert_eq!(req.challenge.len(), 64); // 32 bytes hex

        // Step 2: device signs the challenge.
        let challenge_bytes = hex::decode(&req.challenge).unwrap();
        let signature = signing_key.sign(&challenge_bytes);
        let signature_hex = hex::encode(signature.to_bytes());

        // Step 3: approve with the signed challenge.
        let device = registry
            .approve_pairing(req.id, signature_hex)
            .await
            .expect("approve_pairing failed");

        assert_eq!(device.name, "test-phone");
        assert_eq!(device.role, DeviceRole::Operator);
        assert_eq!(device.token.len(), 96); // 48 bytes hex

        // Validate token.
        let found = registry.validate_token(&device.token).await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, device.id);
    }

    /// Approval with wrong signature must fail.
    #[tokio::test]
    async fn wrong_signature_rejected() {
        let registry = DeviceRegistry::new();

        let signing_key = SigningKey::from_bytes(&[1u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.as_bytes());

        let req = registry
            .request_pairing("bad-device".into(), public_key_hex)
            .await
            .unwrap();

        // Supply garbage signature.
        let bad_sig = "aa".repeat(64); // 64 bytes hex = 128 chars
        let result = registry.approve_pairing(req.id, bad_sig).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PairingError::VerificationFailed
        ));
    }

    /// Reject removes the pending request.
    #[tokio::test]
    async fn reject_removes_request() {
        let registry = DeviceRegistry::new();

        let signing_key = SigningKey::from_bytes(&[2u8; 32]);
        let public_key_hex = hex::encode(signing_key.verifying_key().as_bytes());

        let req = registry
            .request_pairing("reject-me".into(), public_key_hex)
            .await
            .unwrap();

        registry.reject_pairing(req.id).await.unwrap();

        // Second reject should fail — request already consumed.
        let result = registry.reject_pairing(req.id).await;
        assert!(matches!(
            result.unwrap_err(),
            PairingError::RequestNotFound(_)
        ));
    }

    /// Revoke removes a device and invalidates its token.
    #[tokio::test]
    async fn revoke_invalidates_token() {
        let registry = DeviceRegistry::new();

        let signing_key = SigningKey::from_bytes(&[3u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.as_bytes());

        let req = registry
            .request_pairing("revoke-me".into(), public_key_hex)
            .await
            .unwrap();

        let challenge_bytes = hex::decode(&req.challenge).unwrap();
        let sig = signing_key.sign(&challenge_bytes);

        let device = registry
            .approve_pairing(req.id, hex::encode(sig.to_bytes()))
            .await
            .unwrap();

        // Token is valid.
        assert!(registry.validate_token(&device.token).await.is_some());

        // Revoke.
        registry.revoke_device(device.id).await.unwrap();

        // Token is no longer valid.
        assert!(registry.validate_token(&device.token).await.is_none());
    }

    /// Token rotation issues a new token and invalidates the old one.
    #[tokio::test]
    async fn token_rotation() {
        let registry = DeviceRegistry::new();

        let signing_key = SigningKey::from_bytes(&[4u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.as_bytes());

        let req = registry
            .request_pairing("rotate-me".into(), public_key_hex)
            .await
            .unwrap();

        let challenge_bytes = hex::decode(&req.challenge).unwrap();
        let sig = signing_key.sign(&challenge_bytes);

        let device = registry
            .approve_pairing(req.id, hex::encode(sig.to_bytes()))
            .await
            .unwrap();

        let old_token = device.token.clone();

        let rotated = registry.rotate_token(device.id).await.unwrap();
        assert_ne!(rotated.token, old_token);

        // Old token invalid, new one valid.
        assert!(registry.validate_token(&old_token).await.is_none());
        assert!(registry.validate_token(&rotated.token).await.is_some());
    }

    #[tokio::test]
    async fn invalid_public_key_rejected() {
        let registry = DeviceRegistry::new();
        let result = registry
            .request_pairing("bad-key".into(), "not-hex".into())
            .await;
        assert!(matches!(
            result.unwrap_err(),
            PairingError::InvalidPublicKey(_)
        ));
    }

    #[tokio::test]
    async fn list_devices_and_pending() {
        let registry = DeviceRegistry::new();

        let signing_key = SigningKey::from_bytes(&[5u8; 32]);
        let public_key_hex = hex::encode(signing_key.verifying_key().as_bytes());

        // Create two pending requests.
        let _r1 = registry
            .request_pairing("dev-a".into(), public_key_hex.clone())
            .await
            .unwrap();
        let r2 = registry
            .request_pairing("dev-b".into(), public_key_hex.clone())
            .await
            .unwrap();

        assert_eq!(registry.list_pending().await.len(), 2);

        // Approve one.
        let challenge_bytes = hex::decode(&r2.challenge).unwrap();
        let sig = signing_key.sign(&challenge_bytes);
        registry
            .approve_pairing(r2.id, hex::encode(sig.to_bytes()))
            .await
            .unwrap();

        assert_eq!(registry.list_pending().await.len(), 1);
        assert_eq!(registry.list_devices().await.len(), 1);
    }
}
