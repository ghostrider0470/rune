/// Operator-visible restart continuity contract for channel sessions.
///
/// This is intentionally explicit and narrow: only behaviors that are
/// implemented and covered by tests should be described here.
pub const RESTART_CONTINUITY_SUMMARY: &str =
    "restart continuity is intentionally partial: durable approval requests, operator-triggered resume, resumed-session notification, and session/transcript restoration are supported across restart; in-flight turns, live process handles, and typing/progress UI do not resume in place";

pub const RESUMED_SESSION_NOTICE_TEMPLATE: &str =
    "Resumed session `{session_id}` after restart. Transcript history plus durable approval/session state were restored from the last saved activity. In-flight turns, live process handles, and typing/progress UI do not resume in place; send your next message to continue.";
