//! Integration tests for PostgreSQL-backed device pairing repositories.
//!
//! When `TEST_DATABASE_URL` is set the tests use that instance; otherwise an
//! embedded PostgreSQL server is started automatically.

use chrono::{Duration, Utc};
use uuid::Uuid;

use rune_store::embedded::EmbeddedPg;
use rune_store::models::{NewPairedDevice, NewPairingRequest};
use rune_store::pg::PgDeviceRepo;
use rune_store::pool::{PgPool, create_pool, run_migrations};
use rune_store::repos::DeviceRepo;

use std::sync::OnceLock;
use tokio::sync::{Mutex, OnceCell};

static EMBEDDED: OnceLock<OnceCell<Result<(EmbeddedPg, String), String>>> = OnceLock::new();
static SETUP_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

async fn database_url() -> Result<String, String> {
    if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        return Ok(url);
    }

    let cell = EMBEDDED.get_or_init(OnceCell::new);
    let result = cell
        .get_or_init(|| async {
            let tmp = std::env::temp_dir().join(format!("rune-device-test-pg-{}", Uuid::now_v7()));
            match EmbeddedPg::start(&tmp, "rune_device_test").await {
                Ok(epg) => {
                    let url = epg.database_url().to_string();
                    Ok((epg, url))
                }
                Err(err) => Err(format!(
                    "failed to start embedded PostgreSQL for device pairing tests: {err}"
                )),
            }
        })
        .await;

    result
        .as_ref()
        .map(|(_, url)| url.clone())
        .map_err(Clone::clone)
}

async fn setup() -> Option<PgPool> {
    let _guard = SETUP_LOCK.get_or_init(|| Mutex::new(())).lock().await;

    let url = match database_url().await {
        Ok(url) => url,
        Err(err) => {
            eprintln!("skipping rune-store device pairing integration test setup: {err}");
            return None;
        }
    };

    let pool = match create_pool(&url, 5) {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!(
                "skipping rune-store device pairing integration tests: pool creation failed: {err}"
            );
            return None;
        }
    };

    if let Err(err) = run_migrations(&pool).await {
        eprintln!("skipping rune-store device pairing integration tests: migrations failed: {err}");
        return None;
    }

    let conn = match pool.get().await {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!(
                "skipping rune-store device pairing integration tests: failed to get connection: {err}"
            );
            return None;
        }
    };

    if let Err(err) = conn.batch_execute(
        "TRUNCATE sessions, turns, transcript_items, jobs, approvals, \
         tool_executions, channel_deliveries, paired_devices, pairing_requests CASCADE",
    )
    .await
    {
        eprintln!("skipping rune-store device pairing integration tests: truncate failed: {err}");
        return None;
    }

    Some(pool)
}

