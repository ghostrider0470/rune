## Blocked: Wave 0 validation cannot complete because Rust tooling is missing

**Agent:** workspace bootstrap
**Wave:** 0
**Issue:** The repository had no Cargo workspace or crate skeletons, so Wave 0 bootstrap was started and the full workspace skeleton was created manually. However, the required output gate in `docs/AGENT-ORCHESTRATION.md` says `cargo check` must succeed, and this host currently has no `cargo` or `rustc` installed, so that gate cannot be executed or verified.
**Options I see:**
- A. Install Rust toolchain on the host, then run `cargo check`, `cargo test`, and continue into Wave 1.
- B. Continue writing code/spec files without compile validation, accepting churn and avoidable breakage risk.
- C. Move validation to another build-capable environment and treat this host as planning/edit-only.
**My recommendation:** A. Install the Rust toolchain on this machine and keep the repo compiling at every wave gate. That matches the orchestration contract and avoids speculative unvalidated progress.
**Blocked on:** Rust tooling availability (`cargo`, `rustc`) for compile/test validation.
