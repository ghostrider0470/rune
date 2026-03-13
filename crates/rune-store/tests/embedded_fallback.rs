//! Embedded PostgreSQL fallback integration test.
//!
//! Enabled only when `RUNE_RUN_EMBEDDED_PG_TESTS=1` to avoid forcing
//! heavyweight Postgres bootstrap in every default test run.

use diesel_async::RunQueryDsl;
use rune_store::EmbeddedPg;
use rune_store::pool::{create_pool, run_migrations};
use uuid::Uuid;

#[tokio::test]
async fn embedded_pg_bootstrap_runs_migrations_and_accepts_connections() {
    if std::env::var("RUNE_RUN_EMBEDDED_PG_TESTS").as_deref() != Ok("1") {
        return;
    }

    let data_dir =
        std::env::temp_dir().join(format!("rune-store-embedded-test-{}", Uuid::now_v7()));

    let embedded = EmbeddedPg::start(&data_dir, "rune_test")
        .await
        .expect("embedded postgres should start");

    // Verifies migration runner works against embedded fallback.
    run_migrations(embedded.database_url()).expect("migrations should run on embedded postgres");
    // Idempotence check: second run should be a no-op and still succeed.
    run_migrations(embedded.database_url()).expect("second migration run should be idempotent");

    let pool = create_pool(embedded.database_url(), 2).expect("pool should be created");
    let mut conn = pool.get().await.expect("connection should be acquired");

    // Ensure database is queryable after bootstrap + migrations.
    diesel::sql_query("SELECT 1")
        .execute(&mut conn)
        .await
        .expect("embedded postgres should accept queries");

    embedded
        .stop()
        .await
        .expect("embedded postgres should stop cleanly");
    let _ = std::fs::remove_dir_all(&data_dir);
}
