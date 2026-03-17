DROP INDEX IF EXISTS idx_paired_devices_token_hash;
ALTER TABLE paired_devices ADD CONSTRAINT paired_devices_token_hash_key UNIQUE (token_hash);
