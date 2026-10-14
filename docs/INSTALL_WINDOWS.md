# Instalasi OpenCommander Windows

Ekstrak ZIP lalu jalankan `OpenCommanderSetup.exe`. Untuk instalasi yang sudah ada, jalankan installer dengan argumen `--upgrade`; pairing dan izin lokal dipertahankan. Agent berjalan pada akun Windows pengguna dan terdaftar untuk mulai saat login.

Pada PC yang sudah dipasangkan, gunakan shortcut Desktop `Jalankan OpenCommander` untuk menghidupkan agent kembali. Binary terpasang berada di `%LOCALAPPDATA%\Programs\OpenCommander\OpenCommander.exe`; argumen `--background` menjalankan agent tanpa membuka UI izin.

Untuk koneksi baru, tambahkan server MCP kustom `https://mcp-production-394c.up.railway.app/mcp` pada Plugin ChatGPT, pilih OAuth lalu login ke akun OpenCommander. Pasangkan kode yang ditampilkan UI agent. Paket tidak memuat password, token atau identitas perangkat.

Instalasi baru memakai persetujuan lokal per operasi dan allowlist. Pemilik dapat menyimpan folder yang diizinkan, `Allow all applications`, izin screenshot dan `Confirm sensitive actions` melalui UI native. Pada PC yang sudah mengaktifkan izin tetap, upgrade mempertahankan pengaturan tersebut.

Screenshot tersedia melalui `desktop_screenshot` dan `desktop_view`. Pada klien yang hanya meneruskan hasil terstruktur, instruksi plugin meneruskan `result.image_data` ke mekanisme `image()` agar AI menerima konteks gambar. Klik dan drag mengikuti tombol utama Windows, termasuk ketika tombol mouse tertukar.

Metadata rilis memakai tanda tangan Ed25519 dan checksum SHA-256. Executable belum ditandatangani Authenticode.
