$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$state=Get-Content (Join-Path $repo '.runtime/railway/deployment.json') -Raw | ConvertFrom-Json
$secrets=Get-Content (Join-Path $repo '.runtime/railway/secrets.dpapi.json') -Raw | ConvertFrom-Json -AsHashtable
$password=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $secrets.KC_BOOTSTRAP_ADMIN_PASSWORD)).Password
$token=Invoke-RestMethod ($state.auth_url+'/realms/master/protocol/openid-connect/token') -Method Post -Body @{grant_type='password';client_id='admin-cli';username='opencommander-admin';password=$password}
$headers=@{Authorization='Bearer '+$token.access_token}
$base=$state.auth_url+'/admin/realms/opencommander'
function PutJson($Path,$Value){$null=Invoke-RestMethod ($base+$Path) -Headers $headers -Method Put -ContentType 'application/json' -Body ($Value|ConvertTo-Json -Depth 30)}
$scopes=Invoke-RestMethod ($base+'/client-scopes') -Headers $headers
if(!($scopes|Where-Object name -eq 'basic')){
    $basic=@{name='basic';protocol='openid-connect';attributes=@{'include.in.token.scope'='false';'display.on.consent.screen'='false'};protocolMappers=@(
        @{name='Stable owner subject';protocol='openid-connect';protocolMapper='oidc-sub-mapper';consentRequired=$false;config=@{'id.token.claim'='true';'access.token.claim'='true';'introspection.token.claim'='true'}},
        @{name='MCP resource audience';protocol='openid-connect';protocolMapper='oidc-audience-mapper';consentRequired=$false;config=@{'included.custom.audience'=$state.mcp_url;'id.token.claim'='false';'access.token.claim'='true'}}
    )}
    $null=Invoke-RestMethod ($base+'/client-scopes') -Headers $headers -Method Post -ContentType 'application/json' -Body ($basic|ConvertTo-Json -Depth 12)
}
PutJson '' @{defaultDefaultClientScopes=@('basic','computer:read');defaultOptionalClientScopes=@('computer:control')}
$name='opencommander-chatgpt-cimd'
$profiles=Invoke-RestMethod ($base+'/client-policies/profiles') -Headers $headers
$profiles.profiles=@(@($profiles.profiles|Where-Object name -ne $name)+@{
    name=$name;description='Allow only verified ChatGPT confidential metadata clients';executors=@(@{
        executor='client-id-metadata-document';configuration=@{
            'cimd-allow-http-scheme'=$false;'cimd-allow-permitted-domains'=@('chatgpt.com','persistent.oaistatic.com');
            'cimd-required-properties'=@('client_id','redirect_uris','token_endpoint_auth_method','jwks_uri');
            'cimd-restrict-same-domain'=$false;'cimd-resource-indicator-allow-list'=@($state.mcp_url);
            'only-allow-confidential-client'=$true;'accept-public-client-with-confidential-client-only-grant'=$false
        }
    })
})
PutJson '/client-policies/profiles' $profiles
$policies=Invoke-RestMethod ($base+'/client-policies/policies') -Headers $headers
$policies.policies=@(@($policies.policies|Where-Object name -ne $name)+@{
    name=$name;enabled=$false;description='Reserved until Keycloak accepts current ChatGPT metadata fields';profiles=@($name);
    conditions=@(@{condition='client-id-uri';configuration=@{'client-id-uri-scheme'=@('https');'client-id-uri-allow-permitted-domains'=@('chatgpt.com')}})
})
PutJson '/client-policies/policies' $policies
# Keycloak 26.8 CIMD rejects an extra field currently present in ChatGPT metadata.
# Register the exact verified metadata client explicitly; authenticate with ChatGPT JWKS.
$metadata=Invoke-RestMethod 'https://chatgpt.com/oauth/client.json'
if($metadata.client_id -ne 'https://chatgpt.com/oauth/client.json' -or $metadata.jwks_uri -ne 'https://chatgpt.com/oauth/jwks.json' -or $metadata.token_endpoint_auth_method -ne 'private_key_jwt' -or @($metadata.redirect_uris).Count -ne 1 -or $metadata.redirect_uris[0] -ne 'https://chatgpt.com/connector_platform_oauth_redirect'){throw 'ChatGPT metadata changed; review exact client settings'}
$client=@{clientId=$metadata.client_id;name='ChatGPT OpenCommander';enabled=$true;protocol='openid-connect';publicClient=$false;clientAuthenticatorType='client-jwt';standardFlowEnabled=$true;implicitFlowEnabled=$false;directAccessGrantsEnabled=$false;serviceAccountsEnabled=$false;consentRequired=$true;fullScopeAllowed=$false;redirectUris=@($metadata.redirect_uris);webOrigins=@('https://chatgpt.com');defaultClientScopes=@('basic','computer:read');optionalClientScopes=@('computer:control','offline_access');attributes=@{'pkce.code.challenge.method'='S256';'use.jwks.url'='true';'jwks.url'=$metadata.jwks_uri;'use.refresh.tokens'='true'}}
$existing=Invoke-RestMethod ($base+'/clients?clientId='+[Uri]::EscapeDataString($metadata.client_id)) -Headers $headers
if($existing.Count){PutJson ('/clients/'+$existing[0].id) $client}else{$null=Invoke-RestMethod ($base+'/clients') -Headers $headers -Method Post -ContentType 'application/json' -Body ($client|ConvertTo-Json -Depth 12)}
& (Join-Path $PSScriptRoot 'enable-chatgpt-offline-scope.ps1')
& (Join-Path $PSScriptRoot 'fix-mcp-token-audience.ps1')
Write-Output 'Exact ChatGPT metadata client registered with private_key_jwt, PKCE and consent; no secrets printed.'
