# Instalasi OpenCommander Windows

Ekstrak ZIP lalu jalankan `OpenCommanderSetup.exe`. Untuk instalasi yang sudah ada, jalankan installer dengan argumen `--upgrade`; pairing dan izin lokal dipertahankan. Agent berjalan pada akun Windows pengguna dan terdaftar untuk mulai saat login.

Pada PC yang sudah dipasangkan, gunakan shortcut Desktop `Jalankan OpenCommander` untuk menghidupkan agent kembali. Binary terpasang berada di `%LOCALAPPDATA%\Programs\OpenCommander\OpenCommander.exe`; argumen `--background` menjalankan agent tanpa membuka UI izin.

Untuk koneksi baru, tambahkan server MCP kustom `https://mcp-production-394c.up.railway.app/mcp` pada Plugin ChatGPT, pilih OAuth lalu login ke akun OpenCommander. Pasangkan kode yang ditampilkan UI agent. Paket tidak memuat password, token atau identitas perangkat.

Instalasi baru memakai persetujuan lokal per operasi dan allowlist. Pemilik dapat menyimpan folder yang diizinkan, `Allow all applications`, izin screenshot dan `Confirm sensitive actions` melalui UI native. Pada PC yang sudah mengaktifkan izin tetap, upgrade mempertahankan pengaturan tersebut.

Screenshot tersedia melalui `desktop_screenshot` dan `desktop_view`. Pada klien yang hanya meneruskan hasil terstruktur, instruksi plugin meneruskan `result.image_data` ke mekanisme `image()` agar AI menerima konteks gambar. Klik dan drag mengikuti tombol utama Windows, termasuk ketika tombol mouse tertukar.

Terminal mendukung empat task bersamaan. `terminal_execute` dengan `interactive=true` membuka stdin pipe; gunakan `terminal_input` dengan `target_task_id` untuk mengirim teks dan EOF. Tambahkan `pty=true` untuk TUI melalui ConPTY Windows. `terminal_input` pada sesi ConPTY menerima teks, daftar `keys` dan pasangan `cols`/`rows` untuk resize. `terminal_screen` membaca layar karakter dan cursor; `task_output` membaca output VT dengan stdout/stderr digabung. ConPTY tidak memakai EOF pipe; gunakan tombol keluar sesuai program atau `task_cancel` untuk menutup sesi dan proses anaknya. Durasi task maksimal 600 detik. ConPTY membutuhkan Windows 10 1809 atau lebih baru.

Proses yang selesai normal memiliki status task `succeeded`; periksa `exit_code` dan `program_succeeded` untuk hasil programnya. `success_exit_codes` default `[0]`; untuk pilihan normal `choice`, tetapkan kode yang diterima, misalnya `[1,2]`. Kegagalan peluncuran dan timeout tetap gagal, sedangkan pembatalan memiliki status `cancelled`.

Setelah pembaruan schema/tool, buka koneksi OpenCommander pada halaman Plugin ChatGPT, pilih **Refresh**, lalu buat percakapan baru. Daftar MCP v0.3.2 memuat 35 tools, termasuk `terminal_screen` dan `wordcraft`. Jalankan WordCraft dengan `--control 7981`, lalu gunakan tool `wordcraft` dengan path absolut ke `wordcraft-cli.exe` untuk memanggil MCP bawaan WordCraft. Hanya dokumen dalam approved root dan perintah editor yang diizinkan dapat digunakan.

Metadata rilis memakai tanda tangan Ed25519 dan checksum SHA-256. Executable belum ditandatangani Authenticode.

## Gambar dari ChatGPT ke Windows dan WordCraft

Tool `image_place` menyimpan gambar PNG/JPEG pada path Windows yang diizinkan, tanpa menimpa file yang sudah ada. Masukkan tepat satu sumber: `source_url` (HTTPS langsung ke berkas gambar, maksimum 8 MB) atau `image_base64` (maksimum 1.400.000 karakter). Path tujuan wajib berakhiran .png/.jpg/.jpeg sesuai format asli. Tool mengembalikan path, dimensi, ukuran, dan SHA-256.

Untuk langsung menyisipkan ke WordCraft, jalankan WordCraft dengan `--control 7981` dan tambahkan `wordcraft_cli_path` yang menunjuk ke `wordcraft-cli.exe`. Pengaturan opsional: `wordcraft_port`, `width_pt`, dan `alt_text`. Tool menggunakan command bawaan `insert.picture` pada dokumen aktif. Jika penyisipan gagal, gambar yang sudah berhasil disimpan tetap berada di PC dan `wordcraft_error` dikembalikan.

File sandbox internal ChatGPT bukan alamat unduhan Windows. Jika hasil image generation tidak memberikan URL yang dapat diakses atau bytes/Base64 yang dapat diteruskan, asset perlu dijembatani terlebih dahulu. Tool ini tidak dengan sendirinya memberi ChatGPT akses langsung ke bytes tersembunyi dari generator gambar.
