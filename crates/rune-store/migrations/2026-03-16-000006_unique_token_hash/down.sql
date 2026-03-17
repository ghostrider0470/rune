ALTER TABLE paired_devices DROP CONSTRAINT IF EXISTS paired_devices_token_hash_key;
CREATE INDEX idx_paired_devices_token_hash ON paired_devices (token_hash);
