-- Add execution-mode and tool-execution linkage to process handles.
ALTER TABLE process_handles ADD COLUMN execution_mode TEXT;
ALTER TABLE process_handles ADD COLUMN tool_execution_id UUID;
