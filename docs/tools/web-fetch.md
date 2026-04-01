# web_fetch

`web_fetch` performs approved HTTP GET/POST requests and returns status, selected headers, and a truncated response body.

## Stateful browsing

The tool now supports lightweight stateful flows without shell-side cookie hacks:

- `session`: logical session key used to reuse request state across related calls
- `persist_session`: whether the request updates stored session state
- `redirect_policy`: `follow` (default), `manual`, or `error`

When a session is enabled, Rune can persist:

- redacted request headers previously attached to the session
- merged cookie state derived from `Set-Cookie` responses

Sensitive request headers such as `authorization` and `cookie` stay redacted in tool output.

## Redirect policies

- `follow`: follows redirects automatically up to the client limit
- `manual`: returns the redirect response so the model can inspect `location`
- `error`: fails immediately if a redirect is encountered

Use `manual` for login handshakes or multi-step flows where the model should inspect redirect targets before continuing.

## Example

```json
{
  "url": "https://example.com/login",
  "method": "POST",
  "headers": {
    "content-type": "application/json"
  },
  "body": "{\"username\":\"demo\"}",
  "session": "example-login",
  "redirect_policy": "manual"
}
```

Follow-up requests can reuse the same session key:

```json
{
  "url": "https://example.com/account",
  "session": "example-login"
}
```

## Safety defaults

- requests still require approval
- response bodies remain truncated to 50 KB
- redirects default to `follow` for compatibility
- cookie/header persistence only happens when `session` is provided
