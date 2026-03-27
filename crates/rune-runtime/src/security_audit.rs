use std::path::Path;

use rune_spells_security_audit::{AuditReport, run_security_audit};

pub fn run_host_security_audit(target: Option<&Path>) -> AuditReport {
    run_security_audit(target)
}
