$ErrorActionPreference='Stop'
$repoRoot=Split-Path $PSScriptRoot -Parent
$artifactRoot=Join-Path $repoRoot artifacts
$licenseRoot=Join-Path $artifactRoot 'licenses'
New-Item -ItemType Directory -Force -Path $licenseRoot | Out-Null
function Copy-LicenseFiles([string]$Directory,[string]$Label){
 if(-not(Test-Path -LiteralPath $Directory)){return}
 $files=Get-ChildItem -LiteralPath $Directory -File | Where-Object {$_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)'}
 if($files){$safe=$Label -replace '[^A-Za-z0-9_.-]','_';$destination=Join-Path $licenseRoot $safe;New-Item -ItemType Directory -Force -Path $destination | Out-Null;foreach($file in $files){Copy-Item -LiteralPath $file.FullName -Destination $destination -Force;(Get-Item -LiteralPath (Join-Path $destination $file.Name)).IsReadOnly=$false}}
}
if(Test-Path "$env:USERPROFILE\.cargo\bin"){$env:Path="$env:USERPROFILE\.cargo\bin;"+$env:Path}
Push-Location (Join-Path $repoRoot 'apps\windows-agent')
try{$metadata=(cargo metadata --locked --format-version 1 | ConvertFrom-Json -AsHashtable);if($LASTEXITCODE){throw 'Cargo license inventory failed'};foreach($p in $metadata.packages){Copy-LicenseFiles (Split-Path $p.manifest_path -Parent) ("rust_"+$p.name+'_'+$p.version)}}finally{Pop-Location}
$rustRoot=rustc --print sysroot
Copy-LicenseFiles $rustRoot 'rust-toolchain'
$runtimeNotice=Join-Path $repoRoot '.tools\w64devkit\COPYING.MinGW-w64-runtime.txt'
if(Test-Path -LiteralPath $runtimeNotice){Copy-Item -LiteralPath $runtimeNotice -Destination $licenseRoot}
Push-Location (Join-Path $repoRoot 'apps\gateway')
try{$modules=go list -m -f '{{.Path}}|{{.Version}}|{{.Dir}}' all;if($LASTEXITCODE){throw 'Go license inventory failed'};foreach($line in $modules){$fields=$line.Split('|');if($fields.Count -eq 3){Copy-LicenseFiles $fields[2] ('go_'+$fields[0]+'_'+$fields[1])}};$goRoot=go env GOROOT;Copy-LicenseFiles $goRoot 'go-toolchain'}finally{Pop-Location}
$agent=Join-Path $artifactRoot opencommander-agent.exe
$setup=Join-Path $artifactRoot OpenCommanderSetup.exe
Copy-Item -LiteralPath (Join-Path $repoRoot LICENSE) -Destination $artifactRoot
Copy-Item -LiteralPath (Join-Path $repoRoot 'THIRD_PARTY_NOTICES.md') -Destination $artifactRoot
Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\INSTALL_WINDOWS.md') -Destination (Join-Path $artifactRoot 'INSTALL_WINDOWS.md')
Compress-Archive -LiteralPath $agent,$setup,(Join-Path $artifactRoot LICENSE),(Join-Path $artifactRoot THIRD_PARTY_NOTICES.md),(Join-Path $artifactRoot INSTALL_WINDOWS.md),$licenseRoot -DestinationPath (Join-Path $artifactRoot 'opencommander-0.1.0-windows-x64.zip') -Force
$files=Get-ChildItem -LiteralPath $artifactRoot -File | Where-Object {$_.Extension -eq '.exe' -or $_.Extension -eq '.zip' -or $_.Name -eq 'opencommander-gateway-linux-amd64'}
$files | ForEach-Object {"$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)"} | Set-Content -LiteralPath (Join-Path $artifactRoot 'SHA256SUMS.txt')
Write-Output "Packaged unsigned Windows binaries with dependency notices and checksums. No fixture screenshots, tokens, database or state were included."
