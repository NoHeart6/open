$ErrorActionPreference='Stop'
$repoRoot=Split-Path $PSScriptRoot -Parent
$expected=(Get-Content -LiteralPath (Join-Path $repoRoot 'VERSION') -Raw).Trim()
if($expected -notmatch '^\d+\.\d+\.\d+$'){throw 'VERSION must contain a semantic version'}
$versions=[ordered]@{}
foreach($name in @('package.json','package-lock.json','apps/mcp-server/package.json','apps/mcp-server/package-lock.json','plugins/opencommander/plugin.json')){
    $manifest=Get-Content -LiteralPath (Join-Path $repoRoot $name) -Raw | ConvertFrom-Json -AsHashtable
    $versions[$name]=$manifest.version
    if($null -ne $manifest.packages){$versions["$name root package"]=$manifest.packages[''].version}
}
$cargo=Get-Content -LiteralPath (Join-Path $repoRoot 'apps/windows-agent/Cargo.toml') -Raw
$versions['agent manifest']=[regex]::Match($cargo,'(?m)^version = "([^"]+)"').Groups[1].Value
$cargoLock=Get-Content -LiteralPath (Join-Path $repoRoot 'apps/windows-agent/Cargo.lock') -Raw
$versions['agent lockfile']=[regex]::Match($cargoLock,'(?m)^name = "opencommander-agent"\r?\nversion = "([^"]+)"').Groups[1].Value
$gateway=Get-Content -LiteralPath (Join-Path $repoRoot 'apps/gateway/main.go') -Raw
$versions['gateway']=[regex]::Match($gateway,'const gatewayVersion = "([^"]+)"').Groups[1].Value
foreach($entry in $versions.GetEnumerator()){
    if($entry.Value -ne $expected){throw "Version mismatch: $($entry.Key) is $($entry.Value), expected $expected"}
}
Write-Output "OpenCommander version $expected is consistent across all component manifests."
