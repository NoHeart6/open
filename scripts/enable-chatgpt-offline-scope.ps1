$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$state=Get-Content (Join-Path $repo '.runtime/railway/deployment.json') -Raw|ConvertFrom-Json
$protected=Get-Content (Join-Path $repo '.runtime/railway/secrets.dpapi.json') -Raw|ConvertFrom-Json -AsHashtable
$password=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.KC_BOOTSTRAP_ADMIN_PASSWORD)).Password
$admin=Invoke-RestMethod ($state.auth_url+'/realms/master/protocol/openid-connect/token') -Method Post -Body @{grant_type='password';client_id='admin-cli';username='opencommander-admin';password=$password}
$headers=@{Authorization='Bearer '+$admin.access_token}
$base=$state.auth_url+'/admin/realms/opencommander'
$scopes=Invoke-RestMethod ($base+'/client-scopes') -Headers $headers
$offline=@($scopes|Where-Object name -eq 'offline_access')
if($offline.Count -ne 1){throw 'Expected the existing offline_access scope'}
$role=Invoke-RestMethod ($base+'/roles/offline_access') -Headers $headers
$owners=@(Invoke-RestMethod ($base+'/users?username=owner&exact=true') -Headers $headers)
if($owners.Count -ne 1){throw 'Expected the existing owner account'}
$ownerId=$owners[0].id
$ownerRoles=Invoke-RestMethod ($base+'/users/'+$ownerId+'/role-mappings/realm') -Headers $headers
$scopeRoles=Invoke-RestMethod ($base+'/client-scopes/'+$offline[0].id+'/scope-mappings/realm') -Headers $headers
$clients=@()
foreach($clientId in @('https://chatgpt.com/oauth/client.json','opencommander-chatgpt')){
    $found=@(Invoke-RestMethod ($base+'/clients?clientId='+[Uri]::EscapeDataString($clientId)) -Headers $headers)
    if($found.Count -ne 1){throw ('Expected existing ChatGPT client '+$clientId)}
    $clients+=Invoke-RestMethod ($base+'/clients/'+$found[0].id) -Headers $headers
}
Add-Type -AssemblyName System.Security
$snapshot=@{at=[DateTime]::UtcNow.ToString('o');clients=$clients;owner_id=$ownerId;owner_roles_before=$ownerRoles;offline_scope_roles_before=$scopeRoles}|ConvertTo-Json -Depth 30
$encrypted=[Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($snapshot),$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)
[IO.File]::WriteAllBytes((Join-Path $repo ('.runtime/railway/chatgpt-offline-before-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'.dpapi')),$encrypted)
$effective=Invoke-RestMethod ($base+'/users/'+$ownerId+'/role-mappings/realm/composite') -Headers $headers
if('offline_access' -notin @($effective.name)){
    $null=Invoke-RestMethod ($base+'/users/'+$ownerId+'/role-mappings/realm') -Method Post -Headers $headers -ContentType 'application/json' -Body (ConvertTo-Json -InputObject @($role) -Depth 8)
}
if('offline_access' -notin @($scopeRoles.name)){
    $null=Invoke-RestMethod ($base+'/client-scopes/'+$offline[0].id+'/scope-mappings/realm') -Method Post -Headers $headers -ContentType 'application/json' -Body (ConvertTo-Json -InputObject @($role) -Depth 8)
}
foreach($client in $clients){
    $null=Invoke-RestMethod ($base+'/clients/'+$client.id+'/optional-client-scopes/'+$offline[0].id) -Method Put -Headers $headers
    $saved=Invoke-RestMethod ($base+'/clients/'+$client.id) -Headers $headers
    if('offline_access' -notin @($saved.optionalClientScopes)){throw 'Optional scope was not persisted'}
    foreach($field in @('clientId','publicClient','clientAuthenticatorType','fullScopeAllowed','consentRequired','redirectUris','webOrigins','defaultClientScopes','attributes')){
        if((ConvertTo-Json -InputObject $client.$field -Depth 12 -Compress) -cne (ConvertTo-Json -InputObject $saved.$field -Depth 12 -Compress)){throw ('Unrelated client configuration changed: '+$field)}
    }
}
$password=$null;$admin=$null;$headers=$null
Write-Output 'PASS offline_access enabled for the two existing ChatGPT clients; client security settings preserved; encrypted backup saved.'
