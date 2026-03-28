//! Backward-compatibility shim — re-exports from [`crate::spell`].
//!
//! The skill system has been renamed to "spells" (issue #299).
//! Prefer importing from `crate::spell` directly in new code.

pub use crate::spell::{
    Spell as Skill, SpellFrontmatter as SkillFrontmatter, SpellRegistry as SkillRegistry,
    parse_spell_frontmatter as parse_skill_frontmatter,
};
