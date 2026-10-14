# Perbaikan usability OpenCommander v0.2.7

Agent v0.2.7 sudah terpasang pada PC pemilik dan terhubung ke gateway. Plugin akun yang sama diperbarui ke v0.2.7 tanpa mengubah identitas, OAuth, audience atau default prompt. Server MCP memakai deployment `0250b195-f043-417e-b7d3-af966ab21f01`, berstatus SUCCESS.

| Fitur | Bukti setelah perbaikan |
| --- | --- |
| Screenshot | Panggilan asli `desktop_screenshot` mengembalikan status succeeded, blok MCP image/jpeg dan JPEG identik pada `result.image_data`. `task_status` dan `desktop_view` mempertahankan gambar tersebut. Pengguna mengonfirmasi AI di ChatGPT biasa dapat melihat dan menjelaskan layar setelah penerusan gambar melalui `image()`. |
| UI Automation | Focus dan set_value memverifikasi foreground/teks; select membaca kembali status pilihan. Invoke melaporkan sent, lalu efeknya dibuktikan melalui penghitung pada fixture. |
| Mouse dan keyboard | Teks hasil pengetikan dibaca kembali; klik menaikkan penghitung tombol; drag mencapai endpoint; scroll mengubah item list yang terlihat. Semua mengembalikan screenshot setelah tindakan. |
| Get-FileHash | PowerShell 5.1 native berhasil menghitung SHA-256 yang sesuai, termasuk ketika PSModulePath pada request berisi direktori yang tidak ada. |

## Pengiriman screenshot

Laporan pengguna menunjukkan gambar terminal JPEG dapat dianalisis setelah Base64 diteruskan dengan `image()`, tetapi klien hanya menampilkan metadata pada hasil screenshot bawaan. Respons MCP sudah mempunyai image content; menambahkan panel saja tidak cukup membuktikan konteks gambar diterima model.

Server sekarang menunggu task screenshot sampai selesai, maksimal 15 detik, termasuk status queued, dispatched dan running. Task hanya disubmit sekali; task ID dan idempotency tetap dipertahankan. Jika belum selesai atau menunggu persetujuan lokal, hasil task tetap dapat diambil melalui tool status.

Agent menyediakan PNG untuk kompatibilitas serta JPEG kecil untuk jalur klien terstruktur. PNG dibatasi 500.000 byte dan JPEG 100.000 byte, sehingga respons agent tetap berada di bawah budget transport. JPEG memakai kualitas 70, rasio aspek asli, maksimal 960 × 600, dan ukuran adaptif untuk layar dengan banyak detail.

Server mengirim JPEG sebagai blok gambar MCP dan menyediakan `result.image_data = {mime_type, base64}`. Raw PNG tidak ikut diteruskan pada hasil terstruktur. JSON teks hanya berisi metadata, sementara JPEG terstruktur tersedia bagi adapter yang menyembunyikan content blocks. Instruksi plugin meminta penerusan image_data melalui mekanisme `image()` pada eksekusi yang sama; string Base64 sendiri belum menjadi konteks visual.

Pengujian produksi: task `pciXtfXCJXSvMLSWVPoTFzmp`, request `4b49438f-b48d-4846-89f4-4dd6dd15c513`, gambar 720 × 450, 69.688 byte, Base64 92.920 karakter. Gambar, Base64 dan desktop pengguna tidak disertakan dalam Git atau laporan ini. Klien independen memverifikasi bahwa panggilan asli, task_status dan desktop_view menghasilkan JPEG yang sama.

Panel privat menggunakan resource MCP App `ui://opencommander/desktop-v1.html`. Browser fixture memverifikasi gambar PNG dimuat melalui notifikasi tool-result standar. Dukungan panel di ChatGPT belum dikonfirmasi; penerimaan gambar melalui image() dikonfirmasi terpisah.

