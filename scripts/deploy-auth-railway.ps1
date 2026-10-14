$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
Set-Location $repo
$statePath=Join-Path $repo '.runtime/railway/deployment.json'
$secretPath=Join-Path $repo '.runtime/railway/secrets.dpapi.json'
$state=Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json -AsHashtable
$secrets=Get-Content -LiteralPath $secretPath -Raw | ConvertFrom-Json -AsHashtable
foreach($name in @('OPENCOMMANDER_CLIENT_SECRET','OPENCOMMANDER_OWNER_PASSWORD','KC_BOOTSTRAP_ADMIN_PASSWORD')){
    if(!$secrets[$name]){
        $value=[Convert]::ToBase64String([System.Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).TrimEnd('=').Replace('+','-').Replace('/','_')
        $secrets[$name]=ConvertFrom-SecureString (ConvertTo-SecureString $value -AsPlainText -Force)
    }
}
$secrets | ConvertTo-Json | Set-Content -LiteralPath $secretPath
function ReadSecret([string]$Name){[System.Net.NetworkCredential]::new('',(ConvertTo-SecureString $secrets[$Name])).Password}
$variables=@{
    KC_DB='postgres'; KC_DB_URL='jdbc:postgresql://${{Postgres.RAILWAY_PRIVATE_DOMAIN}}:${{Postgres.PGPORT}}/${{Postgres.PGDATABASE}}?sslmode=require';
    KC_DB_USERNAME='${{Postgres.PGUSER}}'; KC_DB_PASSWORD='${{Postgres.PGPASSWORD}}';
    KC_HOSTNAME=$state.auth_url; KC_HOSTNAME_STRICT='true'; KC_PROXY_HEADERS='xforwarded';
    KC_HTTP_ENABLED='true'; KC_HTTP_HOST='::'; KC_HTTP_PORT='8080'; PORT='8080';
    KC_DB_POOL_INITIAL_SIZE='1'; KC_DB_POOL_MIN_SIZE='1'; KC_DB_POOL_MAX_SIZE='5';
    JAVA_OPTS_KC_HEAP='-Xms128m -Xmx512m'; KC_CACHE='local'; KC_HEALTH_ENABLED='true';
    KC_BOOTSTRAP_ADMIN_USERNAME='opencommander-admin'; KC_BOOTSTRAP_ADMIN_PASSWORD=(ReadSecret 'KC_BOOTSTRAP_ADMIN_PASSWORD');
    OPENCOMMANDER_MCP_RESOURCE=$state.mcp_url;
    OPENCOMMANDER_CLIENT_SECRET=(ReadSecret 'OPENCOMMANDER_CLIENT_SECRET');
    OPENCOMMANDER_OWNER_PASSWORD=(ReadSecret 'OPENCOMMANDER_OWNER_PASSWORD')
}
foreach($name in $variables.Keys){
    $null=$variables[$name] | & railway variable set $name --stdin --skip-deploys --service $state.auth_service_id --project $state.project_id --environment $state.environment_id
    if($LASTEXITCODE -ne 0){throw "Failed to configure auth variable $name"}
}
$config=@{serviceId=$state.auth_service_id;environmentId=$state.environment_id;input=@{
    dockerfilePath='infra/docker/auth.Dockerfile';healthcheckPath='/realms/opencommander/.well-known/openid-configuration';
    healthcheckTimeout=300;numReplicas=1;restartPolicyType='ON_FAILURE';restartPolicyMaxRetries=5;overlapSeconds=0;drainingSeconds=15
}}
$null=($config | ConvertTo-Json -Depth 8 -Compress) | & railway api --file (Join-Path $PSScriptRoot 'railway-service-config.graphql') --variables '@-'
if($LASTEXITCODE -ne 0){throw 'Failed to configure Keycloak service'}
& railway up --service $state.auth_service_id --project $state.project_id --environment $state.environment_id --detach
if($LASTEXITCODE -ne 0){throw 'Failed to upload Keycloak build'}
Write-Output 'Keycloak build queued; verify HTTPS discovery and authorization-code/PKCE before use.'
