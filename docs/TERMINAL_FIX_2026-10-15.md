# Terminal paralel dan stdin — v0.2.8

Agent Windows v0.2.8 sudah terpasang. Gateway dan MCP produksi serta plugin akun yang sama sudah diperbarui. MCP sekarang menyediakan 32 tools, termasuk `terminal_input`.

| Laporan awal | Hasil pengujian setelah perbaikan |
| --- | --- |
| Dua terminal berjalan bergantian | PASS: dua task berbeda berjalan pada interval yang overlap. |
| Terminal kedua gagal saat server panjang aktif | PASS: perintah kedua selesai saat listener server pertama masih aktif. |
| Stdin interaktif belum tersedia | PASS melalui klien MCP independen: stdin Unicode, echo streaming dan EOF. BLOCKED pada sesi ChatGPT pengguna: `terminal_input` dan parameter `interactive` belum muncul. |
| Multi-monitor | Belum diuji secara fisik; konfigurasi ini memiliki satu monitor. Tes konversi koordinat virtual dengan origin negatif tetap tersedia. |
| Dialog elevasi UAC | Belum diuji pada konfigurasi ini. Agent tetap menolak operasi desktop pada secure desktop Windows. |

## Penyebab dan perubahan

Sebelumnya satu semaphore berkapasitas 1 dipakai untuk semua operasi, dan permit ditahan sampai terminal selesai. Reproduksi produksi menggunakan listener TCP lokal yang hidup 30 detik. Terminal kedua dengan deadline 5 detik gagal dengan `deadline has elapsed` sebelum dapat menjalankan perintah. Task pertama: `Zh9cj6Ck8downQt8ZOP11Y0J`; kedua: `B772-pKLwAKvmxIubSYbMh1S`. Listener percobaan dihentikan setelah reproduksi.

Terminal sekarang memiliki pool empat slot yang terpisah dari slot operasi desktop/file. `terminal_input` memakai kanal kontrol sesi sehingga tetap dapat mengirim input saat semua slot terminal terisi. Batas total task lokal yang belum selesai tetap 16. Permission, consent lokal, validasi owner dan deadline tetap berlaku.

`terminal_execute` menerima `interactive` dan `stdin`. `interactive=true` mempertahankan stdin pipe selama proses masih hidup. `stdin` memasok input awal; jika interactive=false, pipe ditutup setelah input awal. PowerShell menggunakan encoding UTF-8 untuk stdin/stdout. Program yang dipanggil melalui cmd menentukan sendiri encoding pembacaannya. Sesi ini memakai redirected pipes; ConPTY dan aplikasi terminal layar penuh belum didukung.

`terminal_input` menerima device_id, idempotency_key, target_task_id, text dan eof. Text dikirim persis, termasuk newline jika diperlukan oleh program. Tool membuat task tersendiri; ID terminal tujuan tetap dipakai untuk output dan pembatalan. Gateway memverifikasi bahwa target adalah task terminal milik akun yang sama pada perangkat yang sama. Agent juga memverifikasi owner terhadap registry sesi lokal.

Kanal stdin dibatasi delapan request tertunda per sesi dan 16.384 byte per input. Write/flush dibatasi tiga detik. Receipt mengonfirmasi bytes_sent dan stdin_closed; efek pada aplikasi dibaca dari output terminal. Jika acknowledgement atau write tidak selesai, hasil dilaporkan uncertain agar input yang mungkin sudah terkirim tidak diulang. Idempotency request dan journal mencegah pengiriman ulang identik. Input ke sesi yang sudah selesai ditolak.

EOF menutup pipe sesudah teks. Task_cancel pada ID terminal menghentikan proses dan tree dalam Windows Job Object. Registry serta writer stdin dilepas saat sesi selesai, dibatalkan, atau terkena timeout. Durasi terminal maksimal 600 detik; task server foreground juga mengikuti deadline tersebut.

## Bukti pengujian

- 26 tes Rust lulus, termasuk scheduler terminal saat server aktif, stdin Unicode pada proses nyata, penolakan owner berbeda, EOF, input awal, job timeout/cancel dan installer. Clippy serta build release berhasil.
- 15 tes gateway lulus; go vet bersih. Regresi stdin meliputi ownership, target terminal, idempotency, request kosong dan penolakan token read-only.
- 10 tes MCP lulus; lint, typecheck dan build berhasil. Schema stdin tersedia dan token read-only ditolak sebelum mutasi.
- 27 pemeriksaan produksi gabungan terminal/screenshot lulus menggunakan OAuth dan agent terpasang v0.2.8. Permission tetap memakai owner preapproval; tidak ada sesi fixture yang tertinggal.
- Task `7tAdpR-9QqZ_LvwNTtjxfrqc` dan `V7MSo7xfKfX498vJTwAKPjDK` berbeda dan keduanya succeeded. Interval native keduanya: start 1792010618227, end 1792010621242 milidetik Unix, overlap 3.015 detik.
- Stdin Unicode menghasilkan echo nyata. Retry idempotency yang sama mengembalikan task receipt yang sama tanpa echo ganda. EOF terkirim saat keempat terminal aktif. Desktop reads tetap tersedia pada kondisi tersebut.
- Input ke terminal yang selesai ditolak; input awal ditutup setelah dikirim. Semua terminal fixture akhirnya succeeded atau cancelled. Journal lokal menunjukkan nol task aktif sesudah pengujian.
- Pengiriman gambar asli, task_status dan desktop_view tetap menghasilkan JPEG yang sama setelah pembaruan terminal.

