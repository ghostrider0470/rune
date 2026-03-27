---
name: code-review
namespace: rune.code-review
version: 0.1.0
kind: tool
description: Structured multi-dimensional code review engine with mechanical AST checks and semantic LLM review.
requires:
  - filesystem
  - git
tags:
  - code-review
  - security
  - performance
  - rust
triggers:
  - code review
  - review this PR
  - audit code
---

# Code Review Engine

Native Rust spell for structured code review across 7 dimensions: security, performance, correctness, maintainability, testing, accessibility, documentation.
Supports file, diff, and PR targets with syn-based mechanical checks for Rust and LLM-powered semantic analysis.
