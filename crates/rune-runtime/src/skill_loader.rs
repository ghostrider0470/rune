//! Backward-compatibility shim — re-exports from [`crate::spell_loader`].
//!
//! The skill system has been renamed to "spells" (issue #299).
//! Prefer importing from `crate::spell_loader` directly in new code.

pub use crate::spell_loader::{SpellLoader as SkillLoader, SpellScanSummary as SkillScanSummary};
