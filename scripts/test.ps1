param([switch]$Desktop)
$ErrorActionPreference='Stop'
$repoRoot=Split-Path $PSScriptRoot -Parent
if(Test-Path "$env:USERPROFILE\.cargo\bin"){$env:Path="$env:USERPROFILE\.cargo\bin;"+$env:Path}
if(Test-Path (Join-Path $repoRoot '.tools\w64devkit\bin')){$env:Path=(Join-Path $repoRoot '.tools\w64devkit\bin')+';'+$env:Path;$env:RUSTFLAGS='-C link-self-contained=yes'}
Push-Location (Join-Path $repoRoot 'apps\windows-agent')
try{cargo fmt --check;if($LASTEXITCODE){throw 'Rust format failed'};cargo clippy --locked --all-targets -- -D warnings;if($LASTEXITCODE){throw 'Clippy failed'};cargo test --locked;if($LASTEXITCODE){throw 'Rust tests failed'}}finally{Pop-Location}
Push-Location (Join-Path $repoRoot 'apps\gateway')
try{$goFiles=Get-ChildItem -File -Filter '*.go' | Select-Object -ExpandProperty FullName;$unformatted=gofmt -l $goFiles;if($LASTEXITCODE -or $unformatted){throw 'Go format failed'};go vet ./...;if($LASTEXITCODE){throw 'Go vet failed'};go test -count=1 ./...;if($LASTEXITCODE){throw 'Go tests failed'}}finally{Pop-Location}
Push-Location (Join-Path $repoRoot 'apps\mcp-server')
try{foreach($check in @('lint','typecheck','test')){npm run $check;if($LASTEXITCODE){throw "$check failed"}};npm audit;if($LASTEXITCODE){throw 'Dependency audit failed'};if($Desktop){npx tsx test/e2e.ts --desktop}else{npx tsx test/e2e.ts};if($LASTEXITCODE){throw 'End-to-end tests failed'}}finally{Pop-Location}
