//! LanceDB embedded vector store backend.
//!
//! Implements [`MemoryEmbeddingRepo`] and [`MemoryFactRepo`] using LanceDB
//! with Apache Arrow for data exchange.

mod memory;
mod memory_fact;

use std::sync::Arc;

use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Float64Array, Int32Array, RecordBatch,
    RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use lancedb::Connection;
use tracing::info;

use crate::error::StoreError;

/// Embedded LanceDB vector store.
#[derive(Clone)]
pub struct LanceStore {
    db: Connection,
    embedding_dims: i32,
}

impl LanceStore {
    /// Connect to a LanceDB instance and ensure required tables exist.
    pub async fn new(uri: &str, embedding_dims: i32) -> Result<Self, StoreError> {
        let db = lancedb::connect(uri)
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb connect: {e}")))?;

        let store = Self { db, embedding_dims };
        store.ensure_tables().await?;

        info!("lancedb vector store connected to {uri}");
        Ok(store)
    }

    /// Create tables if they don't exist.
    async fn ensure_tables(&self) -> Result<(), StoreError> {
        let existing = self
            .db
            .table_names()
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb list tables: {e}")))?;

        if !existing.iter().any(|n| n == "memory_embeddings") {
            self.db
                .create_empty_table("memory_embeddings", embeddings_schema(self.embedding_dims))
                .execute()
                .await
                .map_err(|e| StoreError::Database(format!("create memory_embeddings: {e}")))?;
            info!("created lancedb table 'memory_embeddings'");
        }

        if !existing.iter().any(|n| n == "memory_facts") {
            self.db
                .create_empty_table("memory_facts", facts_schema(self.embedding_dims))
                .execute()
                .await
                .map_err(|e| StoreError::Database(format!("create memory_facts: {e}")))?;
            info!("created lancedb table 'memory_facts'");
        }

        Ok(())
    }

    async fn open_embeddings_table(&self) -> Result<lancedb::Table, StoreError> {
        self.db
            .open_table("memory_embeddings")
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("open memory_embeddings: {e}")))
    }

    async fn open_facts_table(&self) -> Result<lancedb::Table, StoreError> {
        self.db
            .open_table("memory_facts")
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("open memory_facts: {e}")))
    }
}

// ── Arrow schemas ────────────────────────────────────────────────────

fn embedding_field(dims: i32) -> Field {
    Field::new(
        "embedding",
        DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), dims),
        false,
    )
}

pub(crate) fn embeddings_schema(dims: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("project_id", DataType::Utf8, true),
        Field::new("file_path", DataType::Utf8, false),
        Field::new("chunk_index", DataType::Int32, false),
        Field::new("chunk_text", DataType::Utf8, false),
        embedding_field(dims),
        Field::new("created_at", DataType::Utf8, false),
    ]))
}

pub(crate) fn facts_schema(dims: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("fact_id", DataType::Utf8, false),
        Field::new("fact", DataType::Utf8, false),
        Field::new("category", DataType::Utf8, false),
        embedding_field(dims),
        Field::new("source_session_id", DataType::Utf8, true),
        Field::new("created_at", DataType::Utf8, false),
        Field::new("updated_at", DataType::Utf8, false),
        Field::new("access_count", DataType::Int32, false),
    ]))
}

// ── Arrow extraction helpers ─────────────────────────────────────────

/// Collect all RecordBatches from a LanceDB query stream.
pub(crate) async fn collect_batches(
    stream: lancedb::arrow::SendableRecordBatchStream,
) -> Result<Vec<RecordBatch>, StoreError> {
    use futures::TryStreamExt;
    stream
        .try_collect()
        .await
        .map_err(|e| StoreError::Database(format!("lancedb query collect: {e}")))
}

/// Extract a string column from a RecordBatch.
pub(crate) fn str_col<'a>(batch: &'a RecordBatch, name: &str) -> &'a StringArray {
    batch
        .column_by_name(name)
        .expect("missing column")
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("not a string column")
}

/// Extract an Int32 column from a RecordBatch.
pub(crate) fn i32_col<'a>(batch: &'a RecordBatch, name: &str) -> &'a Int32Array {
    batch
        .column_by_name(name)
        .expect("missing column")
        .as_any()
        .downcast_ref::<Int32Array>()
        .expect("not an i32 column")
}

