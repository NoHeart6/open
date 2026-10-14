# OpenCommander

Agent Windows native yang dikendalikan melalui tool MCP dari percakapan ChatGPT.

ChatGPT → MCP server TypeScript → gateway Go → agent Rust → Windows.

## Status saat ini

Agent Windows v0.2.4 sudah terpasang pada PC ini dan terhubung ke gateway. Semua aplikasi lokal diizinkan atas permintaan pemilik; peluncuran VS Code sudah diuji melalui endpoint MCP produksi (`docs/ALL_APPLICATIONS_2026-10-15.md`). Screenshot dan fokus UI Automation sudah diuji; hasilnya ada di `docs/DESKTOP_FIX_2026-10-15.md`. Akses profil pengguna dan mode izin tetap aktif. Koneksi dan panggilan tool di percakapan ChatGPT pengguna tetap memerlukan bukti dari host tersebut. Status keseluruhan belum COMPLETE.

## Struktur

- `apps/windows-agent/`: agent, installer, UI izin, persetujuan lokal dan eksekusi Windows.
- `apps/gateway/`: autentikasi perangkat, penyimpanan SQLite, routing dan task.
- `apps/mcp-server/`: OAuth, tool MCP dan pengujian integrasi.
- `infra/`: Docker, konfigurasi Keycloak dan kunci publik rilis.
- `plugins/opencommander/`: paket plugin dan instruksi kontrol Windows.
- `scripts/`: build, diagnosis, konfigurasi OAuth dan penandatanganan metadata rilis.
- `.railway/`: konfigurasi infrastruktur Railway.
- `docs/`: audit awal, hasil perbaikan dan batas verifikasi.

## Instalasi Windows

Jalankan `OpenCommanderSetup.exe`. Installer mempertahankan pairing dan kebijakan saat upgrade. Agent memiliki UI native untuk pairing, akses remote, folder yang diizinkan dan persetujuan tindakan sensitif.

Instalasi baru mengizinkan workspace terbatas. Pada PC ini, pemilik secara eksplisit mengizinkan seluruh profil `C:\Users\Administrator`. Kebijakan itu disimpan di konfigurasi lokal dan bertahan setelah restart/upgrade. Atas permintaan pemilik, mode izin tetap juga aktif pada PC ini (`ask_before_sensitive: false`), sehingga terminal dan kontrol Windows tidak meminta klik persetujuan per operasi. Instalasi baru tetap memakai konfirmasi per operasi. Centang kembali `Confirm sensitive actions` lalu simpan untuk mengaktifkan konfirmasi lagi. Folder internal agent, lokasi kredensial yang dikecualikan dan izin Windows tetap dilindungi.

Gunakan `system_info` untuk melihat `agent_version` dan `workspace.approved_roots` sebelum operasi file atau menentukan working directory terminal. Detail dan bukti ada di `docs/PROFILE_ACCESS_2026-10-14.md` dan `docs/TRUSTED_OWNER_MODE_2026-10-14.md`.

Pada PC ini `workspace.allow_all_applications` bernilai `true`. `application_launch` dapat meluncurkan executable lokal yang ada tanpa mendaftarkannya satu per satu. Pengaturan tersimpan sebagai `policy.allow_all_applications` dan tersedia pada checkbox `Allow all applications` di UI Windows. Working directory masih mengikuti root file yang diizinkan. Instalasi baru memakai allowlist sampai pemilik mengaktifkan pengaturan tersebut.

## Hubungkan ChatGPT

Tambahkan server MCP kustom pada halaman Plugin ChatGPT dengan URL:

```text
https://mcp-production-394c.up.railway.app/mcp
```

Pilih OAuth, login pada halaman OpenCommander dan tinjau persetujuan. Setelah terhubung, mulai dengan `device_list` dan `system_info`. Password dan token tidak disertakan dalam paket plugin.

## Pengembangan dan verifikasi

```powershell
.\scripts\build.ps1 -Release
.\scripts\diagnose-chatgpt-integration.ps1
.\scripts\diagnose-chatgpt-integration.ps1 -ProfileAccess
.\scripts\diagnose-chatgpt-integration.ps1 -Desktop
.\scripts\diagnose-chatgpt-integration.ps1 -AllApplications
```

Diagnosis produksi menggunakan kredensial lokal terenkripsi yang telah diprovisikan; jangan menambahkan kredensial ke source. Mode `-ProfileAccess` membuat fixture sementara pada profil pengguna yang sudah diizinkan, lalu membersihkannya.

Mode `-Desktop` menggunakan izin tetap yang sudah diaktifkan pemilik, membaca screenshot dan menguji fokus pada jendela fixture sementara. Jendela fixture ditutup setelah pengujian. Untuk fokus jendela, `desktop_automation` cukup menerima `hwnd` dan `op: focus`. Untuk kontrol tertentu, gunakan `element_path` dari `desktop_inspect` dan nama/ID yang tidak kosong; inspeksi ulang setelah UI berubah.

Rust: `cargo test --locked` dan `cargo clippy --locked --all-targets -- -D warnings` di `apps/windows-agent`. TypeScript: `npm run lint`, `npm run typecheck`, `npm test` di `apps/mcp-server`. E2E Windows lokal: `npx tsx test/e2e.ts --headless` atau tanpa `--headless` untuk pengujian desktop.

Bukti pengujian berada di `artifacts/`; runtime, log, backup dan kredensial lokal dikecualikan dari Git dan ZIP plugin. Metadata rilis memakai Ed25519; executable belum ditandatangani Authenticode.
