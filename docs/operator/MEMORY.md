# Memory

Rune's memory system provides both file-based workspace memory and semantic vector memory (Mem0) for cross-session recall.

## File-based memory

- **`MEMORY.md`** — curated long-term memory, loaded only in Direct (main) sessions
- **`memory/*.md`** — daily notes (`memory/YYYY-MM-DD.md`), today's and yesterday's loaded for all session types
- **`memory/lessons.md`** — persistent lessons, loaded in Direct sessions
- Privacy boundary: `MEMORY.md` is main-session-only

## Mem0 — semantic vector memory

Mem0 provides automatic cross-session memory using pgvector embeddings:

- **Recall**: before each turn, the user message is embedded and matched against stored facts via cosine similarity
- **Capture**: after each turn, an LLM extracts durable facts and stores them with embeddings
- **Deduplication**: new facts are checked against existing memories (cosine > 0.85 threshold) to avoid duplicates

### Configuration

```toml
[mem0]
enabled = true
postgres_url = "postgres://user:pass@host:5432/dbname?sslmode=require"
embedding_endpoint = "https://your-resource.openai.azure.com/openai/deployments/text-embedding-3-large/embeddings"
embedding_api_key = "your-key"
embedding_model = "text-embedding-3-large"
embedding_dims = 2000
api_version = "2024-02-01"
top_k = 10
similarity_threshold = 0.3
dedup_threshold = 0.85
```

### Embedding dimensions

The `embedding_dims` setting controls vector dimensionality throughout the system: the API request, table DDL, and validation. The table and HNSW index are auto-created on startup.

| Setup | Recommended `embedding_dims` | Why |
|---|---|---|
| **Azure Cosmos DB for PostgreSQL** | `2000` | pgvector 0.8.0 caps HNSW/IVFFlat indexes at 2000 dims |
| **Local/self-managed PostgreSQL** (pgvector >= 0.9.0) | `3072` | Full native dimensionality, HNSW supports up to 4000 dims |
| **Any PostgreSQL** (no index needed) | `3072` | Brute-force cosine scan works at any dimension, fast for <100k rows |

`text-embedding-3-large` natively supports dimension reduction via the API's `dimensions` parameter (Matryoshka embeddings). At 2000 dims it retains ~99.5% quality vs 3072.

### Local PostgreSQL setup

For full 3072-dim embeddings with HNSW index:

```bash
# Install pgvector >= 0.9.0
sudo apt install postgresql-16-pgvector

# Or from source
git clone https://github.com/pgvector/pgvector.git
cd pgvector && make && sudo make install
```

```toml
[mem0]
embedding_dims = 3072
postgres_url = "postgres://rune:password@localhost:5432/rune"
```

Rune auto-creates the `rune_memories` table and HNSW index on startup. To change dimensions after initial setup, drop the table and restart:

```sql
DROP TABLE rune_memories;
-- Rune recreates it on next startup with the configured dims
```

### Azure Cosmos DB for PostgreSQL

Cosmos DB uses the Citus extension. Rune handles the differences automatically:

- Uses `SELECT create_extension('vector')` instead of `CREATE EXTENSION` (required by Citus)
- Gracefully skips HNSW index when dims exceed the pgvector version's limit
- Tested with Cosmos DB PostgreSQL 16 / Citus 12.1 / pgvector 0.8.0

### Operator CLI

```
rune memory status    # show memory config and stats
rune memory search    # search memory files
rune memory get       # read a memory file
```

### Agent tools

The agent has these memory tools available:

- **`memory_search`** — search MEMORY.md and memory/*.md for relevant snippets
- **`memory_get`** — read a bounded snippet from memory files
- **`memory_write`** — append a note to today's daily memory file

Mem0 recall/capture happens automatically and is invisible to the agent's tool calls.

## Read next

- [`../parity/PARITY-INVENTORY.md`](../parity/PARITY-INVENTORY.md) — memory/tool surface inventory
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) — runtime semantics behind retrieval
- [`../FUNCTIONALITY-CHECKLIST.md`](../FUNCTIONALITY-CHECKLIST.md) — implementation status
