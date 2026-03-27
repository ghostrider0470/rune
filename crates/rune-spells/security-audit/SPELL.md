---
name: security-audit
namespace: rune.security-audit
version: 0.1.0
kind: tool
description: Native Rust spell for baseline host security auditing (open ports, sensitive file permissions, SSH config, firewall status).
requires:
  - filesystem
  - process
  - network
tags:
  - security
  - audit
  - hardening
triggers:
  - security audit
  - audit this machine
  - check firewall and ssh
---

# Security Audit

Native Rust spell that performs baseline host security checks and returns structured findings.
