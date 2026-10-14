import {Client,StreamableHTTPClientTransport} from '@modelcontextprotocol/client';
import {createRemoteJWKSet,jwtVerify} from 'jose';
import {randomBytes,createHash,randomUUID} from 'node:crypto';
import {spawn,execFileSync,type ChildProcess} from 'node:child_process';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import {actions,compatibilityTools} from '../src/tools.js';

const repo=resolve(fileURLToPath(new URL('../../..',import.meta.url)));
const deployment=JSON.parse(await readFile(join(repo,'.runtime','railway','deployment.json'),'utf8')) as Record<string,string>;
function setting(name:string){const value=deployment[name];assert(value,`Missing deployment setting ${name}`);return value}
let input='';for await(const chunk of process.stdin)input+=String(chunk);
const credentials=JSON.parse(input) as {owner_password:string;client_secret:string;pairing_code?:string};input='';
const issuer=setting('auth_url')+'/realms/opencommander';const resource=setting('mcp_url');const gateway=setting('gateway_url');const callback='http://127.0.0.1:18123/callback';
const dir=join(repo,'.runtime',`railway-e2e-${Date.now()}`);const root=join(dir,'files');const agentState=join(dir,'agent');await mkdir(root,{recursive:true});
const checks:{name:string;status:string;detail?:unknown}[]=[];
const pass=(name:string,detail?:unknown)=>{checks.push({name,status:'PASS',detail});console.log(`PASS ${name}`)};
const delay=(ms:number)=>new Promise(r=>setTimeout(r,ms));
const terminalStates=['succeeded','failed','cancelled','expired','uncertain'];
async function until<T>(fn:()=>Promise<T|undefined>,timeout=60000):Promise<T>{const start=Date.now();while(Date.now()-start<timeout){const result=await fn();if(result!==undefined)return result;await delay(500)}throw new Error('Timed out waiting for expected state')}
const cookies=new Map<string,string>();
async function session(url:string,options:RequestInit={}){
 url=new URL(url,issuer).href;
 assert.equal(new URL(url).origin,new URL(issuer).origin,'Authentication stays on the configured issuer');
 const headers=new Headers(options.headers);if(cookies.size)headers.set('Cookie',[...cookies].map(([k,v])=>`${k}=${v}`).join('; '));
 const response=await fetch(url,{...options,headers,redirect:'manual'});
 for(const cookie of response.headers.getSetCookie()){const pair=cookie.split(';')[0]!;const split=pair.indexOf('=');cookies.set(pair.slice(0,split),pair.slice(split+1))}
 return response;
}
const attr=(tag:string,name:string)=>new RegExp(`\\b${name}=["']([^"']*)["']`,'i').exec(tag)?.[1]?.replaceAll('&amp;','&');
function form(html:string){const tag=/<form\b[^>]*>/i.exec(html)?.[0];assert(tag,'Expected Keycloak authentication form');const action=attr(tag,'action');assert(action);const values:Record<string,string>={};for(const inputTag of html.matchAll(/<input\b[^>]*>/gi)){const name=attr(inputTag[0],'name');if(name&&attr(inputTag[0],'type')==='hidden')values[name]=attr(inputTag[0],'value')??''}return {action,values}}
interface Tokens {access_token:string;refresh_token:string}
let tokens:Tokens|undefined;let client:Client|undefined;let agent:ChildProcess|undefined;let device='';let notepadPid:number|undefined;
const notepad=join(process.env.WINDIR??'C:\\Windows','System32','notepad.exe');let agentOutput='';
async function connect(accessToken:string){const c=new Client({name:'opencommander-railway-verification',version:'0.1.0'},{versionNegotiation:{mode:{pin:'2026-07-28'}}});await c.connect(new StreamableHTTPClientTransport(new URL(resource),{requestInit:{headers:{Authorization:`Bearer ${accessToken}`}}}));return c}
async function tool(name:string,args:Record<string,unknown>){const result=await client!.callTool({name,arguments:args});assert(!result.isError,`${name} rejected`);return result.structuredContent as Record<string,unknown>}
async function operate(name:string,args:Record<string,unknown>,consent=false,key=randomUUID()){
 const submitted=await tool(name,{device_id:device,idempotency_key:key,timeout_seconds:60,...args});const id=String(submitted.task_id);
 if(consent){const ready=await until(async()=>{const status=String((await tool('approval_status',{task_id:id})).status);return status==='approval_pending'?'pending':terminalStates.includes(status)?'terminal':undefined});
  if(ready==='pending')await until(async()=>{try{execFileSync('powershell.exe',['-NoProfile','-File',join(repo,'scripts','consent-test.ps1'),'-TaskId',id,'-AgentPid',String(agent!.pid),'-Decision','yes'],{windowsHide:true,stdio:'pipe',timeout:10000});return true}catch{const status=String((await tool('task_status',{task_id:id})).status);return terminalStates.includes(status)?true:undefined}});
 }
 return until(async()=>{const task=await tool('task_status',{task_id:id});return terminalStates.includes(String(task.status))?task:undefined});
}
async function stopAgent(){if(agent&&agent.exitCode===null){agent.kill();await new Promise<void>(r=>{agent!.once('exit',()=>r());setTimeout(r,5000).unref()})}}
try{verification:{
 assert.equal((await fetch(gateway+'/healthz')).status,200);assert.equal((await fetch(new URL('/healthz',resource))).status,200);pass('Railway gateway and MCP HTTPS health checks');
 assert.equal((await fetch(gateway+'/api/devices')).status,404);pass('Public gateway refuses internal API');
 const unauth=await fetch(resource,{method:'POST',headers:{'Content-Type':'application/json'},body:'{}'});assert.equal(unauth.status,401);assert.match(unauth.headers.get('www-authenticate')??'',/resource_metadata/);pass('Public MCP requires OAuth and advertises resource metadata');
 const discovery=await fetch(issuer+'/.well-known/openid-configuration').then(r=>r.json()) as Record<string,unknown>;
 assert.equal(discovery.issuer,issuer);assert((discovery.code_challenge_methods_supported as string[]).includes('S256'));assert.equal(new URL(String(discovery.token_endpoint)).protocol,'https:');pass('Production Keycloak discovery and PKCE S256');
 const verifier=randomBytes(32).toString('base64url');const state=randomUUID();const auth=new URL(String(discovery.authorization_endpoint));
 const params={client_id:'opencommander-chatgpt',redirect_uri:callback,response_type:'code',scope:'openid computer:read computer:control',resource,state,code_challenge:createHash('sha256').update(verifier).digest('base64url'),code_challenge_method:'S256'};
 for(const [k,v]of Object.entries(params))auth.searchParams.set(k,v);
 const noPkce=new URL(auth);noPkce.searchParams.delete('code_challenge');noPkce.searchParams.delete('code_challenge_method');const missing=await session(noPkce.href);assert(missing.status>=400||/invalid_request/.test(missing.headers.get('location')??await missing.text()));pass('Authorization rejects missing PKCE');
 let response=await session(auth.href);let html=await response.text();const login=form(html);login.values.username='owner';login.values.password=credentials.owner_password;login.values.credentialId='';
 response=await session(login.action,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams(login.values)});
 let location=response.headers.get('location');
 for(let i=0;i<6&&!(location?.startsWith(callback));i++){
  if(location){response=await session(new URL(location,issuer).href);location=response.headers.get('location');if(location?.startsWith(callback))break}
  html=await response.text();
  if(!/name=["']accept["']/.test(html))throw new Error('Unexpected authentication step: '+html.replace(/<script[\s\S]*?<\/script>/gi,'').replace(/<style[\s\S]*?<\/style>/gi,'').replace(/<[^>]+>/g,' ').replace(/\s+/g,' ').slice(0,450));
  const consent=form(html);consent.values.accept='Yes';
  response=await session(consent.action,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams(consent.values)});location=response.headers.get('location');
 }
 assert(location);assert(location.startsWith(callback),'Login must redirect to the exact allowlisted callback');const redirected=new URL(location);assert.equal(redirected.searchParams.get('state'),state);assert.equal(redirected.searchParams.get('iss'),issuer);const code=redirected.searchParams.get('code');assert(code);
 const tokenEndpoint=String(discovery.token_endpoint);const tokenResponse=await fetch(tokenEndpoint,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({grant_type:'authorization_code',client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,redirect_uri:callback,resource,code,code_verifier:verifier})});assert.equal(tokenResponse.status,200);tokens=await tokenResponse.json() as Tokens;
 const verified=await jwtVerify(tokens.access_token,createRemoteJWKSet(new URL(String(discovery.jwks_uri))),{issuer,audience:resource,algorithms:['RS256'],requiredClaims:['sub','iat','exp','jti']});assert(String(verified.payload.scope).includes('computer:control'));pass('Real authorization-code login with PKCE, issuer and resource-bound signed access token');
 const refreshed=await fetch(tokenEndpoint,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({grant_type:'refresh_token',client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,resource,refresh_token:tokens.refresh_token})});assert.equal(refreshed.status,200);const next=await refreshed.json() as Tokens;assert.notEqual(next.refresh_token,tokens.refresh_token);tokens=next;pass('Real refresh flow rotates refresh tokens');
 client=await connect(tokens.access_token);const catalog=await client.listTools();assert.equal(catalog.tools.length,Object.keys(actions).length+compatibilityTools.length+10);for(const name of ['device_list','device_status','system_info','file_list','file_read','file_write','file_manage','file_search','terminal_execute','process_list','process_manage','application_launch','desktop_screenshot','desktop_windows','desktop_interact','task_status','task_cancel'])assert(catalog.tools.some(t=>t.name===name));assert.equal(client.getProtocolEra(),'modern');pass('Authenticated public MCP discovers every required tool and modern typed operations',{tools:catalog.tools.length});
 if(credentials.pairing_code){
  const paired=String((await tool('device_pair',{code:credentials.pairing_code})).device_id);
  await until(async()=>((await tool('device_status',{device_id:paired})).connected?true:undefined));
  const submitted=await tool('system_info',{device_id:paired,idempotency_key:randomUUID(),timeout_seconds:30});
  const completed=await until(async()=>{const result=await tool('task_status',{task_id:submitted.task_id});return terminalStates.includes(String(result.status))?result:undefined});assert.equal(completed.status,'succeeded');assert.equal((completed.result as Record<string,unknown>).os,'Windows');
  await writeFile(join(repo,'artifacts','installed-agent-results.json'),JSON.stringify({at:new Date().toISOString(),device_id:paired,gateway,connected:true,system_info:'PASS',host_chatgpt_tool_session:'Not directly verified'},null,2));
  const revoke=String(discovery.revocation_endpoint);await fetch(revoke,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,token:tokens.refresh_token,token_type_hint:'refresh_token'})});
  pass('Installed Windows agent paired and system_info verified through Railway');break verification;
 }
 agent=spawn(join(repo,'artifacts','opencommander-agent.exe'),['--init','--gateway',gateway,'--root',root,'--state',agentState,'--application',notepad,'--allow-modify','--allow-screenshots'],{windowsHide:true,stdio:['ignore','pipe','pipe']});agent.stdout!.on('data',b=>{agentOutput+=String(b)});agent.stderr!.on('data',()=>{});
 const pairingCode=await until(async()=>{for(const line of agentOutput.split(/\r?\n/)){try{const parsed=JSON.parse(line) as Record<string,unknown>;if(parsed.event==='pairing_required')return String(parsed.code)}catch{}}return undefined});agentOutput='';
 device=String((await tool('device_pair',{code:pairingCode})).device_id);await until(async()=>((await tool('device_status',{device_id:device})).connected?true:undefined));pass('Real Windows Rust agent enrolled and connected over Railway WSS');
 const info=await operate('system_info',{});assert.equal(info.status,'succeeded');assert.equal((info.result as Record<string,unknown>).os,'Windows');pass('Public MCP through Railway to real Windows system_info');
 const path=join(root,'railway-fixture.txt');const written=await operate('file_write',{path,text:'Verified through Railway HTTPS and WSS'});assert.equal(written.status,'succeeded');const read=await operate('file_read',{path});assert.equal((read.result as Record<string,unknown>).text,'Verified through Railway HTTPS and WSS');pass('Public MCP file write and read under local Windows policy');
 const terminal=await operate('terminal_execute',{command:"Write-Output 'RAILWAY-WINDOWS-VERIFIED'",cwd:root,env:{}},true);assert.equal(terminal.status,'succeeded');assert.match(String((terminal.result as Record<string,unknown>).stdout),/RAILWAY-WINDOWS-VERIFIED/);pass('Public MCP PowerShell execution with native Windows consent');
 const launched=await operate('application_launch',{path:notepad,args:[],cwd:root},true);assert.equal(launched.status,'succeeded');notepadPid=Number((launched.result as Record<string,unknown>).pid);
 const window=await until(async()=>{const windows=await operate('desktop_windows',{});return ((windows.result as Record<string,unknown>).windows as Record<string,unknown>[]).find(w=>w.pid===notepadPid)});
 assert.equal((await operate('desktop_interact',{op:'focus',hwnd:window.hwnd},true)).status,'succeeded');assert.equal((await operate('desktop_interact',{op:'type',hwnd:window.hwnd,text:'OpenCommander Railway GUI verified'},true)).status,'succeeded');
 const text=execFileSync('powershell.exe',['-NoProfile','-File',join(repo,'scripts','verify-notepad.ps1'),'-ProcessId',String(notepadPid)],{windowsHide:true,encoding:'utf8'});assert.match(text,/OpenCommander Railway GUI verified/);
 const capture=await operate('desktop_screenshot',{});const captured=await client.callTool({name:'task_status',arguments:{task_id:capture.task_id}});const picture=(captured.content as {type:string;data?:string}[]).find(c=>c.type==='image');assert(picture?.data);await writeFile(join(dir,'desktop.png'),Buffer.from(picture.data,'base64'));pass('Public MCP launches, focuses and types in real Notepad; authenticated screenshot returned');
 await operate('process_manage',{op:'terminate',pid:notepadPid},true);notepadPid=undefined;
 const completedId=String(written.task_id);
 if(process.argv.includes('--restart')){
  assert(process.env.RAILWAY_CLI_EXE,'Set RAILWAY_CLI_EXE to the installed native Railway CLI');
  const deployments=JSON.parse(execFileSync(process.env.RAILWAY_CLI_EXE,['deployment','list','--service','gateway','--limit','1','--json'],{windowsHide:true,encoding:'utf8',timeout:20000})) as {id:string}[];
  const deploymentId=deployments[0]?.id;assert(deploymentId&&/^[a-f0-9-]{36}$/.test(deploymentId));
  execFileSync(process.env.RAILWAY_CLI_EXE,['api',`mutation { deploymentRestart(id: "${deploymentId}") }`],{windowsHide:true,stdio:'pipe',timeout:20000});
  await delay(8000);await until(async()=>{try{return (await tool('device_status',{device_id:device})).connected?true:undefined}catch{return undefined}},90000);
  assert.equal((await tool('task_status',{task_id:completedId})).status,'succeeded');pass('Railway gateway restart reconnects WSS and preserves completed task in volume');
 }
 await tool('device_revoke',{device_id:device});device='';pass('Remote fixture device revoked after verification');
 const wrongResource=await fetch(tokenEndpoint,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({grant_type:'refresh_token',client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,resource:'https://example.invalid/mcp',refresh_token:tokens.refresh_token})});assert.equal(wrongResource.status,400);assert.equal((await wrongResource.json() as {error:string}).error,'invalid_target');pass('Token endpoint rejects an unrecognized resource indicator');
 const revoked=await fetch(String(discovery.revocation_endpoint),{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,token:tokens.refresh_token,token_type_hint:'refresh_token'})});assert.equal(revoked.status,200);
 const blocked=await fetch(tokenEndpoint,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({grant_type:'refresh_token',client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,resource,refresh_token:tokens.refresh_token})});assert.equal(blocked.status,400);pass('OAuth refresh token revocation prevents refresh');
 const replay=await fetch(tokenEndpoint,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({grant_type:'authorization_code',client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,redirect_uri:callback,resource,code,code_verifier:verifier})});assert.equal(replay.status,400);pass('Authorization code cannot be replayed');
}}catch(error){checks.push({name:'Railway end-to-end',status:'FAIL',detail:String(error)});console.error(String(error));process.exitCode=1}
finally{
 if(device&&client)await tool('device_revoke',{device_id:device}).catch(()=>{});
 if(client)await client.close().catch(()=>{});await stopAgent();
 if(notepadPid)try{execFileSync('powershell.exe',['-NoProfile','-Command',`Stop-Process -Id ${notepadPid} -ErrorAction SilentlyContinue`],{windowsHide:true,stdio:'ignore'})}catch{}
 await writeFile(join(repo,'artifacts',credentials.pairing_code?'installed-agent-verification.json':'railway-results.json'),JSON.stringify({at:new Date().toISOString(),project_id:deployment.project_id,urls:{gateway:deployment.gateway_url,mcp:resource,issuer},checks,evidence_directory:dir},null,2));
 console.log(`Evidence: ${dir}`);
}
