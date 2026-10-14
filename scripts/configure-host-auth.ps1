param([Parameter(Mandatory=$true)][string]$CallbackUrl)
$ErrorActionPreference='Stop'
$callback=[uri]$CallbackUrl
if($callback.Scheme -ne 'http' -or $callback.Host -ne '127.0.0.1' -or $callback.Port -lt 1024 -or $callback.UserInfo -or $callback.Query -or $callback.Fragment -or $callback.AbsolutePath -notmatch '^/callback(?:/[A-Za-z0-9_-]+)?$'){throw 'Use the exact loopback callback printed by the host; wildcard redirects are forbidden'}
$repo=Split-Path -Parent $PSScriptRoot
$state=Get-Content (Join-Path $repo '.runtime/railway/deployment.json') -Raw|ConvertFrom-Json
$secrets=Get-Content (Join-Path $repo '.runtime/railway/secrets.dpapi.json') -Raw|ConvertFrom-Json -AsHashtable
$password=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $secrets.KC_BOOTSTRAP_ADMIN_PASSWORD)).Password
$token=Invoke-RestMethod ($state.auth_url+'/realms/master/protocol/openid-connect/token') -Method Post -Body @{grant_type='password';client_id='admin-cli';username='opencommander-admin';password=$password}
$headers=@{Authorization='Bearer '+$token.access_token}
$base=$state.auth_url+'/admin/realms/opencommander'
$existing=Invoke-RestMethod ($base+'/clients?clientId=opencommander-host') -Headers $headers
$before=if($existing.Count){Invoke-RestMethod ($base+'/clients/'+$existing[0].id) -Headers $headers}else{$null}
# A protected snapshot allows rollback without exporting any credentials to logs.
Add-Type -AssemblyName System.Security
$snapshot=@{at=[DateTime]::UtcNow.ToString('o');clientId='opencommander-host';before=$before}|ConvertTo-Json -Depth 30
$encrypted=[Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($snapshot),$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)
$backup=Join-Path $repo ('.runtime/railway/host-auth-before-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'.dpapi')
[IO.File]::WriteAllBytes($backup,$encrypted)
$redirects=@('https://chatgpt.com/connector_platform_oauth_redirect',$CallbackUrl)
$client=@{clientId='opencommander-host';name='OpenCommander host read-only';enabled=$true;protocol='openid-connect';publicClient=$true;standardFlowEnabled=$true;implicitFlowEnabled=$false;directAccessGrantsEnabled=$false;serviceAccountsEnabled=$false;consentRequired=$true;fullScopeAllowed=$false;redirectUris=$redirects;webOrigins=@();defaultClientScopes=@('basic','computer:read');optionalClientScopes=@();attributes=@{'pkce.code.challenge.method'='S256';'use.refresh.tokens'='true'}}
$body=$client|ConvertTo-Json -Depth 12
if($before){$null=Invoke-RestMethod ($base+'/clients/'+$before.id) -Method Put -Headers $headers -ContentType 'application/json' -Body $body}else{$null=Invoke-RestMethod ($base+'/clients') -Method Post -Headers $headers -ContentType 'application/json' -Body $body}
$saved=Invoke-RestMethod ($base+'/clients?clientId=opencommander-host') -Headers $headers
$saved=Invoke-RestMethod ($base+'/clients/'+$saved[0].id) -Headers $headers
if(!$saved.publicClient -or $saved.attributes.'pkce.code.challenge.method' -ne 'S256' -or $saved.directAccessGrantsEnabled -or @($saved.redirectUris|Where-Object {$_ -like '*`**'}).Count){throw 'Host client policy verification failed'}
@{at=[DateTime]::UtcNow.ToString('o');clientId=$saved.clientId;pkce='S256';defaultScopes=$saved.defaultClientScopes;redirectUris=$saved.redirectUris;publicClient=$saved.publicClient;consentRequired=$saved.consentRequired}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $repo 'artifacts/host-auth-config.json')
Write-Output 'PASS Host OAuth client registered with exact callback, PKCE S256 and read-only scope; protected backup created.'
