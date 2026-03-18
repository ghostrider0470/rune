ALTER TABLE tool_executions DROP COLUMN IF EXISTS execution_mode;
ALTER TABLE tool_executions DROP COLUMN IF EXISTS approval_id;
DROP TABLE IF EXISTS process_handles;
