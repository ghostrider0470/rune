-- up
ALTER TABLE memory_embeddings ADD COLUMN IF NOT EXISTS project_id TEXT;

ALTER TABLE memory_embeddings DROP CONSTRAINT IF EXISTS memory_embeddings_file_path_chunk_index_key;
CREATE UNIQUE INDEX IF NOT EXISTS uq_memory_embeddings_project_file_chunk
    ON memory_embeddings (COALESCE(project_id, ''), file_path, chunk_index);
CREATE INDEX IF NOT EXISTS idx_memory_embeddings_project_id ON memory_embeddings (project_id);
CREATE INDEX IF NOT EXISTS idx_memory_embeddings_project_file_path ON memory_embeddings (project_id, file_path);
