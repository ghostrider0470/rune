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

## Definition of done for validation

Validation is part of the shipped slice, not a private local checkpoint. Before a PR is merged, the work should include:
- code changes required by the issue
- validation evidence proportionate to the risk and surface area
- canonical documentation updates whenever the change affects operator behavior, contributor workflow, runtime semantics, configuration, troubleshooting, or architecture

A change is not done if tests passed but the durable docs for the new behavior are still missing.

## Common checks

```bash
cargo check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Current contributor use

Use this doc as the testing entrypoint for:
- understanding the current validation posture
- navigating from general testing questions into parity and development references

## Read next

- use [`DEVELOPMENT.md`](DEVELOPMENT.md) when the testing question is really about local build/run setup
- use [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md) and [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) when the test question is really about runtime invariants
- use [`EXECUTION-SPEED-POLICY.md`](EXECUTION-SPEED-POLICY.md) when you need focused-vs-broad validation expectations during active work

## Further detail still missing

Deeper follow-up documentation is still useful for:
- test layers and expectations
- parity-oriented acceptance pointers
- focused vs broad validation guidance
- links to subsystem-specific validation docs if split later
