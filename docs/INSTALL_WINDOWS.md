# Instalasi OpenCommander Windows

Ekstrak ZIP lalu jalankan `OpenCommanderSetup.exe`. Untuk instalasi yang sudah ada, jalankan installer dengan argumen `--upgrade`; pairing dan izin lokal dipertahankan. Agent berjalan pada akun Windows pengguna dan terdaftar untuk mulai saat login.

Pada PC yang sudah dipasangkan, gunakan shortcut Desktop `Jalankan OpenCommander` untuk menghidupkan agent kembali. Binary terpasang berada di `%LOCALAPPDATA%\Programs\OpenCommander\OpenCommander.exe`; argumen `--background` menjalankan agent tanpa membuka UI izin.

Untuk koneksi baru, tambahkan server MCP kustom `https://mcp-production-394c.up.railway.app/mcp` pada Plugin ChatGPT, pilih OAuth lalu login ke akun OpenCommander. Pasangkan kode yang ditampilkan UI agent. Paket tidak memuat password, token atau identitas perangkat.

Instalasi baru memakai persetujuan lokal per operasi dan allowlist. Pemilik dapat menyimpan folder yang diizinkan, `Allow all applications`, izin screenshot dan `Confirm sensitive actions` melalui UI native. Pada PC yang sudah mengaktifkan izin tetap, upgrade mempertahankan pengaturan tersebut.

Screenshot tersedia melalui `desktop_screenshot` dan `desktop_view`. Pada klien yang hanya meneruskan hasil terstruktur, instruksi plugin meneruskan `result.image_data` ke mekanisme `image()` agar AI menerima konteks gambar. Klik dan drag mengikuti tombol utama Windows, termasuk ketika tombol mouse tertukar.

Terminal mendukung empat task bersamaan. `terminal_execute` dengan `interactive=true` membuka stdin pipe; gunakan `terminal_input` dengan `target_task_id` untuk mengirim teks dan EOF. Tambahkan `pty=true` untuk TUI melalui ConPTY Windows. `terminal_input` pada sesi ConPTY menerima teks, daftar `keys` dan pasangan `cols`/`rows` untuk resize. `terminal_screen` membaca layar karakter dan cursor; `task_output` membaca output VT dengan stdout/stderr digabung. ConPTY tidak memakai EOF pipe; gunakan tombol keluar sesuai program atau `task_cancel` untuk menutup sesi dan proses anaknya. Durasi task maksimal 600 detik. ConPTY membutuhkan Windows 10 1809 atau lebih baru.

Proses yang selesai normal memiliki status task `succeeded`; periksa `exit_code` dan `program_succeeded` untuk hasil programnya. `success_exit_codes` default `[0]`; untuk pilihan normal `choice`, tetapkan kode yang diterima, misalnya `[1,2]`. Kegagalan peluncuran dan timeout tetap gagal, sedangkan pembatalan memiliki status `cancelled`.

Setelah pembaruan schema/tool, buka koneksi OpenCommander pada halaman Plugin ChatGPT, pilih **Refresh**, lalu buat percakapan baru. Daftar MCP v1.0.2 memuat 42 tools, termasuk `terminal_screen` dan `wordcraft`. Jalankan WordCraft dengan `--control 7981`, lalu gunakan tool `wordcraft` dengan path absolut ke `wordcraft-cli.exe` untuk memanggil MCP bawaan WordCraft. Hanya dokumen dalam approved root dan perintah editor yang diizinkan dapat digunakan.

Metadata rilis memakai tanda tangan Ed25519 dan checksum SHA-256. Executable belum ditandatangani Authenticode.

## Gambar dari ChatGPT ke Windows dan WordCraft

Tool `image_place` menyimpan gambar PNG/JPEG pada path Windows yang diizinkan, tanpa menimpa file yang sudah ada. Masukkan tepat satu sumber: `source_url` (HTTPS langsung ke berkas gambar, maksimum 8 MB) atau `image_base64` (maksimum 1.400.000 karakter). Path tujuan wajib berakhiran .png/.jpg/.jpeg sesuai format asli. Tool mengembalikan path, dimensi, ukuran, dan SHA-256.