Pengujian oleh klien independen ini membuktikan eksekusi Windows melalui endpoint MCP produksi. Pengguna kemudian mengonfirmasi terminal paralel dan streaming stdout PASS di ChatGPT biasa. Stdin/EOF masih BLOCKED karena definisi tool di sesi tersebut belum memuat `terminal_input` atau parameter `interactive`. Status ini berbeda dari hasil eksekusi independen dan belum dapat dianggap lulus di ChatGPT.

## Pembaruan daftar tool ChatGPT

Versi yang dilaporkan saat MCP initialize sebelumnya masih hardcoded `0.2.0`. Server sekarang membaca versi `0.2.8` dari package.json; healthz juga mengembalikan versi tersebut. Tes menguji versi initialize, healthz, keberadaan terminal_input dan schema interactive/stdin. Perubahan nomor versi tidak otomatis menyegarkan action yang sudah disimpan ChatGPT.

Menurut [dokumentasi OpenAI](https://developers.openai.com/plugins/deploy/connect-chatgpt), koneksi MCP kustom yang berubah perlu dibuka pada halaman Plugins dan dipilih **Refresh**. Setelah metadata baru terkonfirmasi, mulai percakapan baru. Membuka percakapan baru sebelum Refresh saja tidak memastikan schema baru tersedia. Periksa bahwa daftar memuat `terminal_input` dan `terminal_execute` menerima `interactive` serta `stdin`. Jika koneksi workspace menampilkan action baru sebagai disabled, pengelola perlu meninjau dan mengaktifkannya sesuai [Action control](https://help.openai.com/en/articles/12584461-creating-custom-mcp-apps-in-chatgpt).

Nama tampilan `open` atau `OpenCommander` tidak menentukan isi schema; endpoint koneksi harus `https://mcp-production-394c.up.railway.app/mcp`. Browser ChatGPT pengguna yang sudah login tidak tersedia dalam sesi kontrol Codex ini, sehingga Refresh pada koneksi pengguna belum dilakukan oleh Codex.

Sesudah deploy metadata versi, 10 tes MCP, lint, typecheck dan build lulus. Diagnosis terautentikasi produksi menghasilkan 17 pemeriksaan PASS: initialize melaporkan versi 0.2.8, tools/list memuat 32 tools, interactive bertipe boolean, stdin bertipe string dan terminal_input tersedia. Bukti tersanitasi ada di `artifacts/chatgpt-integration-diagnostics.json`. Pemeriksaan ini memakai klien independen, bukan sesi ChatGPT pengguna.

Laporan lokal tersanitasi: `artifacts/terminal-before-results.json` dan `artifacts/terminal-results.json`. Jalankan ulang dengan `scripts/diagnose-chatgpt-integration.ps1 -Terminal`; tambahkan -Screenshot untuk regresi pengiriman gambar. Output fixture dipakai untuk assertion, sedangkan screenshot dan Base64 tidak disimpan dalam laporan.

## Distribusi

Gateway deployment: `aed5b479-a9ce-42d7-a145-9c98931a5cf2`. MCP deployment fitur terminal: `8e9a86c2-eab6-4c6a-9b7a-142a652cef4b`; pembaruan metadata versi: `83988a94-74e3-4c31-af34-02556ba74646`. Semuanya SUCCESS.

Plugin OpenCommander v0.2.8 tetap memiliki ID `plugins_6acf7aa2e4788191a6c8ab77b2f36fb9`. Release `pluginrel_6acfea3240008191b3ff80ce308c4779` dibaca kembali; identitas, interface/default prompt, OAuth dan audience tidak berubah. Skill menambahkan cara menggunakan terminal paralel dan stdin.

Checksum konfigurasi dan pairing tetap sama setelah upgrade; semua aplikasi dan izin tetap masih aktif. Rilis publik diverifikasi dengan Ed25519 serta SHA-256. Installer berukuran 5.533.696 byte, hash `255d880a4a7fe95a853870ebe45807fa275895814478cdb19fd094576c65986b`; binary terpasang memiliki hash yang sama. Executable belum ditandatangani Authenticode.

ZIP Windows: `releases/windows/OpenCommander-Windows-0.2.8.zip`. ZIP plugin tersedia di Downloads sebagai `OpenCommander-ChatGPT-0.2.8.zip`. Paket tidak membawa password, pairing, log, database atau screenshot privat.
