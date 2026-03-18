# Providers

This is the operator-facing entry doc for Rune model/provider setup.

## Current provider direction

Rune is explicitly Azure-oriented while still supporting broader provider abstraction.

Current provider-related reference surfaces:
- Azure AI Foundry / Azure OpenAI are first-class requirements
- OpenAI and Anthropic provider paths are part of the active runtime shape

## Current canonical references

Use these docs for the current contract picture:
- [`../AZURE-COMPATIBILITY.md`](../AZURE-COMPATIBILITY.md)
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md)
- [`../INDEX.md`](../INDEX.md)
- [`../../rune-plan.md`](../../rune-plan.md)

## What belongs here over time

This file should become the stable operator entry for:
- provider kinds and configuration expectations
- Azure-specific setup notes
- model routing/operator mental model
- links to deeper provider-specific docs if split later
