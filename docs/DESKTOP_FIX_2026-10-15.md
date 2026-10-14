# Screenshot dan fokus UI Automation — v0.2.3

Terpasang pada PC pemilik dan diterapkan ke MCP produksi pada 15 Oktober 2026, waktu Asia/Bangkok.

## Penyebab dan perbaikan

Journal agent mengonfirmasi `Screenshot exceeds response budget`. Implementasi lama mengubah ukuran sekali ke maksimum 1280×800 lalu menolak PNG di atas 700.000 byte. Sekarang encoder memakai RGB, kompresi PNG dan pengecilan bertahap sampai gambar memenuhi batas tersebut. Gambar kecil tidak diperbesar. Dimensi layar virtual asli tetap dikirim untuk konversi koordinat mouse.

Journal juga mengonfirmasi `An exact automation_id or name is required` dan `UI selector must match exactly one element`. Fokus jendela kini dapat memakai HWND saja. Fokus elemen root memverifikasi HWND foreground, sehingga tidak mengharuskan root UIA memiliki keyboard focus.

Selector nama/ID persis sekarang mencari seluruh subtree jendela melalui kondisi UIA; pencarian tidak lagi dibatasi hasil inspeksi 200 elemen/depth 6. Nama/ID kosong diabaikan. Selektor yang tidak ada atau ambigu memberikan alasan yang berbeda dan meminta inspeksi ulang.

Inspeksi mengembalikan `element_path`, `focusable`, `enabled` dan `truncated`. Path dapat memilih kontrol dengan nama sama. Nama/ID tambahan memverifikasi identitas pada path. Path dapat berubah saat UI berubah, sehingga harus diambil ulang setelah perubahan. Kontrol password dan OpenCommander tetap dilindungi. Fokus kontrol membaca kembali keyboard focus dengan waktu tunggu singkat untuk provider asinkron.

## Verifikasi

- Rust: 21 tes lulus; Clippy bersih. Fixture gambar acak mereproduksi penolakan lama, lalu menghasilkan PNG valid dalam batas wire. Gambar kecil mempertahankan piksel.
- MCP: 7 tes lulus; lint, typecheck dan build berhasil.
- Produksi: 22 pemeriksaan lulus melalui OAuth dan JSON-RPC ke endpoint MCP sebenarnya, memakai agent terpasang v0.2.3.
- Screenshot sebenarnya: PNG valid 161.390 byte, 1280×800, untuk layar fisik 1920×1200. Pengujian memeriksa signature PNG, dimensi dan dekompresi IDAT tanpa memublikasikan gambar desktop.
- Fokus HWND tanpa selector dan fokus root berdasarkan nama berhasil. Dua editor dengan nama sama ditolak sebagai ambigu; path editor kedua berhasil dan keyboard focus dibaca kembali. Identitas path yang salah ditolak. Editor di bawah sembilan panel, di luar depth inspeksi, berhasil difokuskan dengan nama persis. Selector yang tidak ada tetap ditolak.
- Seluruh operasi pengujian berjalan tanpa dialog persetujuan per tool. Fixture dihentikan setelah selesai. Token refresh diagnostik dicabut; pairing perangkat pemilik tidak dicabut.

Bukti lokal: `artifacts/desktop-fix-results.json`. Pengujian ini memakai klien independen; pemuatan tool terbaru pada percakapan ChatGPT pengguna belum diverifikasi dari host ChatGPT.

## Instalasi dan rilis

Binary terpasang cocok dengan build v0.2.3. Pairing terenkripsi tetap sama dan `ask_before_sensitive: false` tetap aktif, dengan root profil `C:\Users\Administrator`. Shortcut Desktop `Jalankan OpenCommander.lnk` tetap menunjuk binary terpasang dengan `--background`.

Backup sebelum upgrade: `.runtime/desktop-fix-before-20261015-014336`.

Deployment MCP berhasil: `3e252ac6-56d4-4e28-a78c-5a3dcc6e0b6b`. Metadata rilis publik v0.2.3 diverifikasi dengan Ed25519; hash dan ukuran installer publik cocok. Executable belum ditandatangani Authenticode.

ZIP: `C:\Users\Administrator\Downloads\OpenCommander-Windows-0.2.3.zip`, berisi installer, metadata rilis, kunci publik dan laporan ini. Password, token dan data pairing tidak disertakan.
