import {randomBytes,createHash,randomUUID} from 'node:crypto';
import {readFile,writeFile,unlink,rmdir,mkdtemp,realpath,rm} from 'node:fs/promises';
import {join,resolve,sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {inflateSync} from 'node:zlib';
import {createRemoteJWKSet,jwtVerify} from 'jose';
import {actions,compatibilityTools} from '../src/tools.js';

const repo=fileURLToPath(new URL('../../..',import.meta.url));
const config=JSON.parse(await readFile(repo+'/.runtime/railway/deployment.json','utf8')) as Record<string,string>;
const resource=config.mcp_url!;const issuer=config.auth_url!+'/realms/opencommander';
let input='';for await(const part of process.stdin)input+=String(part);
const credentials=JSON.parse(input||'{}') as {owner_password?:string;client_secret?:string;verify_profile_access?:boolean;verify_unattended?:boolean;verify_desktop?:boolean;verify_all_applications?:boolean};input='';
const controlRequested=credentials.verify_profile_access===true||credentials.verify_desktop===true||credentials.verify_all_applications===true;
const fixtures:{folder:string;file:string}[]=[];
const report:{at:string;client:string;host_chatgpt:string;checks:Record<string,unknown>[];system_info?:unknown;device_id?:string;task_id?:string}={at:new Date().toISOString(),client:'independent raw HTTP JSON-RPC, not ChatGPT host',host_chatgpt:'NOT VERIFIED',checks:[]};
const pass=(name:string,detail?:unknown)=>{report.checks.push({name,status:'PASS',detail});console.log('PASS '+name)};
let tokens:{access_token:string;refresh_token:string}|undefined;let tokenEndpoint='';let revokeEndpoint='';let stage='public endpoint checks';
try{
 assert.equal((await fetch(config.gateway_url!+'/healthz')).status,200);assert.equal((await fetch(new URL('/healthz',resource))).status,200);pass('Gateway and MCP health');
 const rejected=await fetch(resource,{method:'POST',headers:{'Content-Type':'application/json'},body:'{}'});assert.equal(rejected.status,401);assert.match(rejected.headers.get('www-authenticate')??'',/resource_metadata/);pass('Anonymous MCP denied with OAuth challenge');
 const metadata=await fetch(new URL('/.well-known/oauth-protected-resource',resource)).then(r=>r.json()) as Record<string,unknown>;
 assert.equal(metadata.resource,resource);assert.deepEqual(metadata.authorization_servers,[issuer]);pass('Exact protected resource and issuer metadata');
 const discovery=await fetch(issuer+'/.well-known/openid-configuration').then(r=>r.json()) as Record<string,unknown>;
 assert.equal(discovery.issuer,issuer);assert((discovery.code_challenge_methods_supported as string[]).includes('S256'));tokenEndpoint=String(discovery.token_endpoint);revokeEndpoint=String(discovery.revocation_endpoint);pass('HTTPS OAuth discovery, JWKS and S256');
 if(!credentials.owner_password||!credentials.client_secret){report.checks.push({name:'Authenticated discovery and Windows execution',status:'BLOCKED',detail:'Protected diagnostic credentials unavailable'});}else{
 stage='protected fixture authorization-code PKCE login';
 const callback='http://127.0.0.1:18123/callback';const verifier=randomBytes(32).toString('base64url');const state=randomUUID();const auth=new URL(String(discovery.authorization_endpoint));
 const requestedScope='openid offline_access computer:read'+(controlRequested?' computer:control':'');
 for(const [k,v]of Object.entries({client_id:'opencommander-chatgpt',redirect_uri:callback,response_type:'code',scope:requestedScope,state,code_challenge:createHash('sha256').update(verifier).digest('base64url'),code_challenge_method:'S256'}))auth.searchParams.set(k,v);
 const cookies=new Map<string,string>();
 async function session(url:string,options:RequestInit={}){url=new URL(url,issuer).href;assert.equal(new URL(url).origin,new URL(issuer).origin);const headers=new Headers(options.headers);if(cookies.size)headers.set('Cookie',[...cookies].map(([k,v])=>k+'='+v).join('; '));const r=await fetch(url,{...options,headers,redirect:'manual'});for(const c of r.headers.getSetCookie()){const p=c.split(';')[0]!;const n=p.indexOf('=');cookies.set(p.slice(0,n),p.slice(n+1))}return r;}
 const attr=(tag:string,name:string)=>new RegExp(`\\b${name}=["']([^"']*)["']`,'i').exec(tag)?.[1]?.replaceAll('&amp;','&');
 function form(html:string){const tag=/<form\b[^>]*>/i.exec(html)?.[0];assert(tag,'Expected authentication form');const action=attr(tag,'action');assert(action);const values:Record<string,string>={};for(const t of html.matchAll(/<input\b[^>]*>/gi)){const name=attr(t[0],'name');if(name&&attr(t[0],'type')==='hidden')values[name]=attr(t[0],'value')??''}return {action,values};}
 let response=await session(auth.href);const login=form(await response.text());login.values.username='owner';login.values.password=credentials.owner_password;login.values.credentialId='';
 response=await session(login.action,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams(login.values)});let location=response.headers.get('location');
 for(let n=0;n<6&&!location?.startsWith(callback);n++){if(location){response=await session(location);location=response.headers.get('location');if(location?.startsWith(callback))break}const html=await response.text();assert(/name=["']accept["']/.test(html),'Unexpected authentication step');const consent=form(html);consent.values.accept='Yes';response=await session(consent.action,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams(consent.values)});location=response.headers.get('location');}
 assert(location);assert(location.startsWith(callback));const redirected=new URL(location);assert.equal(redirected.searchParams.get('state'),state);assert.equal(redirected.searchParams.get('iss'),issuer);const code=redirected.searchParams.get('code');assert(code);
 const tokenResponse=await fetch(tokenEndpoint,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({grant_type:'authorization_code',client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,redirect_uri:callback,code,code_verifier:verifier})});assert.equal(tokenResponse.status,200);tokens=await tokenResponse.json() as typeof tokens;
 assert(tokens);const verified=await jwtVerify(tokens.access_token,createRemoteJWKSet(new URL(String(discovery.jwks_uri))),{issuer,audience:resource,algorithms:['RS256'],requiredClaims:['sub','iat','exp','jti']});assert.equal(String(verified.payload.scope).split(' ').includes('computer:control'),controlRequested);pass('Fixture token has requested scope, signature, subject, issuer, audience and PKCE');
 const refreshResponse=await fetch(tokenEndpoint,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({grant_type:'refresh_token',client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,refresh_token:tokens.refresh_token})});assert.equal(refreshResponse.status,200);const refreshed=await refreshResponse.json() as NonNullable<typeof tokens>;
 const refreshedClaims=await jwtVerify(refreshed.access_token,createRemoteJWKSet(new URL(String(discovery.jwks_uri))),{issuer,audience:resource,algorithms:['RS256'],requiredClaims:['sub','iat','exp','jti']});assert.equal(refreshedClaims.payload.sub,verified.payload.sub);assert.equal(String(refreshedClaims.payload.scope).split(' ').includes('computer:control'),controlRequested);assert.notEqual(refreshed.refresh_token,tokens.refresh_token);tokens=refreshed;pass('Offline refresh without resource parameter retains exact MCP audience, owner and requested scope');
 let seq=0;
 async function rpc(method:string,params:unknown={},notify=false){const r=await fetch(resource,{method:'POST',headers:{Authorization:'Bearer '+tokens!.access_token,'Content-Type':'application/json',Accept:'application/json, text/event-stream','MCP-Protocol-Version':'2025-11-25'},body:JSON.stringify({jsonrpc:'2.0',...notify?{}:{id:++seq},method,params})});const requestId=r.headers.get('x-opencommander-request-id');assert.match(requestId??'',/^[a-f0-9-]{36}$/);assert(r.ok,'Authenticated MCP HTTP failure');const text=await r.text();if(notify){pass('Initialized notification accepted',{request_id:requestId});return null}const value=JSON.parse(r.headers.get('content-type')?.includes('text/event-stream')?text.split('\n').filter(l=>l.startsWith('data: ')).map(l=>l.slice(6)).join(''):text) as {error?:unknown;result:Record<string,unknown>};assert(!value.error,'JSON-RPC error');return {result:value.result,requestId};}
 stage='raw initialize and discovery';const initialized=await rpc('initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'opencommander-independent-diagnosis',version:'0.1.0'}});pass('Independent raw HTTP initialize',{request_id:initialized!.requestId,protocol:initialized!.result.protocolVersion});await rpc('notifications/initialized',{},true);
 const listed=await rpc('tools/list');const tools=listed!.result.tools as {name:string;inputSchema:Record<string,unknown>}[];assert.equal(tools.length,Object.keys(actions).length+compatibilityTools.length+9);assert(tools.every(t=>t.inputSchema.type==='object'));pass('Independent authenticated tools/list',{request_id:listed!.requestId,count:tools.length,names:tools.map(t=>t.name)});
 async function call(name:string,args:Record<string,unknown>){const result=await rpc('tools/call',{name,arguments:args});assert(result);return {data:result.result.structuredContent as Record<string,unknown>,error:result.result.isError===true,requestId:result.requestId,content:result.result.content};}
 stage='real device_list';const devices=await call('device_list',{});assert(!devices.error);const connected=(devices.data.devices as Record<string,unknown>[]).filter(d=>d.connected);assert.equal(connected.length,1,'Expected one connected owned device for this read-only diagnostic');const owned=connected[0]!;report.device_id=String(owned.device_id);pass('Actual installed owned device connected',{device_id:report.device_id,request_id:devices.requestId});
 stage='real Windows system_info';const submitted=await call('system_info',{device_id:report.device_id,idempotency_key:randomUUID(),timeout_seconds:30});assert(!submitted.error);report.task_id=String(submitted.data.task_id);pass('Read-only system_info submitted',{task_id:report.task_id,request_id:submitted.requestId});
 let completed:Awaited<ReturnType<typeof call>>|undefined;for(let n=0;n<60;n++){const status=await call('task_status',{task_id:report.task_id});assert(!status.error);if(['succeeded','failed','cancelled','expired','uncertain'].includes(String(status.data.status))){completed=status;break}await new Promise(r=>setTimeout(r,500));}assert.equal(completed?.data.status,'succeeded');const info=completed!.data.result as Record<string,unknown>;assert.equal(info.os,'Windows');assert(typeof info.cpu==='string'&&info.cpu.length>0);assert(Number(info.memory_total_bytes)>0);assert(Number(info.uptime_seconds)>0);report.system_info=info;pass('Actual Windows OS, CPU, RAM and uptime returned',{task_id:report.task_id,request_id:completed!.requestId});
  async function operate(name:string,args:Record<string,unknown>){const submitted=await call(name,{device_id:report.device_id,idempotency_key:randomUUID(),timeout_seconds:30,...args});assert(!submitted.error,'MCP '+name+' request rejected: '+JSON.stringify(submitted.data.error));for(let n=0;n<80;n++){const polled=await call('task_status',{task_id:submitted.data.task_id});assert(!polled.error);if(credentials.verify_unattended||credentials.verify_desktop||credentials.verify_all_applications)assert.notEqual(polled.data.status,'approval_pending','Trusted owner task unexpectedly requested a click');if(['succeeded','failed','cancelled','expired','uncertain'].includes(String(polled.data.status)))return {...polled.data,_content:polled.content};await new Promise(r=>setTimeout(r,250))}throw new Error('Profile access task did not finish')}
 if(credentials.verify_profile_access){
  stage='profile directory access';const workspace=info.workspace as {user_profile:string;approved_roots:string[];ask_before_sensitive:boolean};const manifest=await readFile(repo+'/apps/windows-agent/Cargo.toml','utf8');assert.equal(info.agent_version,/^version = "([^"]+)"$/m.exec(manifest)?.[1]);assert(workspace.approved_roots.includes(workspace.user_profile));

  if(credentials.verify_unattended){
   stage='trusted owner execution';assert.equal(workspace.ask_before_sensitive,false);const cwd=join(workspace.user_profile,'Documents');
   const terminal=await operate('terminal_execute',{command:"Write-Output 'OPENCOMMANDER-TRUSTED-OWNER-VERIFIED'",cwd,env:{}});assert.equal(terminal.status,'succeeded');assert.match(String((terminal.result as {stdout:string}).stdout),/OPENCOMMANDER-TRUSTED-OWNER-VERIFIED/);pass('PowerShell executed through MCP without local approval dialog');
   const launched=await operate('application_launch',{path:'C:\\Windows\\System32\\notepad.exe',args:[],cwd});assert.equal(launched.status,'succeeded');const pid=Number((launched.result as {pid:number}).pid);assert(pid>0);pass('Allowlisted application launched without local approval dialog');
   const stopped=await operate('process_manage',{op:'terminate',pid});assert.equal(stopped.status,'succeeded');pass('Newly launched fixture process terminated without local approval dialog');
  }
  for(const relative of ['Desktop','Documents','Downloads','AppData/Roaming']){
   const parent=join(workspace.user_profile,relative);const folder=join(parent,'.OpenCommander-access-'+randomUUID());const file=join(folder,'fixture.txt');fixtures.push({folder,file});
   assert.equal((await operate('file_list',{path:parent})).status,'succeeded');
   assert.equal((await operate('file_manage',{op:'mkdir',path:folder,preview:false})).status,'succeeded');
   assert.equal((await operate('file_write',{path:file,text:'Profile access verified through MCP'})).status,'succeeded');
   const read=await operate('file_read',{path:file});assert.equal(read.status,'succeeded');assert.equal((read.result as {text:string}).text,'Profile access verified through MCP');
   if(credentials.verify_unattended){const preview=await operate('file_manage',{op:'delete',path:file,preview:true});assert.equal(preview.status,'succeeded');assert.notEqual((preview.result as {requires_local_approval?:boolean}).requires_local_approval,true);}
   const search=await operate('file_search',{path:folder,query:'fixture'});assert.equal(search.status,'succeeded');assert.equal((search.result as {matches:string[]}).matches.length,1);pass('MCP list, mkdir, write, read and search in '+relative);
  }
  assert.equal((await operate('file_list',{path:workspace.user_profile})).status,'succeeded');pass('MCP can list the complete approved user profile');
  const internal=await operate('file_read',{path:join(workspace.user_profile,'AppData/Local/OpenCommander/config.json')});assert.equal(internal.status,'failed');assert.match(JSON.stringify(internal.error),/internal storage/);pass('MCP file access to internal agent configuration remains denied');
  assert.equal((await operate('file_list',{path:'C:\\Windows'})).status,'failed');pass('System directory outside approved user profile remains denied');
 }
 if(credentials.verify_desktop){
  stage='desktop screenshot and UI Automation regression';
  assert.equal(info.agent_version,/^version = "([^"]+)"$/m.exec(await readFile(repo+'/apps/windows-agent/Cargo.toml','utf8'))?.[1]);
  const automationSchema=tools.find(tool=>tool.name==='desktop_automation')!.inputSchema.properties as Record<string,unknown>;assert(automationSchema.element_path,'Production MCP schema must expose the inspected element path');
  assert.equal((info.workspace as {ask_before_sensitive:boolean}).ask_before_sensitive,false);
  const screenshot=await operate('desktop_screenshot',{});assert.equal(screenshot.status,'succeeded',String(screenshot.error));
  const image=(screenshot._content as {type:string;data?:string;mimeType?:string}[]).find(item=>item.type==='image');assert(image?.data);assert.equal(image.mimeType,'image/png');
  const png=Buffer.from(image.data,'base64');assert(png.length<=700000);assert.equal(png.subarray(0,8).toString('hex'),'89504e470d0a1a0a');
  const metadata=screenshot.result as {image_width:number;image_height:number;virtual_screen:{width:number;height:number}};
  assert.equal(png.readUInt32BE(16),metadata.image_width);assert.equal(png.readUInt32BE(20),metadata.image_height);
  const compressed:Buffer[]=[];for(let offset=8;offset<png.length;){const length=png.readUInt32BE(offset);if(png.toString('ascii',offset+4,offset+8)==='IDAT')compressed.push(png.subarray(offset+8,offset+8+length));offset+=length+12;}
  assert(inflateSync(Buffer.concat(compressed)).length>0);assert(metadata.image_width<=metadata.virtual_screen.width);assert(metadata.image_height<=metadata.virtual_screen.height);
  pass('Actual installed-agent screenshot returned as valid bounded MCP PNG',{bytes:png.length,image_width:metadata.image_width,image_height:metadata.image_height,virtual_screen:metadata.virtual_screen});
  const title='OpenCommander UIA regression '+randomUUID();
  const fixture=spawn('powershell.exe',['-NoProfile','-File',join(repo,'scripts','desktop-uia-fixture.ps1'),'-Title',title],{windowsHide:true,stdio:'ignore'});
  try{
   let hwnd:number|undefined;for(let n=0;n<30;n++){const windows=await operate('desktop_windows',{});const found=((windows.result as {windows:{hwnd:number;pid:number;title:string}[]}).windows).find(window=>window.pid===fixture.pid&&window.title===title);if(found){hwnd=found.hwnd;break}await new Promise(r=>setTimeout(r,200))}assert(hwnd,'Owned UIA fixture window did not appear');
   const focused=await operate('desktop_automation',{hwnd,op:'focus'});assert.equal(focused.status,'succeeded',String(focused.error));assert.equal((focused.result as {verified:boolean}).verified,true);
   pass('UI Automation window focus with HWND alone succeeds without selector or consent click');
   const rootFocus=await operate('desktop_automation',{hwnd,op:'focus',name:title});assert.equal(rootFocus.status,'succeeded',String(rootFocus.error));pass('UI Automation root-name focus verifies foreground window');
   const inspected=await operate('desktop_inspect',{hwnd,max_depth:8,max_elements:200});assert.equal(inspected.status,'succeeded');
   const elements=(inspected.result as {elements:{name:string;element_path:number[];focusable:boolean}[]}).elements;
   const editors=elements.filter(element=>element.name==='Repeated editor');assert.equal(editors.length,2);assert(editors.every(element=>element.focusable&&element.element_path.length>0));
   const ambiguous=await operate('desktop_automation',{hwnd,op:'focus',name:'Repeated editor'});assert.equal(ambiguous.status,'failed');assert.match(String(ambiguous.error),/ambiguous/);
   const selected=await operate('desktop_automation',{hwnd,op:'focus',name:'Repeated editor',element_path:editors[1]!.element_path});assert.equal(selected.status,'succeeded',String(selected.error));assert.equal((selected.result as {after:{focused:boolean}}).after.focused,true);
   pass('Duplicate control names remain refused; inspected element_path focuses the intended control');
   const changed=await operate('desktop_automation',{hwnd,op:'focus',name:'Wrong identity',element_path:editors[1]!.element_path});assert.equal(changed.status,'failed');assert.match(String(changed.error),/identity changed/);pass('Stale path identity is rejected without choosing another control');
   assert(!elements.some(element=>element.name==='Deep unique editor'),'Deep fixture must lie beyond inspection depth');assert.equal((inspected.result as {truncated:boolean}).truncated,true);
   const deep=await operate('desktop_automation',{hwnd,op:'focus',name:'Deep unique editor'});assert.equal(deep.status,'succeeded',String(deep.error));assert.equal((deep.result as {after:{focused:boolean}}).after.focused,true);
   pass('Exact selector reaches control beyond truncated inspection tree and verifies keyboard focus');
   const missing=await operate('desktop_automation',{hwnd,op:'focus',name:'Nonexistent fixture control'});assert.equal(missing.status,'failed');assert.match(String(missing.error),/matched no controls/);pass('Nonexistent selector is safely rejected with actionable inspection guidance');
  }finally{if(fixture.exitCode===null&&fixture.signalCode===null){await new Promise<void>(resolve=>{fixture.once('close',()=>resolve());fixture.kill();});}}
 }
 if(credentials.verify_all_applications){
  stage='all applications and actual VS Code launch';
  const workspace=info.workspace as {user_profile:string;allow_all_applications:boolean;approved_applications:string[];ask_before_sensitive:boolean};
  assert.equal(info.agent_version,/^version = "([^"]+)"$/m.exec(await readFile(repo+'/apps/windows-agent/Cargo.toml','utf8'))?.[1]);assert.equal(workspace.allow_all_applications,true);assert.equal(workspace.ask_before_sensitive,false);pass('Installed agent reports persistent all-applications mode and owner preapproval');
  const executable=join(workspace.user_profile,'AppData/Local/Programs/Microsoft VS Code/Code.exe');assert(!workspace.approved_applications.includes(executable));
  const runtime=await realpath(join(repo,'.runtime'));const profile=await mkdtemp(join(runtime,'vscode-launch-fixture-'));let pid:number|undefined;
  try{
   const launched=await operate('application_launch',{path:executable,args:['--new-window','--user-data-dir',join(profile,'profile'),'--extensions-dir',join(profile,'extensions'),'--disable-extensions','--skip-welcome','--skip-release-notes']});assert.equal(launched.status,'succeeded',String(launched.error));pid=Number((launched.result as {pid:number}).pid);assert(pid>0);
   let hwnd:number|undefined;for(let n=0;n<30;n++){const windows=await operate('desktop_windows',{});const window=((windows.result as {windows:{hwnd:number;pid:number}[]}).windows).find(window=>window.pid===pid);if(window){hwnd=window.hwnd;break}await new Promise(r=>setTimeout(r,300))}assert(hwnd,'New isolated VS Code window did not appear');pass('VS Code outside explicit allowlist launches through production MCP and opens a real window',{pid,hwnd,task_id:launched.task_id});
   assert.equal((await operate('desktop_automation',{hwnd,op:'focus'})).status,'succeeded');const close=await operate('desktop_interact',{hwnd,op:'shortcut',keys:['ALT','F4']});assert.equal(close.status,'succeeded');
   for(let n=0;n<20;n++){const windows=await operate('desktop_windows',{});if(!((windows.result as {windows:{pid:number}[]}).windows).some(window=>window.pid===pid))break;await new Promise(r=>setTimeout(r,250))}pass('New VS Code fixture window closes through the remote control tools');
  }finally{
   const cleanup=spawn('powershell.exe',['-NoProfile','-File',join(repo,'scripts','cleanup-vscode-fixture.ps1'),'-ProfileRoot',profile,'-Executable',executable],{windowsHide:true,stdio:['ignore','pipe','pipe']});let error='';cleanup.stderr.on('data',part=>{error+=String(part)});const exit=await new Promise<number|null>(done=>cleanup.once('close',done));assert.equal(exit,0,'VS Code fixture cleanup failed: '+error);
   const target=await realpath(profile);assert.equal(target,resolve(profile));assert(target.startsWith(runtime+sep+'vscode-launch-fixture-'),'Refusing cleanup outside the newly created runtime fixture');await rm(target,{recursive:true,force:false});
  }
  pass('Isolated VS Code fixture processes and temporary profile removed');
  const missing=await operate('application_launch',{path:join(workspace.user_profile,'missing-application-'+randomUUID()+'.exe'),args:[]});assert.equal(missing.status,'failed');assert.match(String(missing.error),/existing absolute executable/);pass('All-applications mode still rejects nonexistent executable paths');
 }
 stage='security regression';if(!controlRequested){const control=await call('device_revoke',{device_id:report.device_id});assert(control.error);assert.equal((control.data.error as Record<string,unknown>).code,'INSUFFICIENT_SCOPE');pass('Read-only token denied control before device mutation');}
 const foreign=await call('device_status',{device_id:'unowned-diagnostic-device'});assert(foreign.error);pass('Unowned device denied');
 const malformed=await rpc('tools/call',{name:'system_info',arguments:{device_id:report.device_id,timeout_seconds:0,idempotency_key:randomUUID()}});assert.equal(malformed!.result.isError,true);pass('Invalid tool schema denied');
 }
}catch(error){report.checks.push({name:stage,status:'FAIL',detail:error instanceof Error?error.message:'Diagnostic check failed'});process.exitCode=1;console.error('FAIL '+stage+': '+(error instanceof Error?error.message:'Diagnostic check failed'));}finally{
 for(const fixture of fixtures){await unlink(fixture.file).catch(()=>{});await rmdir(fixture.folder).catch(()=>{});}
 if(tokens&&credentials.client_secret&&revokeEndpoint){try{const r=await fetch(revokeEndpoint,{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({client_id:'opencommander-chatgpt',client_secret:credentials.client_secret,token:tokens.refresh_token,token_type_hint:'refresh_token'})});assert.equal(r.status,200);pass('Diagnostic refresh token revoked');}catch{report.checks.push({name:'Diagnostic refresh token cleanup',status:'FAIL'});process.exitCode=1;}}
 await writeFile(repo+'/artifacts/'+(credentials.verify_all_applications?'all-applications-results.json':credentials.verify_desktop?'desktop-fix-results.json':credentials.verify_unattended?'unattended-access-results.json':credentials.verify_profile_access?'profile-access-results.json':'chatgpt-integration-diagnostics.json'),JSON.stringify(report,null,2));
}
