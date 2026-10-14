param([switch]$ExpectOfflineScopeFailure)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$state=Get-Content (Join-Path $repo '.runtime/railway/deployment.json') -Raw|ConvertFrom-Json
$verifier=[Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).TrimEnd('=').Replace('+','-').Replace('/','_')
$challenge=[Convert]::ToBase64String([Security.Cryptography.SHA256]::HashData([Text.Encoding]::ASCII.GetBytes($verifier))).TrimEnd('=').Replace('+','-').Replace('/','_')
$query=@{client_id='https://chatgpt.com/oauth/client.json';redirect_uri='https://chatgpt.com/connector_platform_oauth_redirect';response_type='code';scope='openid computer:read computer:control';resource=$state.mcp_url;state='metadata-validation';code_challenge=$challenge;code_challenge_method='S256'}
$qs=($query.GetEnumerator()|ForEach-Object{[Uri]::EscapeDataString($_.Key)+'='+[Uri]::EscapeDataString($_.Value)}) -join '&'
$response=Invoke-WebRequest ($state.auth_url+'/realms/opencommander/protocol/openid-connect/auth?'+$qs) -MaximumRedirection 0 -SkipHttpErrorCheck
if($response.StatusCode -ne 200 -or $response.Content -notmatch 'kc-form-login'){throw 'Exact ChatGPT client did not reach owner login'}
$checks=@(@{name='Exact ChatGPT metadata client reaches owner login';status='PASS'})
foreach($scope in @('openid offline_access computer:read','openid offline_access computer:read computer:control')){
    $query.scope=$scope
    $qs=($query.GetEnumerator()|ForEach-Object{[Uri]::EscapeDataString($_.Key)+'='+[Uri]::EscapeDataString($_.Value)}) -join '&'
    try{$offline=Invoke-WebRequest ($state.auth_url+'/realms/opencommander/protocol/openid-connect/auth?'+$qs) -MaximumRedirection 0 -SkipHttpErrorCheck -ErrorAction SilentlyContinue}catch{throw 'Offline scope authorization probe failed'}
    $location=$offline.Headers.Location -join ''
    if($ExpectOfflineScopeFailure){
        if($location -notmatch 'error=invalid_scope'){throw 'Expected invalid_scope regression was not reproduced'}
        Write-Output ('REPRODUCED invalid_scope: '+$scope)
    }else{
        if($offline.StatusCode -ne 200 -or $offline.Content -notmatch 'kc-form-login'){throw ('ChatGPT scope request rejected: '+$scope)}
        $checks+=@{name=('ChatGPT reaches login with '+$scope);status='PASS'}
    }
}
if($ExpectOfflineScopeFailure){exit 0}
$query.scope='openid offline_access computer:read'
$query.redirect_uri='https://example.invalid/callback'
$qs=($query.GetEnumerator()|ForEach-Object{[Uri]::EscapeDataString($_.Key)+'='+[Uri]::EscapeDataString($_.Value)}) -join '&'
$denied=Invoke-WebRequest ($state.auth_url+'/realms/opencommander/protocol/openid-connect/auth?'+$qs) -MaximumRedirection 0 -SkipHttpErrorCheck
if($denied.StatusCode -ne 400){throw 'Unexpected redirect was not rejected'}
$checks+=@{name='Unallowlisted ChatGPT client callback rejected';status='PASS'}
$query.redirect_uri='https://chatgpt.com/connector_platform_oauth_redirect'
$query.scope='openid offline_access computer:admin'
$qs=($query.GetEnumerator()|ForEach-Object{[Uri]::EscapeDataString($_.Key)+'='+[Uri]::EscapeDataString($_.Value)}) -join '&'
$invalid=Invoke-WebRequest ($state.auth_url+'/realms/opencommander/protocol/openid-connect/auth?'+$qs) -MaximumRedirection 0 -SkipHttpErrorCheck -ErrorAction SilentlyContinue
if(($invalid.Headers.Location -join '') -notmatch 'error=invalid_scope'){throw 'Unknown scope was not rejected'}
$checks+=@{name='Unregistered computer:admin scope rejected';status='PASS'}
$query.scope='openid offline_access computer:read'
$query.Remove('code_challenge');$query.Remove('code_challenge_method')
$qs=($query.GetEnumerator()|ForEach-Object{[Uri]::EscapeDataString($_.Key)+'='+[Uri]::EscapeDataString($_.Value)}) -join '&'
$missingPkce=Invoke-WebRequest ($state.auth_url+'/realms/opencommander/protocol/openid-connect/auth?'+$qs) -MaximumRedirection 0 -SkipHttpErrorCheck -ErrorAction SilentlyContinue
if(($missingPkce.Headers.Location -join '') -notmatch 'error=invalid_request'){throw 'Authorization without PKCE was not rejected'}
$checks+=@{name='Offline scope authorization without PKCE rejected';status='PASS'}
$report=@{at=[DateTime]::UtcNow.ToString('o');client_id='https://chatgpt.com/oauth/client.json';checks=$checks;host_connection='Not yet verified in ChatGPT UI'}
$report|ConvertTo-Json -Depth 6|Set-Content (Join-Path $repo 'artifacts/chatgpt-client-results.json')
Write-Output 'PASS ChatGPT client login, offline scopes, strict callback and unknown scope rejection; host connection still requires user login.'
