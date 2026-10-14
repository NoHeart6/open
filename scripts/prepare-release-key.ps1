$ErrorActionPreference='Stop'
$repoRoot=Split-Path $PSScriptRoot -Parent
$folder=Join-Path $repoRoot '.runtime/releases'
New-Item -ItemType Directory -Force -Path $folder | Out-Null
$privatePath=Join-Path $folder 'release-key.dpapi.txt'
$publicPath=Join-Path $folder 'release-public-key.json'
if(!(Test-Path -LiteralPath $privatePath)){
 $key=(& node (Join-Path $PSScriptRoot 'release-signing.mjs') generate | ConvertFrom-Json)
 if($LASTEXITCODE){throw 'Release key generation failed'}
 ConvertFrom-SecureString (ConvertTo-SecureString $key.private_key_base64 -AsPlainText -Force) | Set-Content -LiteralPath $privatePath
 @{algorithm='Ed25519';public_key_hex=$key.public_key_hex;created_at=(Get-Date).ToUniversalTime().ToString('o')} | ConvertTo-Json | Set-Content -LiteralPath $publicPath
 $key=$null
}
$public=Get-Content -LiteralPath $publicPath -Raw | ConvertFrom-Json
if($public.public_key_hex -notmatch '^[a-f0-9]{64}$'){throw 'Invalid release public key'}
$env:OPENCOMMANDER_RELEASE_PUBLIC_KEY=$public.public_key_hex
Write-Output 'Release verification key loaded; private key remains DPAPI protected.'