Untuk langsung menyisipkan ke WordCraft, jalankan WordCraft dengan `--control 7981` dan tambahkan `wordcraft_cli_path` yang menunjuk ke `wordcraft-cli.exe`. Pengaturan opsional: `wordcraft_port`, `width_pt`, dan `alt_text`. Tool menggunakan command bawaan `insert.picture` pada dokumen aktif. Jika penyisipan gagal, gambar yang sudah berhasil disimpan tetap berada di PC dan `wordcraft_error` dikembalikan.

File sandbox internal ChatGPT bukan alamat unduhan Windows. Jika hasil image generation tidak memberikan URL yang dapat diakses atau bytes/Base64 yang dapat diteruskan, asset perlu dijembatani terlebih dahulu. Tool ini tidak dengan sendirinya memberi ChatGPT akses langsung ke bytes tersembunyi dari generator gambar.


## Transfer media besar (hingga 512 MiB)

Tool `media_transfer` menyimpan file asli ke Windows secara bertahap; tidak perlu menaruh seluruh data Base64 dalam satu permintaan. Mulai dengan `op=begin`, `destination`, `total_bytes` (dan idealnya `sha256`). Gunakan `transfer_id` dari balasannya untuk semua operasi berikutnya.

- **Transfer dari bytes lokal**: kirim `op=chunk` dengan `offset` sama dengan `received_bytes` dan `chunk_base64` maksimal 64 KiB terdecode; setiap respons menyertakan persentase kemajuan. Jika koneksi terputus, gunakan `op=status` lalu lanjut dari byte terakhir; mengirim ulang chunk identik tidak menggandakan data.
- **Transfer dari URL HTTPS**: setelah `begin`, jalankan `op=fetch` dengan `source_url`. Agent mengunduh langsung ke disk dan menggunakan HTTP Range untuk melanjutkan transfer setelah koneksi terputus. Server sumber perlu mendukung Range untuk resume.
- **Selesai**: `op=finish` memeriksa jumlah byte dan SHA-256 (jika diberikan). Opsional `wordcraft_cli_path`, `width_pt`, dan `alt_text` untuk menyisipkan gambar ke dokumen WordCraft yang sedang aktif. Gambar tetap disimpan jika penyisipan WordCraft gagal.
- **Batalkan**: `op=cancel` membersihkan transfer parsial. File final tidak ditimpa.

File dari direktori internal ChatGPT (`sandbox:/...`) tidak bisa dipakai sebagai URL unduhan oleh Windows. Integrasi akses bytes gambar ChatGPT diperlukan sebagai sumber pengiriman chunk, atau gunakan URL unduhan langsung. Transfer 100+ MB paling efisien lewat URL yang dapat diakses agen; mengirim Base64 dalam ratusan pemanggilan MCP memang memungkinkan tetapi lebih lambat.


## Otomatis kirim file gambar dan dokumen dari ChatGPT (asset bridge)

Tool `asset_deliver` menerima **file asli yang tersedia dalam ChatGPT** menggunakan metadata resmi MCP `_meta["openai/fileParams"]=["file"]`. ChatGPT dapat mengisi `file` dengan `{file_id,download_url,mime_type?,file_name?}`. `assets_deliver` menerima array `files` untuk 1?10 berkas sekaligus. Keduanya mengalirkan bytes langsung dari signed HTTPS URL ke agen Windows; model tidak perlu membuat Base64, menyalin `sandbox:/...`, atau menyuruh pengguna mengunduh lalu mengunggah lagi.

Jika `destination` tidak ditentukan, file masuk `%USERPROFILE%\Pictures\OpenCommander\<file_name>` (nama unik saat bentrok). Gunakan `directory` untuk folder tujuan batch atau `destination` untuk nama spesifik. Opsi `wordcraft_cli_path`, `wordcraft_port`, `width_pt`, dan `alt_text` menyisipkan PNG/JPEG ke WordCraft aktif; file lain tetap disimpan di Windows. Transfer dilakukan streaming hingga 512 MiB, hash SHA-256 dihitung, dan partial transfer bisa dilanjutkan jika sumber mendukung HTTP Range dan file ID masih sama. URL yang sudah kedaluwarsa dapat diganti lewat fileParams dengan URL baru dan file ID sama.

