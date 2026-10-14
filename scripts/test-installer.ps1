$ErrorActionPreference='Stop'
$repoRoot=Split-Path $PSScriptRoot -Parent
$installed=Join-Path $env:LOCALAPPDATA 'Programs\OpenCommander'
$runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenCommander'
if(Test-Path -LiteralPath $installed){throw 'Existing installation left untouched'}
if(Get-ItemProperty -LiteralPath $runKey -Name OpenCommander -ErrorAction SilentlyContinue){throw 'Existing startup entry left untouched'}
if(Test-Path -LiteralPath $uninstallKey){throw 'Existing uninstall registration left untouched'}
$setup=Join-Path $repoRoot 'artifacts\OpenCommanderSetup.exe'
try{
 & $setup --install;if($LASTEXITCODE){throw 'Install failed'}
 $file=Join-Path $installed 'OpenCommander.exe'
 if(-not(Test-Path -LiteralPath $file)){throw 'Installed file missing'}
 if((Get-FileHash -LiteralPath $file).Hash -ne (Get-FileHash -LiteralPath $setup).Hash){throw 'Installed executable differs'}
 if((Get-ItemProperty -LiteralPath $runKey -Name OpenCommander).OpenCommander -ne ('"'+$file+'" --background')){throw 'Startup target mismatch'}
 if(-not(Test-Path -LiteralPath $uninstallKey)){throw 'Uninstall registration missing'}
}finally{& $setup --uninstall;if($LASTEXITCODE){throw 'Uninstall failed'}}
if((Test-Path -LiteralPath $installed) -or (Test-Path -LiteralPath $uninstallKey) -or (Get-ItemProperty -LiteralPath $runKey -Name OpenCommander -ErrorAction SilentlyContinue)){throw 'Uninstall verification failed'}
@{status='PASS';checks=5;account=$env:USERNAME;environment='existing development machine, not a clean VM';at=(Get-Date).ToUniversalTime().ToString('o')} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $repoRoot 'artifacts\installer-results.json')
Write-Output 'PASS native installation, exact binary hash, startup, uninstall registry, and complete uninstall'
