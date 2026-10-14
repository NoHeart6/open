# ChatGPT OAuth scope fix — 14 October 2026

The browser reported `invalid_scope` for `openid offline_access computer:read` while adding OpenCommander as a custom MCP server. Both that request and the request including `computer:control` reproduced the failure against the deployed authorization server.

The realm already contained `offline_access`, but the ChatGPT metadata client did not assign it as an optional client scope. The explicitly registered `opencommander-chatgpt` client had the same omission. `openid` is the OIDC protocol marker; it does not need a custom realm scope.

`scripts/enable-chatgpt-offline-scope.ps1` saves an encrypted DPAPI snapshot, adds the existing optional scope to these two clients, checks the owner's offline access role and the scope's role mapping, and verifies that client authentication, consent, PKCE attributes, redirect allowlists, and existing default scopes remain unchanged. The deployment configuration and realm import now retain the scope assignment.

Verification: `scripts/test-chatgpt-client.ps1` checks ordinary login, the exact browser scope request, optional control scope, a forbidden callback, an unknown scope, and missing PKCE. Results are saved in `artifacts/chatgpt-client-results.json`. This verifies authorization request handling; the user's actual ChatGPT login, token exchange, and tool call remain pending.

Retry the custom MCP connection in ChatGPT. No ZIP replacement is required for this authorization-server fix. The earlier ZIP import failure was a separate HTTP 400 whose response body was unavailable; its root cause is still unverified.