**Batas platform**: ChatGPT menentukan apakah file dari sebuah tool/generator tersedia untuk `openai/fileParams`. Plugin tidak dapat membaca file internal `sandbox:/...` hanya dari path atau ID yang tidak diberi download URL. Jika host tidak memberikan `download_url`, tool perlu dukungan API host tersebut; jangan menjanjikan transfer otomatis tanpa tes di chat aktual.


## Pembersihan riwayat otomatis (1.0.2)

Setelah satu jam tanpa penggunaan MCP dan tidak ada tugas aktif, agent menghapus jurnal tugas, hasil, keluaran terminal, dan log aktivitas. Gateway juga menghapus catatan tugas, keluaran, dan log aktivitas akun yang tidak aktif. Pemanggilan tool, pembacaan status/output, dan eksekusi tugas memperbarui waktu aktivitas; heartbeat koneksi tidak. Pemeriksaan berjalan setiap sekitar 10 detik saat terhubung, juga saat reconnect dan sebelum pemulihan startup untuk membersihkan jurnal lama yang penuh. Pairing, izin lokal, file kerja, trash, dan receipt undo dipertahankan. Riwayat yang sudah dihapus tidak dapat dibaca kembali; periksa keadaan aktual sebelum mengulangi tindakan lama.

## OpenCommander 1.0.2: bidirectional workflow + smart editor + visual computer

- `file_export` reads binary Windows file data **back into ChatGPT** in exact 48 KiB Base64 chunks; `op=info` returns original name, size and SHA-256, `op=chunk` returns bounded bytes and next_offset. Maximum file size 512 MiB. For large files, a caller must request successive chunks: this is not yet an automatic ChatGPT-host file attachment/upload mechanism.
- `workflow` runs up to 20 ordered steps with persistent checkpoint (`checkpoint_path`). `op=preview` previews validated actions; `op=run` executes; `op=status` inspects progress. Each step has `id`, `action` and `args`, and optional postcondition `verify: {type: exists|sha256|contains|wordcraft_image, path?, value?}`. A completed step never reruns when the same plan is retried. If a step fails, execution halts and must be explicitly retried with `retry_failed_step=true`; interrupted/uncertain outcomes require `retry_uncertain_step=true` after inspecting side effects. No arbitrary shell commands are allowed as workflow steps.
- `wordcraft_compose` composes a fresh DOCX using WordCraft native MCP with ordered blocks: title, heading1/heading2/heading3, paragraph, image (optional caption), table (rectangular array), page_break. Maximum 80 blocks; verifies images through document inspection and refuses to overwrite an existing output document.
- `visual_agent` supplies `op=observe` (window enumeration or HWND UIA inspection) and `op=act` (single exact UIA focus, invoke, select or set_value). Returns a post-action UI tree, and only marks verified when caller-provided expect_text appears in the new accessibility tree. Optional screenshot is returned as actual MCP image content when permitted. AI reasoning remains in ChatGPT, not on the PC.

### Suggested one-prompt workflows

To work with Windows files, first ask `file_export` for metadata and selected chunks. To automate a document pipeline, create steps for file operations, WordCraft edits and UI inspection; use `workflow` with a unique checkpoint path and explicit postconditions. For a complete new DOCX, use `wordcraft_compose` with blocks and an existing WordCraft `--control 7981` process. To interact visually, inspect before acting and provide `expect_text` so verification does not rely on assuming a click succeeded.

All four tools run under the same paired-device policy, approved roots, local confirmation settings and native MCP authentication. The workflows are deterministic sequences supplied by ChatGPT; this release does not install or run any local language model. Automated multihour planners and arbitrary third-party application plug-ins are not included in this release.
