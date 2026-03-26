//! Cosmos DB NoSQL backend for Rune.
//!
//! All document types are stored in a single container (`rune`) using a
//! synthetic `/pk` partition key and a `type` discriminator field.

mod approval;
mod device;
mod job;
mod job_run;
mod memory;
mod process;
mod session;
mod tool_exec;
mod tool_policy;
mod transcript;
mod turn;

use azure_core::credentials::Secret;
use azure_data_cosmos::clients::ContainerClient;
use azure_data_cosmos::models::{ContainerProperties, PartitionKeyDefinition};
use azure_data_cosmos::{CosmosAccountEndpoint, CosmosAccountReference, CosmosClient};
use serde::de::DeserializeOwned;
use tracing::info;

use crate::error::StoreError;

/// Cosmos DB-backed store. All repos share this single container client.
#[derive(Clone)]
pub struct CosmosStore {
    container: ContainerClient,
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
        })
    }

    /// Return a reference to the underlying container client.
    pub fn container(&self) -> &ContainerClient {
        &self.container
    }
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

    // Create container with /pk partition key -- ignore 409.
    let db_client = client.database_client("rune");
    let pk_def = PartitionKeyDefinition::new(vec!["/pk".to_string()]);
    let props = ContainerProperties::new("rune", pk_def);

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
