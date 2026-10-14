# Semua aplikasi lokal — v0.2.4

Atas permintaan pemilik, `policy.allow_all_applications: true` sudah disimpan pada PC ini. Agent v0.2.4 terpasang dan berjalan dengan konfigurasi tersebut setelah restart. `ask_before_sensitive: false` tetap aktif, sehingga peluncuran aplikasi tidak meminta klik izin per operasi.

## Perubahan

`Policy::application` tetap memvalidasi path executable lokal yang ada. Jika `allow_all_applications` aktif, executable tidak harus ada di array `applications`. Ini mencakup aplikasi yang baru dipasang tanpa enumerasi allowlist. Path relatif, file yang tidak ada, UNC, device namespace, ADS dan symlink tetap ditolak. Peluncuran tidak menaikkan hak akses Windows.

Pengaturan dapat disimpan melalui checkbox native `Allow all applications`. Daftar executable lama tetap tersedia jika mode semua aplikasi dimatikan. Config lama tanpa field tersebut dan instalasi baru tetap memakai allowlist. Argumen tool remote tidak dapat mengaktifkan pengaturan lokal ini.

`system_info.workspace` sekarang melaporkan `allow_all_applications` dan `approved_applications`. Deskripsi tool MCP menjelaskan kedua mode. Izin working directory dan kebijakan file tetap mengikuti konfigurasi folder.

## Bukti

- 22 tes Rust lulus, termasuk default allowlist, penolakan perubahan kebijakan melalui argumen remote, persistensi pengaturan, validasi path dan penyimpanan checkbox native. Clippy bersih.
- 7 tes MCP lulus; lint, typecheck dan build berhasil.
- 20 pemeriksaan produksi lulus melalui OAuth dan JSON-RPC ke MCP sebenarnya, dengan agent terpasang v0.2.4 dan mode semua aplikasi aktif.
- `C:\Users\Administrator\AppData\Local\Programs\Microsoft VS Code\Code.exe` tidak ada dalam array allowlist lama, tetapi berhasil diluncurkan lewat `application_launch`. Jendela nyata dengan PID 19864 dan HWND 2033724 ditemukan. Task: `E6MUug-I09gkIhPq1GLyp807`.
- VS Code diuji dengan user-data dan extensions directory sementara. Fokus dan Alt+F4 melalui tool berhasil menutup jendela; proses fixture dan profil sementara dibersihkan. Profil dan proyek pengguna tidak dipakai oleh fixture.
- Peluncuran path executable yang tidak ada tetap ditolak. Tidak ada operasi pengujian yang menunggu dialog persetujuan lokal.

Bukti tersanitasi: `artifacts/all-applications-results.json`. Ulangi dengan `scripts/diagnose-chatgpt-integration.ps1 -AllApplications` menggunakan PowerShell 7. Pengujian dilakukan oleh klien independen; penerimaan ulang di percakapan ChatGPT pengguna belum diverifikasi dari host tersebut.

## Instalasi

Hash binary terpasang cocok dengan build v0.2.4. Hash kredensial pairing terenkripsi tidak berubah. Backup sebelum upgrade: `.runtime/all-applications-before-20261015-015712`. Shortcut Desktop `Jalankan OpenCommander.lnk` tetap menunjuk binary terpasang.

Deployment MCP: `4a96d428-1f72-4c1f-ab16-9a93a8f18ec0`. Rilis v0.2.4 memakai metadata Ed25519. Executable belum ditandatangani Authenticode.

Paket: `C:\Users\Administrator\Downloads\OpenCommander-Windows-0.2.4.zip`. Paket tidak membawa password, token atau pairing. Mode semua aplikasi dan izin tetap pada PC ini berada dalam konfigurasi lokal, yang dipertahankan saat upgrade.
