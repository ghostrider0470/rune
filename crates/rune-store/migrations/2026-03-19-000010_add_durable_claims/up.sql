-- Durable claim/lease column for preventing duplicate job execution.
ALTER TABLE jobs ADD COLUMN claimed_at TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS idx_jobs_due_unclaimed
    ON jobs (enabled, next_run_at)
    WHERE claimed_at IS NULL;