#[tokio::test]
async fn device_repo_create_find_rotate_touch_and_delete() {
    let Some(pool) = setup().await else {
        return;
    };

    let repo = PgDeviceRepo::new(pool);
    let now = Utc::now();
    let device_id = Uuid::now_v7();

    let created = repo
        .create_device(NewPairedDevice {
            id: device_id,
            name: "ops-phone".into(),
            public_key: "a".repeat(64),
            role: "operator".into(),
            scopes: serde_json::json!(["sessions:read", "status:read"]),
            token_hash: "deadbeef".repeat(8),
            token_expires_at: now + Duration::days(30),
            paired_at: now,
            created_at: now,
        })
        .await
        .unwrap();

    assert_eq!(created.id, device_id);
    assert_eq!(created.name, "ops-phone");
    assert_eq!(created.role, "operator");

    let found = repo.find_device_by_id(device_id).await.unwrap();
    assert_eq!(found.public_key, "a".repeat(64));

    let by_hash = repo
        .find_device_by_token_hash(&("deadbeef".repeat(8)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_hash.id, device_id);

    let by_key = repo
        .find_device_by_public_key(&("a".repeat(64)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_key.id, device_id);

    let rotated = repo
        .update_token(device_id, &("feedface".repeat(8)), now + Duration::days(45))
        .await
        .unwrap();
    assert_eq!(rotated.token_hash, "feedface".repeat(8));

    let updated_role = repo
        .update_role(
            device_id,
            "admin",
            serde_json::json!(["sessions:read", "sessions:write", "status:read"]),
        )
        .await
        .unwrap();
    assert_eq!(updated_role.role, "admin");

    let touched_at = now + Duration::minutes(5);
    repo.touch_last_seen(device_id, touched_at).await.unwrap();
    let touched = repo.find_device_by_id(device_id).await.unwrap();
    let stored_last_seen = touched.last_seen_at.expect("last_seen_at should be set");
    assert!(
        (stored_last_seen - touched_at)
            .num_microseconds()
            .unwrap()
            .abs()
            <= 1
    );

    let listed = repo.list_devices().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, device_id);

    assert!(repo.delete_device(device_id).await.unwrap());
    assert!(
        repo.find_device_by_token_hash(&("feedface".repeat(8)))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn device_repo_pairing_request_take_and_prune() {
    let Some(pool) = setup().await else {
        return;
    };

    let repo = PgDeviceRepo::new(pool);
    let now = Utc::now();
    let expired_id = Uuid::now_v7();
    let fresh_id = Uuid::now_v7();
    let taken_id = Uuid::now_v7();

    repo.create_pairing_request(NewPairingRequest {
        id: expired_id,
        device_name: "expired".into(),
        public_key: "b".repeat(64),
        challenge: "1".repeat(64),
        created_at: now - Duration::minutes(10),
        expires_at: now - Duration::minutes(1),
    })
    .await
    .unwrap();

    repo.create_pairing_request(NewPairingRequest {
        id: fresh_id,
        device_name: "fresh".into(),
        public_key: "c".repeat(64),
        challenge: "2".repeat(64),
        created_at: now,
        expires_at: now + Duration::minutes(5),
    })
    .await
    .unwrap();

    repo.create_pairing_request(NewPairingRequest {
        id: taken_id,
        device_name: "take-me".into(),
        public_key: "d".repeat(64),
        challenge: "3".repeat(64),
        created_at: now,
        expires_at: now + Duration::minutes(5),
    })
    .await
    .unwrap();

    let pending_before_take = repo.list_pending_requests().await.unwrap();
    assert_eq!(pending_before_take.len(), 2);
    assert!(
        pending_before_take
            .iter()
            .all(|request| request.id != expired_id)
    );

    let taken = repo.take_pairing_request(taken_id).await.unwrap().unwrap();
    assert_eq!(taken.device_name, "take-me");
    assert!(repo.take_pairing_request(taken_id).await.unwrap().is_none());

    let pruned = repo.prune_expired_requests().await.unwrap();
    assert_eq!(pruned, 1);

    let pending_after_prune = repo.list_pending_requests().await.unwrap();
    assert_eq!(pending_after_prune.len(), 1);
    assert_eq!(pending_after_prune[0].id, fresh_id);

    assert!(repo.delete_pairing_request(fresh_id).await.unwrap());
    assert!(!repo.delete_pairing_request(fresh_id).await.unwrap());
}

#[tokio::test]
async fn device_repo_rejects_duplicate_public_keys() {
    let Some(pool) = setup().await else {
        return;
    };

    let repo = PgDeviceRepo::new(pool);
    let now = Utc::now();
    let public_key = "e".repeat(64);

    repo.create_device(NewPairedDevice {
        id: Uuid::now_v7(),
        name: "dup-a".into(),
        public_key: public_key.clone(),
        role: "operator".into(),
        scopes: serde_json::json!([]),
        token_hash: "9".repeat(64),
        token_expires_at: now + Duration::days(30),
        paired_at: now,
        created_at: now,
    })
    .await
    .unwrap();

    let duplicate = repo
        .create_device(NewPairedDevice {
            id: Uuid::now_v7(),
            name: "dup-b".into(),
            public_key,
            role: "operator".into(),
            scopes: serde_json::json!([]),
            token_hash: "8".repeat(64),
            token_expires_at: now + Duration::days(30),
            paired_at: now,
            created_at: now,
        })
        .await;

    assert!(matches!(
        duplicate,
        Err(rune_store::StoreError::Conflict(_))
    ));
}
