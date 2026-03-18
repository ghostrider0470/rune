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

## Current operator use

Use this doc as the configuration entrypoint for:
- where config concerns live
- which deeper docs cover deployment/storage/runtime config semantics
- how config relates to provider, channel, and auth behavior

## Read next

- use [`DEPLOYMENT.md`](DEPLOYMENT.md) and [`DATABASES.md`](DATABASES.md) when configuration questions are really about runtime/storage layout
- use [`PROVIDERS.md`](PROVIDERS.md) and [`CHANNELS.md`](CHANNELS.md) when configuration questions are really about model or channel setup
- use [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) when you need deeper runtime semantics behind config behavior

## Further detail still missing

Deeper follow-up documentation is still useful for:
- config file shape and precedence
- secrets and env override guidance
- gateway/auth/runtime configuration pointers
- channel/model/provider configuration navigation
