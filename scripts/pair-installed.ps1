$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$exe=Join-Path $env:LOCALAPPDATA 'Programs/OpenCommander/OpenCommander.exe'
$state=Join-Path $env:LOCALAPPDATA OpenCommander
if(Test-Path (Join-Path $state 'credentials.dpapi')){throw 'Already paired; do not replace identity'}
$processes=Get-CimInstance Win32_Process -Filter "Name='OpenCommander.exe'"|Where-Object ExecutablePath -eq $exe
foreach($p in $processes){Stop-Process -Id $p.ProcessId}
$root=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OpenCommander Workspace'
$null=New-Item -ItemType Directory -Force -Path $root
$log=Join-Path $repo '.runtime/railway/installed-agent.log'
$errorLog=Join-Path $repo '.runtime/railway/installed-agent-error.log'
$arguments=@('--init','--ui','--gateway','https://gateway-production-0964.up.railway.app','--root',('"'+$root+'"'),'--application',(Join-Path $env:WINDIR 'System32/notepad.exe'),'--allow-modify','--allow-screenshots')
$agentProcess=Start-Process -FilePath $exe -ArgumentList $arguments -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $errorLog -PassThru
$deadline=[DateTime]::UtcNow.AddSeconds(30);$code=$null
while([DateTime]::UtcNow -lt $deadline -and !$code){
    foreach($line in @(Get-Content $log -ErrorAction SilentlyContinue)){try{$event=$line|ConvertFrom-Json;if($event.event -eq 'pairing_required'){$code=$event.code}}catch{}}
    if(!$code){Start-Sleep -Milliseconds 300}
}
if(!$code){throw 'Installed agent did not return a fresh pairing code'}
$protected=Get-Content (Join-Path $repo '.runtime/railway/secrets.dpapi.json') -Raw|ConvertFrom-Json -AsHashtable
$values=@{owner_password=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.OPENCOMMANDER_OWNER_PASSWORD)).Password;client_secret=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.OPENCOMMANDER_CLIENT_SECRET)).Password;pairing_code=$code}
Push-Location (Join-Path $repo 'apps/mcp-server')
try{($values|ConvertTo-Json -Compress)| & npx tsx test/railway.ts;if($LASTEXITCODE){throw 'Pairing verification failed'}}finally{Pop-Location}
$agentProcess.Refresh();if($agentProcess.HasExited){throw 'Installed agent exited after pairing'}
& $exe --status
