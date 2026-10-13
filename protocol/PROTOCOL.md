# OpenCommander Protocol v0.1

ChatGPT Chat → MCP (TypeScript, Streamable HTTP `/mcp`) → Gateway (Go, HTTPS+WSS) → Agent (Rust, outbound TLS) → Windows OS.

## Transport

- Agent → Gateway: outbound WSS `wss://<gateway>/agent/v1/connect?device_id=...`, TLS 1.2+, header `Authorization: Bearer <device_token>`.
- MCP → Gateway: HTTPS `POST /internal/v1/...`, header `Authorization: Bearer <owner_token>`, plus `X-Idempotency-Key` for mutating calls.
- ChatGPT → MCP: Streamable HTTP `POST /mcp`, OAuth 2.1 Bearer (aud = MCP resource URL, S256 PKCE, CIMD preferred).

No inbound ports on the PC. No relay proprietary. Agent always dials out.

## Agent frames (JSON, one per WS message)

```json
{"type":"hello","device_id":"dev_...","agent_version":"0.1.0","capabilities":["system_info","file_*","terminal_execute","process_*","app_launch","screenshot","windows","interact"]}
{"type":"heartbeat","device_id":"dev_...","ts":1699999999,"paused":false}
{"type":"task_result","task_id":"task_...","device_id":"dev_...","ok":true,"result":{},"error":null}
{"type":"task_ack","task_id":"task_...","device_id":"dev_...","state":"running"}
```

Gateway → Agent:

```json
{"type":"task_assign","task_id":"task_...","op":"system_info","params":{},"deadline_ms":30000,"idempotency_key":"..."}
{"type":"task_cancel","task_id":"task_..."}
{"type":"pairing_ack","owner_id":"..."}
{"type":"pause","paused":true}
```

## Task states

`queued → dispatched → running → (succeeded|failed|cancelled|uncertain)`.

- `uncertain`: agent lost contact after possible side effect. Never auto-retry without owner confirmation.
- Journal acknowledgement: agent must ack `running` before deadline; otherwise gateway marks `uncertain` on reconnect.

## Security

- Device identity: `device_id` + 256-bit `device_token` stored in Windows Credential Manager (DPAPI). Never hardcoded.
- Pairing: 6-digit code, 10 min TTL, single use, bound to `owner_id` after OAuth login.
- Owner isolation: every task carries `owner_id`; gateway rejects cross-owner routing.
- Replay protection: `nonce` + `ts` window 5 min on agent frames; `X-Idempotency-Key` on MCP→gateway writes.
- Rate limit: 60 tool calls/min/owner, 10 task dispatch/s/device.
