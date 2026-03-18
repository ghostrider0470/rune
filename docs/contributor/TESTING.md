# Testing

This is the contributor-facing entry doc for Rune validation flow.

## Current validation posture

Rune work uses tiered validation:
- focused local validation during implementation
- broader local validation before PR/merge
- CI as the final gate

## Current canonical references

- [`EXECUTION-SPEED-POLICY.md`](EXECUTION-SPEED-POLICY.md)
- [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md)
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md)
- [`DEVELOPMENT.md`](DEVELOPMENT.md)
- [`../INDEX.md`](../INDEX.md)

## Common checks

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Current contributor use

Use this doc as the testing entrypoint for:
- understanding the current validation posture
- navigating from general testing questions into parity and development references

## Coverage still expected later

This file can still grow into deeper reference for:
- test layers and expectations
- parity-oriented acceptance pointers
- focused vs broad validation guidance
- links to subsystem-specific validation docs if split later
