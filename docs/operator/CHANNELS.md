# Channels

This is the operator-facing entry doc for channel adapters and messaging-surface setup.

## Scope

Rune channels are responsible for:
- inbound normalization
- outbound delivery
- reply/reaction/media semantics
- adapter-specific setup and health expectations

## Current canonical references

Use these docs for the current contract picture:
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md)
- [`../parity/PARITY-INVENTORY.md`](../parity/PARITY-INVENTORY.md)
- [`../OPENCLAW-COVERAGE-MAP.md`](../OPENCLAW-COVERAGE-MAP.md)
- [`../INDEX.md`](../INDEX.md)

## Current operator use

Use this doc as the channel entrypoint for:
- understanding where channel coverage and behavior docs live
- navigating from adapter questions into parity and protocol references

## Next depth to add

This file can still grow into deeper reference for:
- channel setup/navigation
- provider-specific channel docs
- runtime channel expectations
- health and troubleshooting pointers
