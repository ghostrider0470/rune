ALTER TABLE jobs
ADD COLUMN payload_kind TEXT NOT NULL DEFAULT 'system_event';

ALTER TABLE jobs
ADD COLUMN delivery_mode TEXT NOT NULL DEFAULT 'none';

ALTER TABLE job_runs
ADD COLUMN trigger_kind TEXT NOT NULL DEFAULT 'due';

UPDATE jobs
SET payload_kind = CASE
    WHEN job_type = 'reminder' THEN 'reminder'
    WHEN payload ? 'payload' THEN COALESCE(payload->'payload'->>'kind', 'system_event')
    ELSE COALESCE(payload->>'kind', 'system_event')
END;

UPDATE jobs
SET delivery_mode = CASE
    WHEN job_type = 'reminder' THEN 'announce'
    ELSE delivery_mode
END;
