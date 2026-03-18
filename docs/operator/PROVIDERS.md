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

## Current operator use

Use this doc as the provider entrypoint for:
- where provider setup and Azure-oriented expectations live
- how to navigate from high-level provider questions into the deeper compatibility/runtime docs

## Next depth to add

This file can still grow into deeper reference for:
- provider kinds and configuration expectations
- Azure-specific setup notes
- model routing/operator mental model
- links to deeper provider-specific docs if split later
