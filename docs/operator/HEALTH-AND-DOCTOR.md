# Health and Doctor

This is the operator-facing entry doc for runtime health, diagnostics, and doctor-style checks.

## Scope

Operator-visible runtime health includes:
- gateway health/status surfaces
- startup/runtime diagnostics
- dependency-state visibility
- troubleshooting entrypoints

## Current canonical references

Use these docs for the current contract picture:
- [`OPERATOR-POLICY.md`](OPERATOR-POLICY.md)
- [`DEPLOYMENT.md`](DEPLOYMENT.md)
- [`DATABASES.md`](DATABASES.md)
- [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md)
- [`../INDEX.md`](../INDEX.md)

## Current operator use

Use this doc as the health/diagnostics entrypoint for:
- where health/status expectations live now
- how to navigate to deployment, database, and parity contract troubleshooting references

## Next depth to add

This file can still grow into deeper reference for:
- health/status endpoint pointers
- doctor command/navigation
- common failure-mode triage
- dependency troubleshooting flow
