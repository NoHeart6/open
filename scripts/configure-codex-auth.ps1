$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$state=Get-Content (Join-Path $repo '.runtime/railway/deployment.json') -Raw|ConvertFrom-Json
$secrets=Get-Content (Join-Path $repo '.runtime/railway/secrets.dpapi.json') -Raw|ConvertFrom-Json -AsHashtable
$metadata=Invoke-RestMethod 'https://chatgpt.com/oauth/codex/client.json'
if($metadata.client_id -ne 'https://chatgpt.com/oauth/codex/client.json' -or $metadata.token_endpoint_auth_method -ne 'none' -or $metadata.application_type -ne 'native' -or (@($metadata.redirect_uris)|Sort-Object) -join ',' -ne 'http://127.0.0.1/callback,http://localhost/callback'){throw 'Official Codex metadata changed; review before applying'}
$password=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $secrets.KC_BOOTSTRAP_ADMIN_PASSWORD)).Password
$token=Invoke-RestMethod ($state.auth_url+'/realms/master/protocol/openid-connect/token') -Method Post -Body @{grant_type='password';client_id='admin-cli';username='opencommander-admin';password=$password}
$headers=@{Authorization='Bearer '+$token.access_token}
$base=$state.auth_url+'/admin/realms/opencommander'
$existing=Invoke-RestMethod ($base+'/clients?clientId='+[Uri]::EscapeDataString($metadata.client_id)) -Headers $headers
$before=if($existing.Count){Invoke-RestMethod ($base+'/clients/'+$existing[0].id) -Headers $headers}else{$null}
$owners=Invoke-RestMethod ($base+'/users?username=owner&exact=true') -Headers $headers
if(@($owners).Count -ne 1){throw 'Expected the existing owner account'}
$ownerId=$owners[0].id
$ownerRolesBefore=Invoke-RestMethod ($base+'/users/'+$ownerId+'/role-mappings/realm') -Headers $headers
$scopes=Invoke-RestMethod ($base+'/client-scopes') -Headers $headers
$basic=$scopes|Where-Object name -eq basic
if(!$basic){throw 'Expected existing basic scope'}
$basicBefore=Invoke-RestMethod ($base+'/client-scopes/'+$basic.id) -Headers $headers
$audienceMappers=@($basicBefore.protocolMappers|Where-Object {$_.protocolMapper -eq 'oidc-audience-mapper' -and $_.name -eq 'MCP resource audience'})
if($audienceMappers.Count -ne 1){throw 'Expected the existing MCP audience mapper'}
Add-Type -AssemblyName System.Security
$snapshot=@{at=[DateTime]::UtcNow.ToString('o');clientId=$metadata.client_id;before=$before;owner_id=$ownerId;owner_roles_before=$ownerRolesBefore;basic_scope_before=$basicBefore}|ConvertTo-Json -Depth 30
$encrypted=[Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($snapshot),$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)
[IO.File]::WriteAllBytes((Join-Path $repo ('.runtime/railway/codex-auth-before-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'.dpapi')),$encrypted)
# Keycloak requires both the optional scope and this specific realm role on the user.
# No administrative role or fullScopeAllowed grant is added.
$offlineRole=Invoke-RestMethod ($base+'/roles/offline_access') -Headers $headers
if('offline_access' -notin @($ownerRolesBefore.name)){
    $null=Invoke-RestMethod ($base+'/users/'+$ownerId+'/role-mappings/realm') -Method Post -Headers $headers -ContentType 'application/json' -Body (ConvertTo-Json -InputObject @($offlineRole) -Depth 8)
}
$effectiveRoles=Invoke-RestMethod ($base+'/users/'+$ownerId+'/role-mappings/realm/composite') -Headers $headers
if('offline_access' -notin @($effectiveRoles.name)){throw 'Owner offline-token permission verification failed'}
# The resource-indicators feature loses the URI audience on offline refresh.
# Bind every MCP token, including refreshed tokens, to this exact protected URI.
$mapper=$audienceMappers[0]
$mapper.config.PSObject.Properties.Remove('included.client.audience')
$mapper.config|Add-Member -NotePropertyName 'included.custom.audience' -NotePropertyValue $state.mcp_url -Force
$null=Invoke-RestMethod ($base+'/client-scopes/'+$basic.id+'/protocol-mappers/models/'+$mapper.id) -Method Put -Headers $headers -ContentType 'application/json' -Body ($mapper|ConvertTo-Json -Depth 12)
$savedMapper=Invoke-RestMethod ($base+'/client-scopes/'+$basic.id+'/protocol-mappers/models/'+$mapper.id) -Headers $headers
if($savedMapper.config.'included.custom.audience' -ne $state.mcp_url -or $savedMapper.config.'included.client.audience'){throw 'Exact resource audience mapper verification failed'}
$client=@{clientId=$metadata.client_id;name='Codex OpenCommander';enabled=$true;protocol='openid-connect';publicClient=$true;standardFlowEnabled=$true;implicitFlowEnabled=$false;directAccessGrantsEnabled=$false;serviceAccountsEnabled=$false;consentRequired=$true;fullScopeAllowed=$false;redirectUris=@($metadata.redirect_uris);webOrigins=@();defaultClientScopes=@('basic','computer:read');optionalClientScopes=@('computer:control','offline_access');attributes=@{'pkce.code.challenge.method'='S256';'use.refresh.tokens'='true'}}
$body=$client|ConvertTo-Json -Depth 12
if($before){$null=Invoke-RestMethod ($base+'/clients/'+$before.id) -Method Put -Headers $headers -ContentType 'application/json' -Body $body}else{$null=Invoke-RestMethod ($base+'/clients') -Method Post -Headers $headers -ContentType 'application/json' -Body $body}
$saved=Invoke-RestMethod ($base+'/clients?clientId='+[Uri]::EscapeDataString($metadata.client_id)) -Headers $headers
$scopes=Invoke-RestMethod ($base+'/client-scopes') -Headers $headers
foreach($scopeName in $client.optionalClientScopes){
    $scope=$scopes|Where-Object name -eq $scopeName
    if(!$scope){throw ('Missing configured scope '+$scopeName)}
    $null=Invoke-RestMethod ($base+'/clients/'+$saved[0].id+'/optional-client-scopes/'+$scope.id) -Method Put -Headers $headers
}
$saved=Invoke-RestMethod ($base+'/clients/'+$saved[0].id) -Headers $headers
if(!$saved.publicClient -or $saved.attributes.'pkce.code.challenge.method' -ne 'S256' -or $saved.directAccessGrantsEnabled -or 'offline_access' -notin $saved.optionalClientScopes -or @($saved.redirectUris|Where-Object {$_.Contains('*')}).Count){throw 'Codex client policy verification failed'}
$checks=@()
$handler=[Net.Http.HttpClientHandler]::new();$handler.AllowAutoRedirect=$false
$http=[Net.Http.HttpClient]::new($handler)
foreach($case in @(@{name='native loopback with ephemeral port';callback='http://127.0.0.1:49197/callback';pkce=$true;allowed=$true},@{name='foreign callback';callback='https://example.invalid/callback';pkce=$true;allowed=$false},@{name='wrong callback path';callback='http://127.0.0.1:49197/other';pkce=$true;allowed=$false},@{name='missing PKCE';callback='http://127.0.0.1:49197/callback';pkce=$false;allowed=$false})){
    $query=@{client_id=$metadata.client_id;redirect_uri=$case.callback;response_type='code';scope='computer:read computer:control offline_access';resource=$state.mcp_url;state=[guid]::NewGuid().ToString()}
    if($case.pkce){$query.code_challenge=[Convert]::ToBase64String([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes([guid]::NewGuid().ToString()))).TrimEnd('=').Replace('+','-').Replace('/','_');$query.code_challenge_method='S256'}
    $encoded=($query.GetEnumerator()|ForEach-Object {[uri]::EscapeDataString($_.Key)+'='+[uri]::EscapeDataString($_.Value)}) -join '&'
    $response=$http.GetAsync($state.auth_url+'/realms/opencommander/protocol/openid-connect/auth?'+$encoded).GetAwaiter().GetResult()
    $content=$response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    $login=[int]$response.StatusCode -eq 200 -and $content -match 'kc-form-login'
    if($login -ne $case.allowed){$errorOnly=if($response.Headers.Location){($response.Headers.Location.Query.TrimStart('?').Split('&')|Where-Object {$_ -match '^error(?:_description)?='}) -join '&'}else{''};throw ('Authorization regression failed: '+$case.name+' status '+$response.StatusCode+' '+$errorOnly)}
    $checks+=@{name=$case.name;status='PASS';http_status=[int]$response.StatusCode;login_form=$login}
}
$http.Dispose()
@{at=[DateTime]::UtcNow.ToString('o');clientId=$saved.clientId;pkce='S256';defaultScopes=$saved.defaultClientScopes;optionalScopes=$saved.optionalClientScopes;redirectUris=$saved.redirectUris;publicClient=$saved.publicClient;consentRequired=$saved.consentRequired;checks=$checks}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $repo 'artifacts/codex-auth-config.json')
Write-Output 'PASS Official Codex client registered; ephemeral loopback callback accepted; foreign/path redirects and missing PKCE rejected.'
