param([switch]$Release)
$ErrorActionPreference='Stop'
$repoRoot=Split-Path $PSScriptRoot -Parent
New-Item -ItemType Directory -Path (Join-Path $repoRoot artifacts) -Force | Out-Null
if(Test-Path (Join-Path $repoRoot 'infra/releases/trusted-key.json')){$env:OPENCOMMANDER_RELEASE_PUBLIC_KEY=(Get-Content (Join-Path $repoRoot 'infra/releases/trusted-key.json') -Raw | ConvertFrom-Json).public_key_hex}
if(Test-Path "$env:USERPROFILE\.cargo\bin"){$env:Path="$env:USERPROFILE\.cargo\bin;"+$env:Path}
if(!$env:RUSTFLAGS -and ((& rustc -vV) -match 'host: .*windows-gnu')){$env:RUSTFLAGS='-C link-self-contained=yes'}
if(Test-Path (Join-Path $repoRoot '.tools\w64devkit\bin')){$env:Path=(Join-Path $repoRoot '.tools\w64devkit\bin')+';'+$env:Path;$env:RUSTFLAGS='-C link-self-contained=yes'}
Push-Location (Join-Path $repoRoot 'apps\windows-agent')
try{if($Release){cargo build --locked --release;$profile='release'}else{cargo build --locked;$profile='debug'};if($LASTEXITCODE){throw 'Rust build failed'};Copy-Item -LiteralPath "target\$profile\opencommander-agent.exe" -Destination (Join-Path $repoRoot 'artifacts\opencommander-agent.exe')}finally{Pop-Location}
Push-Location (Join-Path $repoRoot 'apps\gateway')
try{go build -trimpath -o (Join-Path $repoRoot 'artifacts\opencommander-gateway.exe') .;if($LASTEXITCODE){throw 'Go build failed'};$env:GOOS='linux';$env:CGO_ENABLED='0';go build -trimpath -o (Join-Path $repoRoot 'artifacts\opencommander-gateway-linux-amd64') .;if($LASTEXITCODE){throw 'Linux cross build failed'}}finally{Remove-Item Env:GOOS -ErrorAction SilentlyContinue;Remove-Item Env:CGO_ENABLED -ErrorAction SilentlyContinue;Pop-Location}
Push-Location (Join-Path $repoRoot 'apps\mcp-server')
try{npm ci;if($LASTEXITCODE){throw 'npm install failed'};npm run build;if($LASTEXITCODE){throw 'TypeScript build failed'}}finally{Pop-Location}
Copy-Item -LiteralPath (Join-Path $repoRoot 'artifacts\opencommander-agent.exe') -Destination (Join-Path $repoRoot 'artifacts\OpenCommanderSetup.exe')
Copy-Item -LiteralPath (Join-Path $repoRoot 'artifacts\opencommander-agent.exe') -Destination (Join-Path $repoRoot 'artifacts\OpenCommander.exe')
$files=Get-ChildItem -LiteralPath (Join-Path $repoRoot artifacts) -File | Where-Object {$_.Name -ne 'SHA256SUMS.txt' -and $_.Extension -ne '.json'}
$files | ForEach-Object {"$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)"} | Set-Content -LiteralPath (Join-Path $repoRoot 'artifacts\SHA256SUMS.txt')
Write-Output 'Artifacts built. The native setup executable supports --install and --uninstall.'
