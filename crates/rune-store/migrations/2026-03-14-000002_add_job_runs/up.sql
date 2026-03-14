CREATE TABLE job_runs (
    id          UUID PRIMARY KEY,
    job_id      UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    started_at  TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    status      TEXT NOT NULL,
    output      TEXT,
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_job_runs_job_started_at ON job_runs (job_id, started_at DESC);
