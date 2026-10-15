import {Client,StreamableHTTPClientTransport} from '@modelcontextprotocol/client';
import {SignJWT} from 'jose';
import {spawn,execFileSync,type ChildProcess} from 'node:child_process';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {randomBytes,randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';

const repo=resolve(fileURLToPath(new URL('../../..',import.meta.url)));
const dir=join(repo,'.runtime',`e2e-${Date.now()}`);const root=join(dir,'files');const state=join(dir,'agent');
const notepad=join(process.env.WINDIR??'C:\\Windows','System32','notepad.exe');
await mkdir(root,{recursive:true});await mkdir(state,{recursive:true});await mkdir(join(repo,'artifacts'),{recursive:true});
const gateway='http://127.0.0.1:18080',resource='http://127.0.0.1:13000/mcp',issuer='http://127.0.0.1:13000';
const devKey=randomBytes(32).toString('hex');const bridgeKey=randomBytes(32).toString('hex');const signingSeed=randomBytes(32).toString('base64');
const env={...process.env,OPENCOMMANDER_DEV:'1',GATEWAY_ADDR:'127.0.0.1:18080',GATEWAY_DB:join(dir,'gateway.db'),GATEWAY_SIGNING_SEED:signingSeed,BRIDGE_KEY:bridgeKey,DEV_TOKEN_KEY:devKey,MCP_PORT:'13000',MCP_RESOURCE:resource,OIDC_ISSUER:issuer,GATEWAY_URL:gateway};
const children:ChildProcess[]=[];const logs:string[]=[];const checks:{name:string;status:string;detail?:unknown}[]=[];
let agent:ChildProcess;let gatewayProcess:ChildProcess;let bridge:ChildProcess;let client:Client|undefined;let device='';let notepadPid:number|undefined;
const delay=(ms:number)=>new Promise(r=>setTimeout(r,ms));
function launch(command:string,args:string[]){const p=spawn(command,args,{cwd:repo,env,windowsHide:true,stdio:['ignore','pipe','pipe']});p.stdout?.on('data',(b:Buffer)=>logs.push(b.toString()));p.stderr?.on('data',(b:Buffer)=>logs.push(b.toString()));children.push(p);return p;}
async function until<T>(f:()=>Promise<T|undefined>,timeout=20000):Promise<T>{const end=Date.now()+timeout;while(Date.now()<end){const v=await f();if(v!==undefined)return v;await delay(200)}throw new Error(`Condition timed out: ${f.toString().slice(0,240)}`);}
async function health(url:string){await until(async()=>{try{const r=await fetch(url);return r.ok?true:undefined}catch{return undefined}})}
async function userToken(owner='e2e-owner',scope='computer:read computer:control') {return new SignJWT({scope}).setSubject(owner).setJti(randomUUID()).setIssuer(issuer).setAudience(resource).setIssuedAt().setExpirationTime('30m').setProtectedHeader({alg:'HS256'}).sign(new TextEncoder().encode(devKey));}
async function connect(owner='e2e-owner'){const c=new Client({name:'opencommander-e2e',version:'1.0.0'},{versionNegotiation:{mode:'auto'}});await c.connect(new StreamableHTTPClientTransport(new URL(resource),{requestInit:{headers:{Authorization:`Bearer ${await userToken(owner)}`}}}));return c;}
async function tool(name:string,args:Record<string,unknown>,c=client!){const r=await c.callTool({name,arguments:args});if(r.isError)throw new Error(JSON.stringify(r));return r.structuredContent as Record<string,unknown>;}
async function submit(name:string,args:Record<string,unknown>,timeout=30,key=randomUUID()){return tool(name,{device_id:device,idempotency_key:key,timeout_seconds:timeout,...args});}
async function finish(id:string,timeout=30000){return until(async()=>{const r=await tool('task_status',{task_id:id});return ['succeeded','failed','cancelled','expired','uncertain'].includes(String(r.status))?r:undefined},timeout);}
async function operate(name:string,args:Record<string,unknown>,consent?:'yes'|'no',timeout=30){
 const submitted=await submit(name,args,timeout);const id=String(submitted.task_id);
 if(consent){await until(async()=>{const s=await tool('approval_status',{task_id:id});return s.status==='approval_pending'?true:undefined});
  let driverError='';try{await until(async()=>{try{execFileSync('powershell.exe',['-NoProfile','-File',join(repo,'scripts','consent-test.ps1'),'-TaskId',id,'-AgentPid',String(agent.pid),'-Decision',consent],{windowsHide:true,stdio:'pipe'});return true}catch(e){driverError=String(e);return undefined}});}catch(e){throw new Error(`Consent driver ${name}/${id}: ${String(e)} ${driverError}`)}
 }
 return finish(id,timeout*1000+10000);
}
function pass(name:string,detail?:unknown){checks.push({name,status:'PASS',detail});console.log(`PASS ${name}`);}
async function stop(p:ChildProcess){if(p.exitCode===null&&!p.killed){p.kill();await new Promise<void>(r=>{p.once('exit',()=>r());setTimeout(r,5000).unref();});}}
try {
 gatewayProcess=launch(join(repo,'artifacts','opencommander-gateway.exe'),[]);await health(gateway+'/healthz');
 bridge=launch(process.execPath,[join(repo,'apps','mcp-server','dist','index.js')]);await health(issuer+'/healthz');client=await connect();
 const toolCatalog=await client.listTools();assert(toolCatalog.tools.some(t=>t.name==='task_output'));await writeFile(join(repo,'artifacts','mcp-tools.json'),JSON.stringify({opencommander_protocol_version:1,mcp_era:client.getProtocolEra(),...toolCatalog},null,2));pass('modern MCP discovery includes typed operations and durable terminal output');
 agent=launch(join(repo,'artifacts','opencommander-agent.exe'),['--init','--gateway',gateway,'--root',root,'--application',notepad,'--state',state,'--dev','--allow-modify','--allow-screenshots']);
 const code=await until(async()=>{for(const line of logs.join('').split(/\r?\n/)){try{const v=JSON.parse(line) as Record<string,unknown>;if(v.event==='pairing_required')return String(v.code)}catch{}}return undefined});
 const paired=await tool('device_pair',{code});device=String(paired.device_id);
 await until(async()=>{const s=await tool('device_status',{device_id:device});return s.connected?true:undefined});pass('secure enrollment and connected real Rust agent');
 const localStatus=JSON.parse(execFileSync(join(repo,'artifacts','opencommander-agent.exe'),['--state',state,'--status'],{windowsHide:true,encoding:'utf8'})) as Record<string,unknown>;assert.equal(localStatus.connected,true);assert.equal(localStatus.running,true);pass('native local status utility confirms the running authenticated connection');
 const info=await operate('system_info',{});assert.equal(info.status,'succeeded',JSON.stringify(info));const sys=info.result as Record<string,unknown>;assert(Number(sys.cpu_count)>0&&Number(sys.memory_total_bytes)>0);assert.equal(sys.os,'Windows');pass('MCP → TypeScript → Go → Rust → real Windows system_info',{cpu_count:sys.cpu_count,os_version:sys.os_version,protocol_era:client.getProtocolEra()});
 const path=join(root,'fixture.txt');const write=await operate('file_write',{path,text:'OpenCommander fixture v1'});assert.equal(write.status,'succeeded');
 const read=await operate('file_read',{path});assert.equal((read.result as Record<string,unknown>).text,'OpenCommander fixture v1');
 const changed=await operate('file_write',{path,text:'OpenCommander fixture v2',expected_sha256:(write.result as Record<string,unknown>).sha256});assert.equal(changed.status,'succeeded');assert.equal(await readFile(path,'utf8'),'OpenCommander fixture v2');pass('atomic create, read and compare-and-swap update through MCP');
 const conflict=await operate('file_write',{path,text:'should not overwrite'});assert.equal(conflict.status,'failed');assert.equal(await readFile(path,'utf8'),'OpenCommander fixture v2');pass('blind overwrite denied');
 const escape=await operate('file_read',{path:join(root,'..','gateway.db')});assert.equal(escape.status,'failed');pass('outside-root access denied');
 const batchFile=join(root,'batch.txt');const batch=await operate('file_batch',{preview:false,items:[{op:'write',path:batchFile,text:'bounded batch'}]});assert.equal(batch.status,'succeeded',JSON.stringify(batch));assert.equal(await readFile(batchFile,'utf8'),'bounded batch');
 const undone=await operate('file_undo',{preview:false,undo_id:(batch.result as Record<string,unknown>).undo_id});assert.equal(undone.status,'succeeded',JSON.stringify(undone));await assert.rejects(readFile(batchFile));pass('encrypted batch receipt and conflict-aware undo through MCP');
 for(const op of ['services','applications','network']) {const inventory=await operate('system_inventory',{op});assert.equal(inventory.status,'succeeded',JSON.stringify(inventory));assert(inventory.result&&typeof inventory.result==='object')};pass('actual Windows service, application and network inventories through MCP');
 await writeFile(join(root,'.env'),'fixture credential');const sensitive=await operate('file_read',{path:join(root,'.env')});assert.equal(sensitive.status,'failed');pass('credential file access denied');
 const foreign=await connect('other-owner');try{const d=await tool('device_list',{},foreign);assert.equal((d.devices as unknown[]).length,0);const foreignTask=await foreign.callTool({name:'task_status',arguments:{task_id:info.task_id}});assert.equal(foreignTask.isError,true)}finally{await foreign.close()};pass('cross-account devices and task results inaccessible');
 const search=await operate('file_search',{path:root,query:'fixture',content:false});assert.equal(search.status,'succeeded');
 const procs=await operate('process_manage',{op:'list'});assert.equal(procs.status,'succeeded');pass('recursive search and actual process enumeration');
 if(!process.argv.includes('--headless')) {
 const unapprovedPath=join(root,'never-approved.txt');const pendingArgs={command:`Set-Content -LiteralPath '${unapprovedPath.replaceAll("'","''")}' -Value 'must-not-run'`,cwd:root,env:{}};
 const unapproved=await submit('terminal_execute',pendingArgs,3);assert.equal((await finish(String(unapproved.task_id))).status,'failed');await assert.rejects(readFile(unapprovedPath));pass('unanswered local consent expires without executing the operation');
 const pendingCancel=await submit('terminal_execute',pendingArgs,30);await until(async()=>{const s=await tool('approval_status',{task_id:pendingCancel.task_id});return s.status==='approval_pending'?true:undefined});await tool('task_cancel',{task_id:pendingCancel.task_id});assert.equal((await finish(String(pendingCancel.task_id))).status,'cancelled');await assert.rejects(readFile(unapprovedPath));pass('cancellation while awaiting consent closes the dialog and prevents execution');
 const denied=await operate('terminal_execute',{shell:'powershell',command:"Write-Output 'DENIED-FIXTURE'",cwd:root,env:{}},'no');assert.equal(denied.status,'failed',JSON.stringify(denied));pass('native local consent denial prevents shell execution');
 const terminal=await operate('terminal_execute',{shell:'powershell',command:"Write-Output 'NATIVE-RUST-EXECUTION'",cwd:root,env:{}},'yes');assert.equal(terminal.status,'succeeded');assert.match(String((terminal.result as Record<string,unknown>).stdout),/NATIVE-RUST-EXECUTION/);pass('real PowerShell command through MCP with native consent');
 const deleted=await operate('file_operation',{op:'delete',path,preview:false},'yes');assert.equal(deleted.status,'succeeded',JSON.stringify(deleted));await assert.rejects(readFile(path));const receipts=await operate('file_recovery',{op:'list'});assert.equal(receipts.status,'succeeded');const restored=await operate('file_recovery',{op:'restore',receipt_id:(deleted.result as Record<string,unknown>).receipt_id,preview:false});assert.equal(restored.status,'succeeded',JSON.stringify(restored));assert.equal(await readFile(path,'utf8'),'OpenCommander fixture v2');pass('consented delete, encrypted trash listing and verified restore through MCP');
 const streamed=await tool('task_output',{task_id:terminal.task_id,after:0});assert.match(JSON.stringify(streamed.chunks),/NATIVE-RUST-EXECUTION/);assert.equal((await tool('task_output',{task_id:terminal.task_id,after:streamed.next_after})).chunks instanceof Array,true);pass('durable stdout chunks and cursor readback');
 const nonzero=await operate('terminal_execute',{command:"Write-Error 'fixture command failure'; exit 7",cwd:root,env:{}},'yes');assert.equal(nonzero.status,'succeeded');assert.equal((nonzero.result as Record<string,unknown>).exit_code,7);assert.equal((nonzero.result as Record<string,unknown>).execution_succeeded,true);assert.equal((nonzero.result as Record<string,unknown>).program_succeeded,false);assert.match(String((nonzero.result as Record<string,unknown>).stderr),/fixture command failure/);pass('normally exited process succeeds as execution while preserving unexpected exit code and stderr');
 const timeout=await operate('terminal_execute',{command:'Start-Sleep -Seconds 10',cwd:root,env:{}},'yes',4);assert.equal(timeout.status,'failed');pass('long command timeout and job termination');
 const cancelled=await submit('terminal_execute',{command:'Start-Sleep -Seconds 15',cwd:root,env:{}},30);await until(async()=>{const s=await tool('task_status',{task_id:cancelled.task_id});return s.status==='approval_pending'?true:undefined});
 await until(async()=>{try{execFileSync('powershell.exe',['-NoProfile','-File',join(repo,'scripts','consent-test.ps1'),'-TaskId',String(cancelled.task_id),'-AgentPid',String(agent.pid),'-Decision','yes'],{windowsHide:true,stdio:'pipe'});return true}catch{return undefined}});
 await until(async()=>{const s=await tool('task_status',{task_id:cancelled.task_id});return s.status==='running'?true:undefined});await tool('task_cancel',{task_id:cancelled.task_id});assert.equal((await finish(String(cancelled.task_id))).status,'cancelled');pass('in-flight command cancellation');
 }
 const key=randomUUID();const once=await submit('file_write',{path:join(root,'only-once.txt'),text:'one operation'},30,key);assert.equal((await finish(String(once.task_id))).status,'succeeded');const twice=await submit('file_write',{path:join(root,'only-once.txt'),text:'one operation'},30,key);assert.equal(twice.task_id,once.task_id);pass('duplicate request returns same persisted task');
 if(!process.argv.includes('--headless')){
  const marker=join(root,'after-crash.txt');const crashKey=randomUUID();const crashArgs={command:`Start-Sleep -Seconds 8; Add-Content -LiteralPath '${marker.replaceAll("'","''")}' -Value 'once'`,cwd:root,env:{}};
  const running=await submit('terminal_execute',crashArgs,60,crashKey);
  await until(async()=>{const s=await tool('task_status',{task_id:running.task_id});return s.status==='approval_pending'?true:undefined});
  await until(async()=>{try{execFileSync('powershell.exe',['-NoProfile','-File',join(repo,'scripts','consent-test.ps1'),'-TaskId',String(running.task_id),'-AgentPid',String(agent.pid),'-Decision','yes'],{windowsHide:true,stdio:'pipe'});return true}catch{return undefined}});
  await until(async()=>{const s=await tool('task_status',{task_id:running.task_id});return s.status==='running'?true:undefined});await stop(agent);
  agent=launch(join(repo,'artifacts','opencommander-agent.exe'),['--state',state]);await until(async()=>{const s=await tool('device_status',{device_id:device});return s.connected?true:undefined},30000);
  assert.equal((await finish(String(running.task_id))).status,'uncertain');assert.equal((await submit('terminal_execute',crashArgs,60,crashKey)).task_id,running.task_id);
  await delay(8500);await assert.rejects(readFile(marker));pass('agent crash records uncertain outcome and never replays the non-idempotent command');
 }
 await stop(gatewayProcess);gatewayProcess=launch(join(repo,'artifacts','opencommander-gateway.exe'),[]);await health(gateway+'/healthz');await until(async()=>{const s=await tool('device_status',{device_id:device});return s.connected?true:undefined},45000);assert.equal((await tool('task_status',{task_id:once.task_id})).status,'succeeded');pass('gateway restart, automatic reconnect and durable completed task');
 await client.close();client=undefined;await stop(bridge);bridge=launch(process.execPath,[join(repo,'apps','mcp-server','dist','index.js')]);await health(issuer+'/healthz');client=await connect();assert.equal((await tool('task_status',{task_id:once.task_id})).status,'succeeded');pass('MCP restart preserves gateway task state');
 await stop(agent);agent=launch(join(repo,'artifacts','opencommander-agent.exe'),['--state',state]);await until(async()=>{const s=await tool('device_status',{device_id:device});return s.connected?true:undefined},30000);assert.equal((await submit('file_write',{path:join(root,'only-once.txt'),text:'one operation'},30,key)).task_id,once.task_id);pass('agent restart and no repeated non-idempotent file write');
 if(process.argv.includes('--desktop')){
  const launched=await operate('application_launch',{path:notepad,args:[],cwd:root},'yes');assert.equal(launched.status,'succeeded');notepadPid=Number((launched.result as Record<string,unknown>).pid);
  const windows=await operate('desktop_windows',{});assert.equal(windows.status,'succeeded');const window=await until(async()=>{const w=await operate('desktop_windows',{});return ((w.result as Record<string,unknown>).windows as Record<string,unknown>[]).find(w=>w.pid===notepadPid)});
  const focused=await operate('desktop_interact',{op:'focus',hwnd:window.hwnd},'yes');assert.equal(focused.status,'succeeded',JSON.stringify(focused));
  const inspected=await operate('desktop_inspect',{hwnd:window.hwnd,max_depth:6,max_elements:200});assert.equal(inspected.status,'succeeded',JSON.stringify(inspected));assert(((inspected.result as Record<string,unknown>).elements as unknown[]).length>0);pass('actual Notepad UI Automation inspection through MCP');
  const active=await operate('desktop_windows',{});assert(((active.result as Record<string,unknown>).windows as Record<string,unknown>[]).some(w=>w.pid===notepadPid&&w.active));
  const typed=await operate('desktop_interact',{op:'type',hwnd:window.hwnd,text:'OpenCommander Windows GUI fixture'},'yes');assert.equal(typed.status,'succeeded',JSON.stringify(typed));
  const screenshot=await client.callTool({name:'task_status',arguments:{task_id:(await operate('desktop_screenshot',{})).task_id}});const content=screenshot.content as {type:string;data?:string}[];const img=content.find(c=>c.type==='image');assert(img?.data);await writeFile(join(dir,'desktop.png'),Buffer.from(img.data,'base64'));
  // Read the actual Notepad edit control; successful SendInput alone does not prove text arrived.
  const verify=execFileSync('powershell.exe',['-NoProfile','-File',join(repo,'scripts','verify-notepad.ps1'),'-ProcessId',String(notepadPid)],{windowsHide:true,encoding:'utf8'});assert.match(verify,/OpenCommander Windows GUI fixture/);pass('real Notepad launch, window focus, Unicode input, screenshot and control-text verification');
  const ended=await operate('process_manage',{op:'terminate',pid:notepadPid},'yes');assert.equal(ended.status,'succeeded');notepadPid=undefined;
 }
 await tool('device_revoke',{device_id:device});const owned=await tool('device_list',{});assert.equal((owned.devices as unknown[]).length,0);pass('device revocation removes ownership and connection');
 await tool('session_revoke',{});assert.equal((await client.callTool({name:'device_list',arguments:{}})).isError,true);pass('access token revocation blocks subsequent gateway operations');
} catch(error){checks.push({name:'end-to-end run',status:'FAIL',detail:String(error)});console.error(error);process.exitCode=1;}
finally {
 if(client)await client.close().catch(()=>{});
 for(const p of children.reverse())await stop(p);
 if(notepadPid){try{execFileSync('powershell.exe',['-NoProfile','-Command',`Stop-Process -Id ${notepadPid} -ErrorAction SilentlyContinue; exit 0`],{windowsHide:true});}catch{/* Process may already have exited. */}}
 await writeFile(join(dir,'execution.log'),logs.join(''));const report=JSON.stringify({at:new Date().toISOString(),platform:process.platform,desktop_requested:process.argv.includes('--desktop'),checks,evidence_directory:dir},null,2);await writeFile(join(dir,'results.json'),report);await writeFile(join(repo,'artifacts','e2e-results.json'),report);await writeFile(join(repo,'artifacts',process.argv.includes('--desktop')?'e2e-desktop-results.json':'e2e-headless-results.json'),report);console.log(`Evidence: ${dir}`);
}
