# Validasi ChatGPT Chat — dibuktikan 2026-10-13 dari dokumentasi resmi

Sumber:
- https://developers.openai.com/plugins/build/app-quickstart
- https://developers.openai.com/plugins/build/mcp-server
- https://developers.openai.com/plugins/build/auth
- https://developers.openai.com/plugins/build/plugins

Hasil:

1. Custom MCP dapat dipilih dalam ChatGPT Chat biasa: YA.
   Jalur: ChatGPT → Plugins → tombol plus → Add custom MCP server → paste HTTPS + `/mcp` → Create as a plugin → install → chat baru → `@` pilih plugin.
2. Tools dapat diekspos ke percakapan: YA, via MCP tools (`registerTool`, Streamable HTTP `/mcp`).
3. Operasi baca dan tulis tersedia: YA. Anotasi `readOnlyHint/destructiveHint/openWorldHint` + OAuth scopes + konfirmasi untuk aksi konsekuensial.
4. Add custom MCP server: form URL + nama + auth, risk warning → Create as a plugin.
5. Instalasi/aktivasi plugin: dari Plugins Directory / local marketplace, lalu `@plugin` di composer.
6. OAuth: OAuth 2.1 authorization-code + PKCE S256, protected-resource metadata `.well-known/oauth-protected-resource`, CIMD preferred / DCR fallback, `resource` echo ke `aud`, redirect `https://chatgpt.com/connector_platform_oauth_redirect` (atau callback-ID bila tanpa issuer identification), mTLS client cert opsional.
7. Registrasi aplikasi MCP: ya — CIMD document / DCR `registration_endpoint`, atau provided client.
8. Akun/host target: plugin dir universal ChatGPT+Codex; workspace Enterprise bisa batasi via domain verified email.
9. Batasan paket/workspace/izin: butuh endpoint HTTPS publik stabil (`/mcp`), bukan tunnel sementara untuk submission publik; Secure MCP Tunnel tidak cukup untuk publik.

Kesimpulan: TIDAK ADA HAMBATAN PLATFORM. Asumsi "Codex-only" salah.

Server diagnostik minimal: `mcp-ts` dengan `OC_DIAGNOSTIC=1` mengekspos `diag_ping` + `diag_system_info` (read-only, noauth) untuk pembuktian pemanggilan dari ChatGPT Chat sebelum pairing.
Langkah bukti: `cd mcp-ts; npm install; OC_DIAGNOSTIC=1 npm run dev; npx ngrok http 8787` → Add custom MCP `https://<id>.ngrok.app/mcp` → `@OpenCommander diag_ping`.
