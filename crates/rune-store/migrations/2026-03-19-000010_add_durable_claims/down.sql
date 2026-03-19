DROP INDEX IF EXISTS idx_jobs_due_unclaimed;
ALTER TABLE jobs DROP COLUMN claimed_at;
