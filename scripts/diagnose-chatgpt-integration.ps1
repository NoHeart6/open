param([switch]$CodexOAuth,[switch]$ProfileAccess,[switch]$Unattended,[switch]$Desktop,[switch]$AllApplications,[switch]$Usability,[switch]$Screenshot,[switch]$Terminal,[switch]$Tui,[switch]$Wordcraft,[switch]$Image,[switch]$Media)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$secretFile=Join-Path $repo '.runtime/railway/secrets.dpapi.json'
$credentials=@{}
$credentials.verify_profile_access=[bool]($ProfileAccess -or $Unattended)
$credentials.verify_unattended=[bool]$Unattended
$credentials.verify_desktop=[bool]$Desktop
$credentials.verify_all_applications=[bool]$AllApplications
$credentials.verify_usability=[bool]$Usability
$credentials.verify_screenshot=[bool]$Screenshot
$credentials.verify_terminal=[bool]$Terminal
$credentials.verify_tui=[bool]$Tui
$credentials.verify_wordcraft=[bool]$Wordcraft
$credentials.verify_image=[bool]$Image
$credentials.verify_media=[bool]$Media
if(Test-Path -LiteralPath $secretFile){
    $protected=Get-Content -LiteralPath $secretFile -Raw|ConvertFrom-Json
    $credentials.owner_password=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.OPENCOMMANDER_OWNER_PASSWORD)).Password
    $credentials.client_secret=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.OPENCOMMANDER_CLIENT_SECRET)).Password
}
Push-Location (Join-Path $repo 'apps/mcp-server')
try{
    # Credentials stay in memory and are passed through stdin, never command-line arguments.
    $testFile=if($CodexOAuth){'test/codex-auth.ts'}else{'test/diagnose.ts'}
    ($credentials|ConvertTo-Json -Compress)|& npx tsx $testFile
    if($LASTEXITCODE -ne 0){$evidence=if($CodexOAuth){'codex-oauth-diagnostics.json'}elseif($Wordcraft){'wordcraft-results.json'}elseif($Tui){'tui-results.json'}elseif($Terminal){'terminal-results.json'}elseif($Screenshot){'screenshot-delivery-results.json'}elseif($Usability){'usability-results.json'}elseif($AllApplications){'all-applications-results.json'}elseif($Desktop){'desktop-fix-results.json'}elseif($Unattended){'unattended-access-results.json'}elseif($ProfileAccess){'profile-access-results.json'}else{'chatgpt-integration-diagnostics.json'};throw ('Diagnosis failed; sanitized evidence is in artifacts/'+$evidence)}
}finally{$credentials.Clear();Pop-Location}
