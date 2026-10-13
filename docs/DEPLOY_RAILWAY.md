# Bukti Deploy Railway (2026-10-13)

Project: `OpenCommander` (NoHeart6) — region `iad`/us-east, 1 replica per service.
Satu image Docker berisi gateway Go + MCP Node; `entrypoint.sh` memilih proses dari `START_CMD`.

## Endpoint publik

| Service | URL | Health |
|---|---|---|
| gateway | https://gateway-production-0964.up.railway.app | `{"ok":true}` |
| mcp | https://mcp-production-394c.up.railway.app | `{"ok":true}` |

MCP `/mcp`: `initialize` → protocolVersion 2025-06-18, serverInfo `opencommander 0.1.0`.
OAuth metadata: `/.well-known/oauth-protected-resource` → resource = domain MCP, scopes `devices:read,devices:write`.

## Uji end-to-end via MCP Railway (curl, bukan Inspector)

| # | Operasi | Task ID | Hasil |
|---|---|---|---|
| 1 | device_list | — | device `dev_6afbf8…c8c7`, owner `owner-test-001`, online true |
| 2 | system_info | task_1791866019_1 | succeeded — Ryzen 5 6600H, Win 10 19045, RAM 14.7 GB (data asli PC) |
| 3 | file_manage mkdir | task_1791866029_2 | succeeded — folder `Documents\OpenCommanderTest` |
| 4 | file_write | task_1791866039_3 | succeeded — 32 bytes, isi terverifikasi di disk |
| 5 | file_read | task_1791866068_4 | succeeded — isi identik "OpenCommander berhasil terhubung" |
| 6 | terminal_execute (cmd) | task_1791866084_5 | succeeded — exit 0, stdout "OpenCommander Railway OK" |
| 7 | desktop_screenshot | task_1791866084_6 | succeeded — PNG base64 valid |

Rantai terbukti: **MCP (Railway HTTPS) → Gateway (Railway WSS) → Rust agent lokal → Windows asli**.
Agent tidak punya port masuk; hanya koneksi keluar.

## Catatan operasional (wajib diketahui)

- **In-memory store**: pairing code GAGAL saat multi-replica (kode dibuat di instance A, diklaim di B). Harus 1 replica ATAU pindah ke Postgres persisten. Saat ini dikunci 1 replica per service.
- **Owner saat ini placeholder** `GATEWAY_OWNER=owner-test-001` untuk uji; OAuth token asli belum Flow.
- **Auto-reconnect agent belum ada**: gateway restart → WS putus, agent harus start ulang. Ini gap §9 yang belum ditutup.
- Agent binary = debug build, unsigned.

## Verifikasi manual di ChatGPT Chat (belum dilakukan)

1. chatgpt.com/plugins → Add custom MCP server → `https://mcp-production-394c.up.railway.app/mcp`
2. Create as plugin → install → chat baru → `@OpenCommander`
3. Uji 4 skrip di `ACCEPTANCE.md`.