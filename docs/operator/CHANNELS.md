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

## Read next

- use [`../OPENCLAW-COVERAGE-MAP.md`](../OPENCLAW-COVERAGE-MAP.md) when you need the broad parity/docs navigation view by surface
- use [`../parity/PARITY-INVENTORY.md`](../parity/PARITY-INVENTORY.md) when you need command/surface coverage detail
- use [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) when you need deeper inbound/outbound runtime semantics

## Further detail still missing

Deeper follow-up documentation is still useful for:
- channel setup/navigation
- provider-specific channel docs
- runtime channel expectations
- health and troubleshooting pointers

## WebChat

Embedded WebChat is served by the gateway at `/webchat` with `/chat` redirecting to it.

Current behavior:
- Uses the gateway WebSocket endpoint at `/ws`
- Auth can be supplied by `api_key` query param or `Authorization`/subprotocol bearer token when gateway auth is enabled
- Browser-scoped routing uses `session_token` and maps to channel refs in the form `webchat:{session_token}`
- Anonymous browser access without a `session_token` maps to `webchat:anonymous`
- WebChat now resolves durable sessions instead of blindly creating a fresh direct session on each connect, so browser refresh/reconnect resumes the same conversation lane for that browser token
- `/webchat?...&session_id=<id>` can still force opening a specific known session in the UI

Operator notes:
- For multi-user usage, always issue distinct `session_token` values per browser/client
- If gateway auth is enabled, prefer short-lived links that carry `api_key` or terminate auth upstream and inject `Authorization`
- Active WebChat sessions appear in normal session listings with `channel_ref` values beginning with `webchat:`
