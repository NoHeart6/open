# Acceptance ChatGPT Chat — BUKTI (belum dijalankan)

Wajib lewat jalur nyata: ChatGPT → MCP → Gateway → Rust → Windows → ChatGPT.
Hanya di **ChatGPT Chat biasa** (bukan Codex/Work/MCP Inspector). Inspector PASS ≠ ChatGPT PASS.

## Prasyarat

- Gateway + MCP live di HTTPS publik (`https://<host>/mcp`).
- Agent terinstal + paired + Connected di PC Windows target.
- Folder pengujian yang disetujui, mis. `%USERPROFILE%\OpenCommanderTest`.

## Skrip uji (jalankan berurutan, catat request ID + task ID)

1. "Gunakan OpenCommander untuk memeriksa spesifikasi komputer Windows saya."
   - Harap: `device_list` → `device_status` → `system_info` → ringkasan asli.
2. "Ambil screenshot desktop saya."
   - Harap: `desktop_screenshot` PNG asli dirender di chat.
3. "Buka Notepad, ketik OpenCommander berhasil terhubung dengan ChatGPT, lalu periksa hasilnya."
   - Harap: `application_launch` + `desktop_interact` + `desktop_screenshot` verifikasi.
4. "Buat file percobaan di folder pengujian, baca kembali isinya, lalu laporkan hasilnya."
   - Harap: `file_write` → `file_read` isi sama persis.

## Bukti lokal yang sudah ada (2026-10-13)

- Agent `--diagnostic`: system_info asli (Ryzen 5 6600H, Win 10 19045) — PASS.
- Gateway: `/health` ok, pairing code terbit, idempotency + cancel + claim sekali pakai — PASS (`go test`).
- MCP diag: `initialize` → `tools/list` (diag_ping, diag_system_info) → `tools/call diag_ping ok:true` — PASS via curl.
- Rust: 3 regression tests file (roundtrip, no-overwrite, hash) — PASS.

## Status

**INCOMPLETE** — 4 dari 15 syarat COMPLETE terpenuhi sebagian (arsitektur + validasi docs + build lokal). Lihat README.
