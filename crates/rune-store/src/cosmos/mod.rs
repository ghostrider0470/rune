//! Cosmos DB NoSQL backend for Rune.
//!
//! All document types are stored in a single container (`rune`) using a
//! synthetic `/pk` partition key and a `type` discriminator field.

mod approval;
mod device;
mod job;
mod job_run;
mod memory;
mod memory_fact;
mod process;
mod session;
mod tool_exec;
mod tool_policy;
mod transcript;
mod turn;

use azure_core::credentials::Secret;
use azure_data_cosmos::clients::ContainerClient;
use azure_data_cosmos::models::{
    ContainerProperties, IndexingPolicy, PartitionKeyDefinition, VectorEmbeddingPolicy,
};
use azure_data_cosmos::{CosmosAccountEndpoint, CosmosAccountReference, CosmosClient};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::de::DeserializeOwned;
use sha2::Sha256;
use tracing::info;

use crate::error::StoreError;

/// Cosmos DB-backed store. All repos share this single container client.
#[derive(Clone)]
pub struct CosmosStore {
    container: ContainerClient,
    endpoint: String,
    key: String,
}

impl CosmosStore {
    /// Connect to Cosmos DB and (optionally) ensure the database/container exist.
    pub async fn new(endpoint: &str, key: &str, run_migrations: bool) -> Result<Self, StoreError> {
        let endpoint_parsed: CosmosAccountEndpoint = endpoint
            .parse()
            .map_err(|e| StoreError::Database(format!("invalid cosmos endpoint: {e}")))?;
        let account =
            CosmosAccountReference::with_master_key(endpoint_parsed, Secret::from(key.to_string()));
        let client = CosmosClient::builder()
            .build(account)
            .await
            .map_err(|e| StoreError::Database(format!("cosmos client creation failed: {e}")))?;

        if run_migrations {
            ensure_database_and_container(&client).await?;
        }

        let db_client = client.database_client("rune");
        let container_client = db_client.container_client("rune").await;

        info!("cosmos store connected to {endpoint}");
        Ok(Self {
            container: container_client,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            key: key.to_string(),
        })
    }

    /// Return a reference to the underlying container client.
    pub fn container(&self) -> &ContainerClient {
        &self.container
    }

    /// Execute a cross-partition SQL query via the Cosmos REST API.
    ///
    /// The Cosmos gateway rejects naive cross-partition POSTs with a "first
    /// chance exception" 400.  The correct approach is to enumerate partition
    /// key ranges (`GET .../pkranges`) and query each range individually with
    /// the `x-ms-documentdb-partitionkeyrangeid` header, then merge results.
    pub async fn query_cross_partition<T: DeserializeOwned>(
        &self,
        sql: &str,
    ) -> Result<Vec<T>, StoreError> {
        let client = reqwest::Client::new();
        let resource_link = "dbs/rune/colls/rune";

        // Step 1: get partition key ranges
        let pk_ranges = self
            .get_partition_key_ranges(&client, resource_link)
            .await?;

        // Step 2: query each range, paginating with continuation
        let url = format!("{}/{}/docs", self.endpoint, resource_link);
        let mut all_results: Vec<T> = Vec::new();

        for range_id in &pk_ranges {
            let mut continuation: Option<String> = None;
            loop {
                let date = chrono::Utc::now()
                    .format("%a, %d %b %Y %H:%M:%S GMT")
                    .to_string();
                let auth_token =
                    generate_auth_token(&self.key, "post", "docs", resource_link, &date)?;

                let mut req = client
                    .post(&url)
                    .header("Authorization", &auth_token)
                    .header("x-ms-date", &date)
                    .header("x-ms-version", "2020-07-15")
                    .header("Content-Type", "application/query+json")
                    .header("x-ms-documentdb-isquery", "true")
                    .header("x-ms-documentdb-query-enablecrosspartition", "true")
                    .header("x-ms-documentdb-partitionkeyrangeid", range_id.as_str());

                if let Some(ref token) = continuation {
                    req = req.header("x-ms-continuation", token);
                }

                let body = serde_json::json!({ "query": sql });
                let resp = req.json(&body).send().await.map_err(|e| {
                    StoreError::Database(format!("cosmos REST request failed: {e}"))
                })?;

                let status = resp.status();
                let next_continuation = resp
                    .headers()
                    .get("x-ms-continuation")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());

                let resp_body: serde_json::Value = resp.json().await.map_err(|e| {
                    StoreError::Database(format!("cosmos REST response parse failed: {e}"))
                })?;

                if !status.is_success() {
                    let msg = resp_body
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    return Err(StoreError::Database(format!(
                        "cosmos REST query failed ({}): {}",
                        status, msg
                    )));
                }

                if let Some(docs_arr) = resp_body.get("Documents").and_then(|v| v.as_array()) {
                    for doc in docs_arr {
                        let item: T = serde_json::from_value(doc.clone())
                            .map_err(|e| StoreError::Serialization(e.to_string()))?;
                        all_results.push(item);
                    }
                }

                match next_continuation {
                    Some(token) if !token.is_empty() => continuation = Some(token),
                    _ => break,
                }
            }
        }

