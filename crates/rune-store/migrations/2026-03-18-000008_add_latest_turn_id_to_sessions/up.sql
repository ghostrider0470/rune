-- Add latest_turn_id to sessions for quick access to the most recent turn.
ALTER TABLE sessions ADD COLUMN latest_turn_id UUID;
