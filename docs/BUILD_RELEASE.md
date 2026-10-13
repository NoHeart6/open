# Build & rilis

## Prasyarat (Windows)

- Rust stable + MinGW dlltool (lihat riwayat: `winget install BrechtSanders.WinLibs.POSIX.UCRT`), Go 1.23+, Node 24+, NSIS.

## Build lokal

```powershell
$env:PATH = 'C:\...\mingw64\bin;C:\Users\Administrator\.cargo\bin;' + $env:PATH
cd agent-rust; cargo test; cargo build --release        # → target\release\OpenCommander.exe
cd ..\gateway-go; go test ./...; go build ./...
cd ..\mcp-ts; npm install; npx tsc; node --test dist/*.test.js
```

## Rilis Windows (unsigned dev)

```powershell
cd agent-rust; cargo build --release
cd ..\installer; makensis setup.nsi   # → OpenCommanderSetup.exe
```

Status tanda tangan: **UNSIGNED development release** sampai sertifikat code signing tersedia. Jangan klaim signed.

## Server (Railway)

1. Salin `deploy/.env.example` → isi `MCP_PUBLIC_URL`, `OAUTH_ISSUER`, `GATEWAY_TOKEN`.
2. Deploy gateway + mcp via `deploy/docker-compose.yml` atau `railway.toml`.
3. Verifikasi: `GET /health`, `GET /.well-known/oauth-protected-resource`, MCP Inspector ke `/mcp`.
4. Sambungkan di ChatGPT: Plugins → Add custom MCP server → `https://<host>/mcp`.