        Ok(all_results)
    }

    /// Fetch partition key range IDs for the container.
    async fn get_partition_key_ranges(
        &self,
        client: &reqwest::Client,
        resource_link: &str,
    ) -> Result<Vec<String>, StoreError> {
        let url = format!("{}/{}/pkranges", self.endpoint, resource_link);
        let date = chrono::Utc::now()
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        let auth_token = generate_auth_token(&self.key, "get", "pkranges", resource_link, &date)?;

        let resp = client
            .get(&url)
            .header("Authorization", &auth_token)
            .header("x-ms-date", &date)
            .header("x-ms-version", "2020-07-15")
            .send()
            .await
            .map_err(|e| StoreError::Database(format!("pkranges request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StoreError::Database(format!(
                "pkranges request failed ({}): {}",
                status, body
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| StoreError::Database(format!("pkranges response parse failed: {e}")))?;

        let ranges = body
            .get("PartitionKeyRanges")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                StoreError::Database("pkranges response missing PartitionKeyRanges".into())
            })?;

        let ids: Vec<String> = ranges
            .iter()
            .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();

        if ids.is_empty() {
            return Err(StoreError::Database("no partition key ranges found".into()));
        }

        Ok(ids)
    }
}

/// Generate a Cosmos DB HMAC-SHA256 authorization token for REST API calls.
fn generate_auth_token(
    key: &str,
    verb: &str,
    resource_type: &str,
    resource_link: &str,
    date: &str,
) -> Result<String, StoreError> {
    let decoded_key = base64::engine::general_purpose::STANDARD
        .decode(key)
        .map_err(|e| StoreError::Database(format!("invalid cosmos key: {e}")))?;

    let payload = format!(
        "{}\n{}\n{}\n{}\n\n",
        verb.to_lowercase(),
        resource_type.to_lowercase(),
        resource_link,
        date.to_lowercase(),
    );

    let mut mac = Hmac::<Sha256>::new_from_slice(&decoded_key)
        .map_err(|e| StoreError::Database(format!("hmac init failed: {e}")))?;
    mac.update(payload.as_bytes());
    let signature = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    let token = format!("type=master&ver=1.0&sig={}", signature);
    // Percent-encode the token for the Authorization header.
    let mut encoded = String::with_capacity(token.len() * 2);
    for byte in token.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    Ok(encoded)
}

/// Create the `rune` database and `rune` container if they do not already exist.
/// Ignores 409 Conflict (already exists).
async fn ensure_database_and_container(client: &CosmosClient) -> Result<(), StoreError> {
    // Create database -- ignore 409.
    match client.create_database("rune", None).await {
        Ok(_) => info!("created cosmos database 'rune'"),
        Err(e) if is_conflict(&e) => {
            info!("cosmos database 'rune' already exists");
        }
        Err(e) => return Err(StoreError::Database(format!("create database: {e}"))),
    }

    // Create container with /pk partition key, 3072-dim vector policy, and indexing.
    let db_client = client.database_client("rune");
    let pk_def = PartitionKeyDefinition::new(vec!["/pk".to_string()]);

    let vector_policy: VectorEmbeddingPolicy = serde_json::from_value(serde_json::json!({
        "vectorEmbeddings": [{
            "path": "/embedding",
            "dataType": "float32",
            "distanceFunction": "cosine",
            "dimensions": 3072
        }]
    }))
    .map_err(|e| StoreError::Serialization(e.to_string()))?;

    let indexing_policy: IndexingPolicy = serde_json::from_value(serde_json::json!({
        "indexingMode": "consistent",
        "automatic": true,
        "includedPaths": [{"path": "/*"}],
        "excludedPaths": [
            {"path": "/_etag/?"},
            {"path": "/embedding/*"}
        ],
        "vectorIndexes": [{"path": "/embedding", "type": "quantizedFlat"}]
    }))
    .map_err(|e| StoreError::Serialization(e.to_string()))?;

    let props = ContainerProperties::new("rune", pk_def)
        .with_vector_embedding_policy(vector_policy)
        .with_indexing_policy(indexing_policy);

    match db_client.create_container(props, None).await {
        Ok(_) => info!("created cosmos container 'rune'"),
        Err(e) if is_conflict(&e) => {
            info!("cosmos container 'rune' already exists");
        }
        Err(e) => return Err(StoreError::Database(format!("create container: {e}"))),
    }

    Ok(())
}

/// Check if an Azure SDK error is a 409 Conflict.
fn is_conflict(err: &azure_core::Error) -> bool {
    let msg = err.to_string();
    msg.contains("409") || msg.contains("Conflict")
}

/// Build a [`PartitionKey`] from an owned or borrowed string.
///
/// The Azure SDK's `From<&str>` impl requires `'static`, so for local
/// strings we go through `String → PartitionKey`.
pub(crate) fn pk(value: &str) -> azure_data_cosmos::PartitionKey {
    azure_data_cosmos::PartitionKey::from(value.to_string())
}

/// Deserialize a [`serde_json::Value`] into `T`.
pub(crate) fn parse_doc<T: DeserializeOwned>(doc: serde_json::Value) -> Result<T, StoreError> {
    serde_json::from_value(doc).map_err(|e| StoreError::Serialization(e.to_string()))
}

/// Drain a Cosmos DB `FeedItemIterator` into a `Vec<T>`.
///
/// The iterator yields individual deserialized items from the query result pages.
pub(crate) async fn collect_query<T: DeserializeOwned + Send + 'static>(
    stream: azure_data_cosmos::FeedItemIterator<serde_json::Value>,
) -> Result<Vec<T>, StoreError> {
    use futures::StreamExt;
    let mut results = Vec::new();
    futures::pin_mut!(stream);
    while let Some(item_result) = stream.next().await {
        let item = item_result?;
        results.push(parse_doc(item)?);
    }
    Ok(results)
}
