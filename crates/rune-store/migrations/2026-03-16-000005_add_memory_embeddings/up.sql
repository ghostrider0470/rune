CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE memory_embeddings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_path TEXT NOT NULL,
    chunk_index INT NOT NULL,
    chunk_text TEXT NOT NULL,
    embedding vector(1536),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (file_path, chunk_index)
);

CREATE INDEX idx_memory_embedding ON memory_embeddings
    USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);

CREATE INDEX idx_memory_tsv ON memory_embeddings
    USING gin (to_tsvector('english', chunk_text));

CREATE INDEX idx_memory_embeddings_file_path ON memory_embeddings (file_path);
