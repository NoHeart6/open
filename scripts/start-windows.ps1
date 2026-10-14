$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$agent=Join-Path $env:LOCALAPPDATA 'Programs/OpenCommander/OpenCommander.exe'
if(!(Test-Path -LiteralPath $agent)){
    & (Join-Path $repo 'artifacts/OpenCommanderSetup.exe') --install
    if($LASTEXITCODE){throw 'Native installer failed'}
}
$root=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OpenCommander Workspace'
$null=New-Item -ItemType Directory -Path $root -Force
$state=Join-Path $env:LOCALAPPDATA 'OpenCommander'
$running=Get-CimInstance Win32_Process -Filter "Name='OpenCommander.exe'"|Where-Object ExecutablePath -eq $agent
if($running){
    if(Test-Path -LiteralPath (Join-Path $state 'credentials.dpapi')){& $agent --status}else{Write-Output 'Agent sudah menunggu pairing; gunakan kode pada sesi yang sudah berjalan.'}
    return
}
if(Test-Path -LiteralPath (Join-Path $state 'credentials.dpapi')){
    & $agent --background
}else{
    & $agent --init --gateway 'https://gateway-production-0964.up.railway.app' --root $root --application (Join-Path $env:WINDIR 'System32/notepad.exe') --allow-modify --allow-screenshots
}
if($LASTEXITCODE){throw 'Agent stopped with an error'}
