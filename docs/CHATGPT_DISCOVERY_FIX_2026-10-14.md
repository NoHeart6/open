# ChatGPT discovery authentication fix — 14 October 2026

After successful browser OAuth login, ChatGPT reported `Authentication succeeded, action discovery failed`. The deployed MCP request log showed HTTP 401 responses. Effective token evaluation for the exact ChatGPT metadata client showed audience `opencommander-mcp`; the MCP verifier requires `https://mcp-production-394c.up.railway.app/mcp`.

The existing audience mapper used a Keycloak client identifier. Requests without a resource indicator, including offline refresh, did not retain the resource URI expected by the verifier. The previous production test always supplied a resource indicator and did not cover this path.

`scripts/fix-mcp-token-audience.ps1` saves a DPAPI encrypted mapper snapshot, then pins the audience to the exact configured MCP resource URI. It updates the shared basic scope used by the ChatGPT metadata client and the dedicated mapper used by the existing authorization-code test client. The verifier continues to enforce issuer, cryptographic signature, audience, expiry, subject, token identifier and read scope. Configuration scripts and the realm import preserve the corrected audience.

Verification:

- Four effective-token evaluations pass: both ChatGPT clients, with ordinary and offline scopes.
- `scripts/diagnose-chatgpt-integration.ps1` performs authorization-code login with PKCE and `openid offline_access computer:read` without supplying a resource indicator. It verifies the signed access token, rotates the offline refresh token without a resource indicator, verifies the refreshed token's audience and subject, initializes the actual public MCP endpoint, lists tools, and obtains system information from the connected Windows agent.
- The same diagnostic verifies that anonymous access, control operations with read-only credentials, an unowned device and malformed tool arguments are denied. Its diagnostic refresh token is revoked on exit. Sixteen checks pass; sanitized results are in `artifacts/chatgpt-integration-diagnostics.json`.

These are independent diagnostics, not ordinary ChatGPT host acceptance. Retry the custom MCP connection with a fresh OAuth login: already issued tokens keep their original audience. The ZIP import HTTP 400 remains a separate unverified issue.
