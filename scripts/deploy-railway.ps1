param(
    [string]$ProjectId,
    [string]$Workspace,
    [string]$OidcIssuer,
    [string]$OidcJwksUrl
)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
Set-Location $repo
$stateDir=Join-Path $repo '.runtime/railway'
New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
$statePath=Join-Path $stateDir 'deployment.json'
$secretPath=Join-Path $stateDir 'secrets.dpapi.json'
function Railway([string[]]$Arguments) {
    $lines=& railway @Arguments
    if($LASTEXITCODE -ne 0){throw "Railway command failed: $($Arguments[0])"}
    return ($lines -join "`n")
}
function ConfigureService([string]$ServiceId,[string]$Dockerfile,[string]$Health='/healthz') {
    $variables=@{serviceId=$ServiceId;environmentId=$state.environment_id;input=@{dockerfilePath=$Dockerfile;healthcheckPath=$Health;healthcheckTimeout=300;numReplicas=1;restartPolicyType='ON_FAILURE';restartPolicyMaxRetries=5;overlapSeconds=0;drainingSeconds=5}}
    $null=($variables | ConvertTo-Json -Depth 8 -Compress) | & railway api --file (Join-Path $PSScriptRoot 'railway-service-config.graphql') --variables '@-'
    if($LASTEXITCODE -ne 0){throw "Failed to configure service $ServiceId"}
}
function SetVariables([string]$Service,[hashtable]$Values) {
    foreach($key in $Values.Keys){
        # Values go through stdin; never print secrets or include them in process arguments.
        $null=$Values[$key] | & railway variable set $key --stdin --skip-deploys --service $Service --project $ProjectId --environment production
        if($LASTEXITCODE -ne 0){throw "Failed to configure $Service variable $key"}
    }
}
$null=Railway @('whoami')
if(Test-Path -LiteralPath $statePath){
    $state=Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json -AsHashtable
    if($ProjectId -and $ProjectId -ne $state.project_id){throw 'Project differs from saved deployment; use a separate checkout/state directory.'}
    $ProjectId=$state.project_id
}else{
    if($ProjectId){throw 'For initial deployment omit ProjectId; this script creates a separate OpenCommander project.'}
    $argsList=@('init','--name','OpenCommander','--json')
    if($Workspace){$argsList+=@('--workspace',$Workspace)}
    $created=Railway $argsList | ConvertFrom-Json -AsHashtable
    $ProjectId=$created.id
    if(!$ProjectId){$ProjectId=$created.project.id}
    if(!$ProjectId){throw 'Project creation returned no id; inspect railway status before retrying.'}
    $state=@{project_id=$ProjectId}
    $state | ConvertTo-Json | Set-Content -LiteralPath $statePath
}
if(!(Test-Path -LiteralPath $secretPath)){
    $encrypted=@{}
    foreach($name in @('BRIDGE_KEY','GATEWAY_SIGNING_SEED')){
        $length=if($name -eq 'BRIDGE_KEY'){48}else{32}
        $value=[Convert]::ToBase64String([System.Security.Cryptography.RandomNumberGenerator]::GetBytes($length))
        $encrypted[$name]=ConvertFrom-SecureString (ConvertTo-SecureString $value -AsPlainText -Force)
    }
    $encrypted | ConvertTo-Json | Set-Content -LiteralPath $secretPath
}
$secrets=Get-Content -LiteralPath $secretPath -Raw | ConvertFrom-Json -AsHashtable
function Secret([string]$Name){[System.Net.NetworkCredential]::new('',(ConvertTo-SecureString $secrets[$Name])).Password}
# Link explicitly to the saved project so an unrelated linked Railway project is never changed.
$null=Railway @('link','--project',$ProjectId,'--environment','production')
if(!$state.gateway_created){
    $null=Railway @('add','--service','gateway','--json')
    $state.gateway_created=$true
    $state | ConvertTo-Json | Set-Content -LiteralPath $statePath
}
$live=InvokeRailwayCli -Arguments @('status','--json') | ConvertFrom-Json -AsHashtable
$state.environment_id=($live.environments.edges | Where-Object {$_.node.name -eq 'production'}).node.id
$state.gateway_service_id=($live.services.edges | Where-Object {$_.node.name -eq 'gateway'}).node.id
ConfigureService $state.gateway_service_id 'infra/docker/gateway.Dockerfile'
SetVariables 'gateway' @{
    GATEWAY_ADDR='[::]:8080'; GATEWAY_PUBLIC_ADDR='[::]:8090'; PORT='8090';
    GATEWAY_DB='/data/gateway.db'; RAILWAY_RUN_UID='0';
    BRIDGE_KEY=(Secret 'BRIDGE_KEY'); GATEWAY_SIGNING_SEED=(Secret 'GATEWAY_SIGNING_SEED')
}
if(!$state.volume_created){
    $null=Railway @('volume','--project',$ProjectId,'--service','gateway','--environment','production','add','--mount-path','/data','--json')
    $state.volume_created=$true
    $state | ConvertTo-Json | Set-Content -LiteralPath $statePath
}
if(!$state.gateway_url){
    $domain=Railway @('domain','--service','gateway','--project',$ProjectId,'--environment','production','--port','8090','--json') | ConvertFrom-Json -AsHashtable
    $hostName=$domain.domain
    if(!$hostName -and $domain.domains){$hostName=$domain.domains[0].domain}
    if(!$hostName){throw 'Domain returned no hostname; inspect railway domain list before retrying.'}
    $state.gateway_url=if($hostName.StartsWith('https://')){$hostName}else{"https://$hostName"}
    $state | ConvertTo-Json | Set-Content -LiteralPath $statePath
}
$null=Railway @('up','--service','gateway','--project',$ProjectId,'--environment','production','--detach')
if($OidcIssuer -and $OidcJwksUrl){
    if(([uri]$OidcIssuer).Scheme -ne 'https' -or ([uri]$OidcJwksUrl).Scheme -ne 'https'){throw 'Production OAuth must use HTTPS.'}
    if(!$state.mcp_created){
        $null=Railway @('add','--service','mcp','--json')
        $state.mcp_created=$true
        $state | ConvertTo-Json | Set-Content -LiteralPath $statePath
    }
    if(!$state.mcp_url){
        $domain=Railway @('domain','--service','mcp','--project',$ProjectId,'--environment','production','--port','3000','--json') | ConvertFrom-Json -AsHashtable
        $hostName=$domain.domain
        if(!$hostName -and $domain.domains){$hostName=$domain.domains[0].domain}
        if(!$hostName){throw 'Domain returned no hostname; inspect railway domain list before retrying.'}
        $state.mcp_url=if($hostName.StartsWith('https://')){"$hostName/mcp"}else{"https://$hostName/mcp"}
    }
    $live=InvokeRailwayCli -Arguments @('status','--json') | ConvertFrom-Json -AsHashtable
    $state.mcp_service_id=($live.services.edges | Where-Object {$_.node.name -eq 'mcp'}).node.id
    ConfigureService $state.mcp_service_id 'infra/docker/mcp.Dockerfile'
    SetVariables 'mcp' @{
        MCP_HOST='::'; MCP_PORT='3000'; PORT='3000';
        MCP_RESOURCE=$state.mcp_url; OIDC_ISSUER=$OidcIssuer; OIDC_JWKS_URL=$OidcJwksUrl;
        GATEWAY_URL='http://gateway.railway.internal:8080'; BRIDGE_KEY=(Secret 'BRIDGE_KEY')
    }
    $state | ConvertTo-Json | Set-Content -LiteralPath $statePath
    $null=Railway @('up','--service','mcp','--project',$ProjectId,'--environment','production','--detach')
}
$state | ConvertTo-Json | Set-Content -LiteralPath $statePath
Write-Output "Deployment queued. Project: $ProjectId"
Write-Output "Gateway: $($state.gateway_url)"
if($state.mcp_url){Write-Output "MCP: $($state.mcp_url)"}else{Write-Output 'MCP awaits production OIDC issuer and JWKS configuration.'}
Write-Output 'Queued is not healthy. Verify deployment status and HTTPS endpoints before pairing.'
