---
name: rust-patterns
namespace: rune.rust-patterns
version: 0.1.0
kind: tool
description: Rust-native pattern library spell for querying production Rust patterns by topic, tags, and file imports.
requires:
  - filesystem
tags:
  - rust
  - patterns
  - codegen
triggers:
  - rust pattern
  - rust best practice
  - rust example
---

# Rust Patterns

Native Rust spell that loads pattern references from TOML files and returns the most relevant matches for agent code generation.
