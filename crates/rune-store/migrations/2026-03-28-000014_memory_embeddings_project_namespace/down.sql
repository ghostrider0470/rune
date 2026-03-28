DROP INDEX IF EXISTS idx_memory_embeddings_project_file_path;
DROP INDEX IF EXISTS idx_memory_embeddings_project_id;
DROP INDEX IF EXISTS uq_memory_embeddings_project_file_chunk;
ALTER TABLE memory_embeddings DROP COLUMN IF EXISTS project_id;
CREATE UNIQUE INDEX IF NOT EXISTS memory_embeddings_file_path_chunk_index_key
    ON memory_embeddings (file_path, chunk_index);
