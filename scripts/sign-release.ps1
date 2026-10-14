$ErrorActionPreference='Stop'
$repoRoot=Split-Path $PSScriptRoot -Parent
$keyRoot=Join-Path $repoRoot '.runtime/releases'
$ciphertext=Get-Content -LiteralPath (Join-Path $keyRoot 'release-key.dpapi.txt') -Raw
$private=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $ciphertext.Trim())).Password
$version=(Select-String -LiteralPath (Join-Path $repoRoot 'apps/windows-agent/Cargo.toml') -Pattern '^version = "([0-9.]+)"$').Matches[0].Groups[1].Value
$package=Join-Path $repoRoot 'artifacts/OpenCommanderSetup.exe'
$issued=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$payload=@{version=$version;platform='windows-x86_64';url='https://mcp-production-394c.up.railway.app/releases/windows/OpenCommanderSetup.exe';sha256=(Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLower();size=(Get-Item -LiteralPath $package).Length;issued_at=$issued;expires_at=$issued+15552000}
$metadata=(@{private_key_base64=$private;payload=$payload} | ConvertTo-Json -Depth 5 -Compress) | & node (Join-Path $PSScriptRoot 'release-signing.mjs') sign
$private=$null
if($LASTEXITCODE){throw 'Release signing failed'}
$folder=Join-Path $repoRoot 'releases/windows'
New-Item -ItemType Directory -Force -Path $folder | Out-Null
Copy-Item -LiteralPath $package -Destination (Join-Path $folder 'OpenCommanderSetup.exe')
[IO.File]::WriteAllText((Join-Path $folder 'stable.json'),$metadata)
Write-Output "Signed release metadata and package prepared for $version. This does not provide Authenticode signing."
