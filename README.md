# OpenCommander

Aplikasi Windows native yang dikendalikan lewat **percakapan ChatGPT biasa**.

ChatGPT adalah otak. OpenCommander menyediakan akses dan eksekusi di Windows.

```
ChatGPT Chat → MCP Server (TS) → Gateway (Go) → Agent (Rust) → Windows
```

Bahasa Indonesia. Lihat `docs/` untuk panduan lengkap.

## Status

`INCOMPLETE (v0.1.0 scaffold)` — jalur ChatGPT Chat tervalidasi dari dokumentasi resmi (lihat `docs/VALIDATION_CHATGPT.md`). Acceptance end-to-end di ChatGPT Chat nyata belum dijalankan (butuh endpoint HTTPS publik + pairing di PC pengguna).

## Struktur

- `agent-rust/` — Windows Agent native (tray, pairing, capabilities).
- `gateway-go/` — Remote gateway (routing, ownership, tasks, recovery).
- `mcp-ts/` — MCP server Streamable HTTP untuk ChatGPT.
- `protocol/` — Spesifikasi protokol Agent↔Gateway.
- `deploy/` — Dockerfile, Compose, Railway, migrasi.
- `installer/` — NSIS + build script (`OpenCommanderSetup.exe`).
- `docs/` — Validasi ChatGPT, keamanan, troubleshooting, rilis.

## Cepat mulai (dev)

```powershell
# 1. MCP diagnostik (tanpa auth, read-only)
cd mcp-ts; npm install; npm run dev
# → http://localhost:8787/mcp

# 2. Gateway
cd gateway-go; go run ./cmd/gateway

# 3. Agent (Windows)
$env:PATH += ";$env:USERPROFILE\.cargo\bin"
cd agent-rust; cargo run -- --diagnostic
```

## Pairing

1. Instal `OpenCommanderSetup.exe`.
2. Tray → Pairing → dapat kode 6 digit / tautan.
3. Buka akun yang dipakai di ChatGPT, tautkan perangkat (OAuth).
4. Status `Connected`. Di ChatGPT: Plugins → Add custom MCP server → `https://<gateway>/mcp` → install → `@OpenCommander`.

Jangan simpan token/password di repo. Lihat `deploy/.env.example`.