/// Extract a floating-point value from a column, accepting Float64 or Float32.
pub(crate) fn f64_value(batch: &RecordBatch, name: &str, row: usize) -> f64 {
    let column = batch.column_by_name(name).expect("missing column");

    if let Some(values) = column.as_any().downcast_ref::<Float64Array>() {
        return values.value(row);
    }

    if let Some(values) = column.as_any().downcast_ref::<Float32Array>() {
        return values.value(row) as f64;
    }

    panic!("column '{name}' is not a Float64 or Float32 column");
}

/// Extract the embedding FixedSizeList column and return each row as Vec<f32>.
pub(crate) fn embedding_col(batch: &RecordBatch, name: &str) -> Vec<Vec<f32>> {
    let list_arr = batch
        .column_by_name(name)
        .expect("missing column")
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .expect("not a FixedSizeList column");
    let values = list_arr
        .values()
        .as_any()
        .downcast_ref::<Float32Array>()
        .expect("inner array not Float32");
    let dim = list_arr.value_length() as usize;
    (0..list_arr.len())
        .map(|i| {
            let offset = i * dim;
            values.values()[offset..offset + dim].to_vec()
        })
        .collect()
}

/// Build a single-row FixedSizeListArray from a flat f32 slice.
fn make_embedding_list(embedding: &[f32], dims: i32) -> Result<FixedSizeListArray, StoreError> {
    let values = Float32Array::from(embedding.to_vec());
    let field = Arc::new(Field::new("item", DataType::Float32, true));
    FixedSizeListArray::try_new(field, dims, Arc::new(values), None)
        .map_err(|e| StoreError::Serialization(format!("embedding array: {e}")))
}

/// Build a single-row RecordBatch for a memory embedding.
#[allow(clippy::too_many_arguments)]
pub(crate) fn embedding_batch(
    schema: &Arc<Schema>,
    id: &str,
    project_id: Option<&str>,
    file_path: &str,
    chunk_index: i32,
    chunk_text: &str,
    embedding: &[f32],
    created_at: &str,
    dims: i32,
) -> Result<RecordBatch, StoreError> {
    let list = make_embedding_list(embedding, dims)?;

    RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(vec![id])),
            Arc::new(StringArray::from(vec![project_id])),
            Arc::new(StringArray::from(vec![file_path])),
            Arc::new(Int32Array::from(vec![chunk_index])),
            Arc::new(StringArray::from(vec![chunk_text])),
            Arc::new(list),
            Arc::new(StringArray::from(vec![created_at])),
        ],
    )
    .map_err(|e| StoreError::Serialization(format!("record batch: {e}")))
}

/// Build a single-row RecordBatch for a memory fact.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fact_batch(
    schema: &Arc<Schema>,
    id: &str,
    fact_id: &str,
    fact: &str,
    category: &str,
    embedding: &[f32],
    source_session_id: Option<&str>,
    created_at: &str,
    updated_at: &str,
    access_count: i32,
    dims: i32,
) -> Result<RecordBatch, StoreError> {
    let list = make_embedding_list(embedding, dims)?;

    let ssid: StringArray = match source_session_id {
        Some(s) => StringArray::from(vec![Some(s)]),
        None => StringArray::from(vec![Option::<&str>::None]),
    };

    RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(vec![id])),
            Arc::new(StringArray::from(vec![fact_id])),
            Arc::new(StringArray::from(vec![fact])),
            Arc::new(StringArray::from(vec![category])),
            Arc::new(list),
            Arc::new(ssid),
            Arc::new(StringArray::from(vec![created_at])),
            Arc::new(StringArray::from(vec![updated_at])),
            Arc::new(Int32Array::from(vec![access_count])),
        ],
    )
    .map_err(|e| StoreError::Serialization(format!("record batch: {e}")))
}

/// Parse an embedding string like "[0.1, 0.2, ...]" into Vec<f32>.
pub(crate) fn parse_embedding_str(s: &str) -> Result<Vec<f32>, StoreError> {
    s.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|v| {
            v.trim()
                .parse::<f32>()
                .map_err(|e| StoreError::Serialization(format!("bad embedding float: {e}")))
        })
        .collect()
}

/// Helper to add a single batch to a table via merge-insert (upsert on `id`).
pub(crate) async fn upsert_batch(
    table: &lancedb::Table,
    schema: &Arc<Schema>,
    batch: RecordBatch,
) -> Result<(), StoreError> {
    let reader = RecordBatchIterator::new(vec![Ok(batch)], schema.clone());
    let mut builder = table.merge_insert(&["id"]);
    builder
        .when_matched_update_all(None)
        .when_not_matched_insert_all();
    builder
        .execute(Box::new(reader))
        .await
        .map_err(|e| StoreError::Database(format!("lancedb upsert: {e}")))
}
