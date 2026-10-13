# Troubleshooting

- MCP 404 di ChatGPT: pastikan URL berakhir `/mcp`, HTTPS publik, bukan `http://localhost`.
- OAuth loop: cek `.well-known/oauth-protected-resource`, `code_challenge_methods_supported: [S256]`, `iss` echo, redirect allowlist.
- Agent offline: cek tray status, `OC_GATEWAY` WSS, firewall outbound 443, heartbeat 30s, online timeout 90s.
- Task `uncertain`: jangan retry otomatis; tanya owner, cek `task_status`, lalu putuskan.
- Rate 429: 60 calls/min/owner — tunggu + backoff eksponensial.
- 423 paused: Resume dari tray.
- Screenshot kosong: sesi desktop terkunci / UAC secure desktop — tidak didukung by design.
- Upgrade: pairing di PROGRAMDATA dipertahankan; uninstall hapus Run key + file, kredensial dicabut di gateway.
