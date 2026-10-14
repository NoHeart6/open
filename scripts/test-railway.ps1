param([switch]$Restart)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$protected=Get-Content -LiteralPath (Join-Path $repo '.runtime/railway/secrets.dpapi.json') -Raw | ConvertFrom-Json -AsHashtable
$values=@{
    owner_password=[System.Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.OPENCOMMANDER_OWNER_PASSWORD)).Password
    client_secret=[System.Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.OPENCOMMANDER_CLIENT_SECRET)).Password
}
Push-Location (Join-Path $repo 'apps/mcp-server')
try{
    $env:RAILWAY_CLI_EXE=Join-Path $env:APPDATA 'npm/node_modules/@railway/cli/bin/railway.exe'
    if($Restart -and !(Test-Path -LiteralPath $env:RAILWAY_CLI_EXE)){throw 'Cannot find native Railway CLI for restart verification'}
    $argsList=@('tsx','test/railway.ts')
    if($Restart){$argsList+='--restart'}
    ($values | ConvertTo-Json -Compress) | & npx @argsList
    if($LASTEXITCODE -ne 0){throw 'Railway verification failed; inspect artifacts/railway-results.json'}
}finally{Pop-Location}
