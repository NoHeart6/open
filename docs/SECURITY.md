# Keamanan OpenCommander

- OAuth 2.1 + PKCE S256, audience check (`aud` = MCP resource), scope enforcement per tool (`securitySchemes`).
- Owner-to-device isolation: setiap task bawa `owner_id`; gateway tolak lintas owner (403).
- Device credential: `device_id` + 256-bit token, simpan di Windows Credential Manager (DPAPI). v0.1 simpan `device_id.txt` di PROGRAMDATA + token via env; JANGAN hardcode.
- TLS wajib di produksi; mTLS OpenAI (`mtls.prod.connectors.openai.com`) didukung di gateway bila diaktifkan.
- Local policy: operasi `file_manage/delete`, `process_manage/kill` wajib `approve:true` + consent native (`rfd` dialog) di agent.
- Emergency pause: tray → Pause; gateway kembalikan 423 untuk dispatch baru.
- Revocation: hapus device di gateway + hapus kredensial di Credential Manager.
- Prompt-injection: instruksi dari file/browser/terminal TIDAK pernah dianggap perintah owner; hanya tool call terautentikasi dari ChatGPT yang dieksekusi.
- Audit: setiap tool log JSON (tool, owner, ok, ms); task journal persisten di gateway.
- Redaction: token/secret tidak masuk tool result; screenshot hanya PNG base64 sesuai permintaan.
- Update aman: metadata rilis bertanda + checksum SHA256, verifikasi sebelum instal; pairing dipertahankan; rollback bila gagal.
