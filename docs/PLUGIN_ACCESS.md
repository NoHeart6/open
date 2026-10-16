# Akses plugin OpenCommander dan WordCraft

## ChatGPT melalui MCP remote

1. Unduh dan pasang agent Windows dari [rilis 0.5.1](https://github.com/NoHeart6/open/releases/tag/v0.5.1).
2. Buka UI OpenCommander, atur folder/aplikasi yang diizinkan, dan siapkan pairing perangkat.
3. Buka [Plugins ChatGPT](https://chatgpt.com/plugins). Jika akun/host menyediakan server MCP kustom, tambahkan:

   ```text
   https://mcp-production-394c.up.railway.app/mcp
   ```

4. Gunakan OAuth, login dengan akun OpenCommander Anda, tinjau scope, dan selesaikan pairing. Paket unduhan tidak berisi akun, password, token, atau identitas perangkat.
5. Mulai percakapan dengan `device_list`, lalu `system_info`. Pastikan perangkat online, agent versi 0.5.1, dan root file sesuai izin pemilik.

Sesudah schema/tools diperbarui, refresh koneksi MCP dan mulai percakapan baru. Daftar tools versi 0.5.1 berjumlah 43. Halaman Plugins adalah pintu masuk pengaturan; bukan tautan instalasi publik satu klik untuk paket ini. Penambahan MCP kustom bergantung pada fitur yang tersedia di akun/host.

## Paket plugin unduhan

[OpenCommander-plugin-0.5.1.zip](https://github.com/NoHeart6/open/releases/download/v0.5.1/OpenCommander-plugin-0.5.1.zip) berisi:

```text
plugin.json
mcp.json
skills/windows-control/SKILL.md
README.md
LICENSE
assets/OpenCommander_AppIcon.png
assets/OpenCommander_Icon.svg
```

Gunakan pada host yang mendukung manifest plugin tersebut. Mengunduh atau mengekstrak ZIP tidak dengan sendirinya menghubungkan ChatGPT; koneksi remote tetap memerlukan OAuth dan pairing. Isi manifest dapat diperiksa di [source plugin](../plugins/opencommander/).

Untuk klien MCP yang mendukung Streamable HTTP dan OAuth, konfigurasi dasarnya:

```json
{
  "mcpServers": {
    "opencommander": {
      "type": "streamable-http",
      "url": "https://mcp-production-394c.up.railway.app/mcp"
    }
  }
}
```

Ikuti login OAuth dari klien; jangan menaruh bearer token dalam konfigurasi yang dibagikan.

## Mengaktifkan WordCraft lewat OpenCommander

Unduh [WordCraft 0.7.0 portable](https://github.com/NoHeart6/open/releases/download/v0.5.1/wordcraft-0.7.0-windows-x64-portable.zip), ekstrak, lalu jalankan dari folder executable:

```powershell
.\wordcraft.exe --control 7981
```

Di UI OpenCommander, izinkan WordCraft/CLI untuk diluncurkan dan folder dokumen untuk dibaca/ditulis. Berikan path absolut CLI sesuai lokasi ekstraksi. Contoh payload tool `wordcraft` setelah mengganti placeholder dengan perangkat/path asli:

```json
{
  "device_id": "<device_id dari device_list>",
  "idempotency_key": "wordcraft-list-commands-001",
  "cli_path": "C:\\Apps\\WordCraft\\wordcraft-cli.exe",
  "port": 7981,
  "tool": "list_commands",
  "arguments": {}
}
```

`C:\Apps\WordCraft` adalah contoh; gunakan lokasi nyata dan working directory yang diizinkan. Periksa `task_status` sampai selesai, lalu baca hasil tool. Untuk membuat DOCX terstruktur, gunakan `wordcraft_compose`; untuk tindakan editor individual gunakan `wordcraft`. Rincian tools, transfer gambar, dan batas akses ada di [panduan Windows](INSTALL_WINDOWS.md).

WordCraft menyediakan MCP native lokal secara terpisah. Pada klien desktop yang dapat menjalankan proses lokal:

```powershell
.\wordcraft-cli.exe mcp
```

Untuk mengendalikan jendela WordCraft yang sudah dijalankan dengan `--control 7981`:

```powershell
.\wordcraft-cli.exe mcp --connect 127.0.0.1:7981
```

ChatGPT remote mengakses WordCraft melalui bridge OpenCommander. Kunci kontrol WordCraft tidak perlu disalin atau dipublikasikan. Dokumentasi upstream: [MCP WordCraft](https://github.com/storytold/wordcraft/blob/3a201c8246cadaae615bb20b5ece5ac117060c35/docs/mcp.md).
