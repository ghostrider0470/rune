# Spell Inventory

This page is the operator-facing inventory of Rune-native spells that ship in the workspace today.

Spell manifests live under `crates/rune-spells/*/SPELL.md`. The inventory below tracks the spell package, the primary exposed tool or surface, and the current practical behavior operators should expect.

| Spell | Package | Primary surface | Current behavior |
| --- | --- | --- | --- |
| Security Audit | `rune-spells-security-audit` | `security_audit` tool | Runs a baseline host security audit covering listening ports, sensitive file permissions, SSH hardening, and firewall status. |
| Rust Patterns | `rune-spells-rust-patterns` | `rust_pattern`, `rust_pattern_validate` tools | Returns relevant Rust implementation patterns and validates common anti-patterns such as `unwrap()` in non-test code and blocking async sleeps. |
| Code Review | `rune-spells-code-review` | `code_review` tool | Performs structured code review for file, diff, or PR targets with Rust AST-based mechanical checks and semantic-review scaffolding. |
| Evolver | `rune-spells-evolver` | internal scaffold | Crate and manifest scaffold for the evolver spell lane; not yet exposed as a stable operator tool surface. |

## Notes

- `Code Review` is intentionally documented as partial: mechanical checks ship today, while semantic review is still scaffolded behind the same tool contract.
- `Evolver` is currently scaffold-only: the spell manifest exists, but the stable operator-facing tool surface is not exposed yet.
- The inventory only lists Rune-native spells that are part of the Rust workspace. Prompt-only skills under `.state/skills/` are tracked separately from this spell inventory.
