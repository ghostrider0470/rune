ALTER TABLE sessions
ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}'::jsonb;
