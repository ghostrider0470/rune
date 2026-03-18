# Configuration

This is the operator-facing entry doc for Rune configuration behavior.

## Scope

Rune configuration covers:
- gateway settings
- model/provider configuration
- channel configuration
- auth and security-related settings
- storage/runtime path choices

## Current canonical references

Use these docs for the current contract picture:
- [`OPERATOR-POLICY.md`](OPERATOR-POLICY.md) — operator-side runtime rules and autonomy guidance
- [`DEPLOYMENT.md`](DEPLOYMENT.md) — deployment/storage/runtime layout expectations
- [`DATABASES.md`](DATABASES.md) — durable storage model and database choices
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) — config-related runtime/state concepts
- [`../INDEX.md`](../INDEX.md) — docs front door

## What belongs here over time

This file should become the stable operator reference entry for:
- config file shape and precedence
- secrets and env override guidance
- gateway/auth/runtime configuration pointers
- channel/model/provider configuration navigation
