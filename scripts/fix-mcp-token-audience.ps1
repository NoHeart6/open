$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$state=Get-Content (Join-Path $repo '.runtime/railway/deployment.json') -Raw|ConvertFrom-Json
$protected=Get-Content (Join-Path $repo '.runtime/railway/secrets.dpapi.json') -Raw|ConvertFrom-Json -AsHashtable
$password=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.KC_BOOTSTRAP_ADMIN_PASSWORD)).Password
$admin=Invoke-RestMethod ($state.auth_url+'/realms/master/protocol/openid-connect/token') -Method Post -Body @{grant_type='password';client_id='admin-cli';username='opencommander-admin';password=$password}
$headers=@{Authorization='Bearer '+$admin.access_token};$base=$state.auth_url+'/admin/realms/opencommander'
$scopes=Invoke-RestMethod ($base+'/client-scopes') -Headers $headers
$basic=@($scopes|Where-Object name -eq 'basic')
if($basic.Count -ne 1){throw 'Expected the existing basic client scope'}
$scope=Invoke-RestMethod ($base+'/client-scopes/'+$basic[0].id) -Headers $headers
$clients=@(Invoke-RestMethod ($base+'/clients?clientId=opencommander-chatgpt') -Headers $headers)
if($clients.Count -ne 1){throw 'Expected the existing ChatGPT test client'}
$legacy=Invoke-RestMethod ($base+'/clients/'+$clients[0].id) -Headers $headers
$changes=@()
foreach($source in @(@{mappers=$scope.protocolMappers;path='/client-scopes/'+$scope.id},@{mappers=$legacy.protocolMappers;path='/clients/'+$legacy.id})){
    $found=@($source.mappers|Where-Object {$_.name -eq 'MCP resource audience' -and $_.protocolMapper -eq 'oidc-audience-mapper'})
    if($found.Count -ne 1){throw 'Expected one existing resource audience mapper'}
    $changes+=@{path=$source.path+'/protocol-mappers/models/'+$found[0].id;before=$found[0]}
}
Add-Type -AssemblyName System.Security
$snapshot=ConvertTo-Json -InputObject $changes -Depth 20
$encrypted=[Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($snapshot),$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)
[IO.File]::WriteAllBytes((Join-Path $repo ('.runtime/railway/audience-before-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'.dpapi')),$encrypted)
foreach($change in $changes){
    $mapper=$change.before
    $mapper.config.PSObject.Properties.Remove('included.client.audience')
    $mapper.config|Add-Member -NotePropertyName 'included.custom.audience' -NotePropertyValue $state.mcp_url -Force
    $null=Invoke-RestMethod ($base+$change.path) -Method Put -Headers $headers -ContentType 'application/json' -Body ($mapper|ConvertTo-Json -Depth 12)
    $saved=Invoke-RestMethod ($base+$change.path) -Headers $headers
    if($saved.config.'included.custom.audience' -ne $state.mcp_url -or $saved.config.'included.client.audience'){throw 'Resource audience mapper read-back failed'}
}
$owner=@(Invoke-RestMethod ($base+'/users?username=owner&exact=true') -Headers $headers)[0]
$checks=@()
foreach($clientId in @('https://chatgpt.com/oauth/client.json','opencommander-chatgpt')){
    $client=@(Invoke-RestMethod ($base+'/clients?clientId='+[Uri]::EscapeDataString($clientId)) -Headers $headers)[0]
    foreach($scopeNames in @('openid computer:read','openid offline_access computer:read')){
        $claims=Invoke-RestMethod ($base+'/clients/'+$client.id+'/evaluate-scopes/generate-example-access-token?userId='+$owner.id+'&scope='+[Uri]::EscapeDataString($scopeNames)) -Headers $headers
        if($state.mcp_url -notin @($claims.aud) -or $claims.sub -ne $owner.id -or 'computer:read' -notin $claims.scope.Split(' ')){throw 'Effective token claims do not match the protected resource'}
        $checks+=@{client=$clientId;scope=$scopeNames;audience=$claims.aud;status='PASS'}
    }
}
@{at=[DateTime]::UtcNow.ToString('o');checks=$checks;host_acceptance='Actual ChatGPT connection must be retried with a newly issued token'}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $repo 'artifacts/chatgpt-audience-results.json')
$password=$null;$admin=$null;$headers=$null
Write-Output 'PASS exact MCP resource audience for ordinary and offline token claims on both ChatGPT clients; encrypted mapper backup saved.'
