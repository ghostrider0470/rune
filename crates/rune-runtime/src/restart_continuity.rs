/// Operator-visible restart continuity contract for channel sessions.
///
/// This is intentionally explicit and narrow: only behaviors that are
/// implemented and covered by tests should be described here.
pub const RESTART_CONTINUITY_SUMMARY: &str = "approval requests, operator-triggered resume, and restart-safe mid-resume continuation are durable";

pub const RESUMED_SESSION_NOTICE_TEMPLATE: &str = "Resumed session `{session_id}` after restart. Transcript history plus durable approval/session state were restored from the last saved activity. In-flight turns, live process handles, and typing/progress UI do not resume in place; send your next message to continue.";