Pada 15 Oktober 2026, pengguna diminta menguji `desktop_screenshot` di ChatGPT biasa, meneruskan `result.image_data` melalui `image()`, lalu meminta AI menjelaskan isi layar. Jawaban pengguna: "Ya, bisa melihat dan menjelaskan layar". Penerimaan visual screenshot bawaan di host tersebut dicatat PASS berdasarkan konfirmasi pengguna. Konfirmasi ini melengkapi bukti pengiriman gambar dari klien independen; tidak tersedia transcript tool mentah tambahan untuk percobaan pengguna tersebut.

## Mouse, UI Automation dan PowerShell

Pada Windows ini `GetSystemMetrics(SM_SWAPBUTTON)` bernilai 1. Sebelum perbaikan, klik default menghasilkan WM_RBUTTONDOWN/WM_RBUTTONUP (516/517) pada fixture dan penghitung tetap 1. Setelah mengikuti tombol utama Windows, fixture menerima WM_LBUTTONDOWN/WM_LBUTTONUP (513/514) dan penghitung menjadi 2. Click, double_click dan drag memakai tombol utama; right_click memakai tombol sekunder. Pengaturan Windows pengguna tidak diubah.

Pointer memakai koordinat fisik pada thread yang sadar DPI per monitor. Gerakan diinjeksi melalui SendInput dalam koordinat absolut virtual desktop dan posisi dibaca kembali. Tombol drag selalu dilepas melalui guard, termasuk jika gerakan gagal. Input manual bersamaan dapat menggagalkan pembacaan posisi; diagnosis berikutnya tanpa gerakan manual lulus.

Semua tindakan desktop melaporkan input_sent, verified dan verification_state. Keberhasilan pengiriman tidak dipakai untuk mengklaim efek bisnis aplikasi. Screenshot dan focused_control sesudah tindakan membantu membaca hasil; kegagalan screenshot tidak mengubah tindakan yang sudah terkirim menjadi kegagalan yang boleh direplay. capture_after=false menonaktifkan observasi tambahan.

PowerShell sebelumnya mewarisi PSModulePath dengan modul PowerShell 7, meskipun executable yang berjalan adalah Windows PowerShell 5.1. Agent kini memilih executable native secara eksplisit, mendahulukan modul dari PSHOME Windows, dan mengimpor Microsoft.PowerShell.Utility dari manifest native sebelum menjalankan perintah pengguna.

## Validasi dan distribusi

- 24 tes Rust lulus; Clippy bersih; build release berhasil.
- 10 tes MCP lulus; lint, typecheck dan build berhasil. Regresi meliputi status dispatched, pengembalian gambar pada panggilan asli, structured-only adapter dan batas ukuran respons.
- 27 pemeriksaan usability produksi lulus tanpa klik persetujuan lokal. Jendela fixture dan prosesnya dibersihkan.
- Pemeriksaan screenshot produksi membaca gambar dari agent terpasang menggunakan OAuth read-only; token read-only tetap ditolak untuk mutasi dan perangkat lain tetap tidak dapat diakses.
- Pairing dan konfigurasi sebelum/sesudah upgrade memiliki checksum yang sama. Seluruh profil pengguna, semua aplikasi dan izin tetap tetap aktif. Satu agent terpasang berjalan; shortcut Desktop masih menunjuk binary itu dengan argumen --background.
- Metadata Ed25519 publik dan checksum installer diverifikasi. Binary terpasang dan installer publik sama: SHA-256 `4c7d68afc621b6b8cf919b2455621a7993a5e76c42e23530d0f5ef68216a04f3`, ukuran 5.490.688 byte. Executable belum ditandatangani Authenticode.
- ZIP Windows berisi installer, binary, petunjuk instalasi dan lisensi dependensi. ZIP plugin berisi manifest, MCP dan skill terbaru. Tidak ada password, pairing, gambar desktop atau database.

Laporan lokal: `artifacts/usability-results.json`, `artifacts/screenshot-delivery-results.json` dan reproduksi `artifacts/mouse-swapped-before.json`. Ulangi dengan `scripts/diagnose-chatgpt-integration.ps1 -Usability` atau `-Screenshot` dari PowerShell 7.

Referensi API: [MOUSEINPUT Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput) dan [OpenAI plugin reference](https://developers.openai.com/plugins/reference).
