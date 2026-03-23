-- Add runtime_profile and policy_profile to sessions per PROTOCOLS.md 4.1.
ALTER TABLE sessions ADD COLUMN runtime_profile TEXT;
ALTER TABLE sessions ADD COLUMN policy_profile TEXT;
