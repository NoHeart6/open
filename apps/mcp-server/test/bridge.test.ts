import {test} from 'node:test';
import assert from 'node:assert/strict';
import {SignJWT} from 'jose';
import {generateKeyPair,exportJWK} from 'jose';
import {createServer} from 'node:http';
import {Client as ModernClient,StreamableHTTPClientTransport as ModernTransport} from '@modelcontextprotocol/client';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import {config,serverVersion,type Config} from '../src/config.js';
import {authenticator} from '../src/auth.js';
import {app} from '../src/index.js';
import {actions,compatibilityTools} from '../src/tools.js';
const c:Config={port:0,host:'127.0.0.1',resource:'http://127.0.0.1:3000/mcp',issuer:'http://127.0.0.1:3000',jwksUrl:'',gateway:'http://127.0.0.1:8080',bridgeKey:'b'.repeat(32),devKey:'d'.repeat(32),dev:true};
async function token(scope='computer:read computer:control',aud=c.resource,expiry='1m'){
 return new SignJWT({scope}).setProtectedHeader({alg:'HS256'}).setIssuer(c.issuer).setSubject('alice').setJti('test-token').setAudience(aud).setIssuedAt().setExpirationTime(expiry).sign(new TextEncoder().encode(c.devKey));
}
test('tokens require signature, expiry, audience, subject and scopes',async()=>{
 const auth=authenticator(c);assert.equal((await auth(`Bearer ${await token()}`)).subject,'alice');
 await assert.rejects(auth(undefined));await assert.rejects(auth('Bearer garbage'));
 await assert.rejects(auth(`Bearer ${await token('computer:read','wrong')}`));await assert.rejects(auth(`Bearer ${await token('computer:read',c.resource,'-1s')}`));await assert.rejects(auth(`Bearer ${await token('other')}`));
});
test('production config rejects plaintext OAuth and weak secrets',()=>{
 assert.throws(()=>config({MCP_RESOURCE:c.resource,OIDC_ISSUER:c.issuer,BRIDGE_KEY:'x',OIDC_JWKS_URL:'http://example.com/keys'}));
 assert.throws(()=>config({OPENCOMMANDER_DEV:'1',MCP_HOST:'0.0.0.0',MCP_RESOURCE:c.resource,OIDC_ISSUER:c.issuer,BRIDGE_KEY:c.bridgeKey,DEV_TOKEN_KEY:c.devKey}));
});
test('Railway private hostname is accepted without admitting public lookalikes',()=>{
 const env={MCP_RESOURCE:'https://mcp.example.com/mcp',OIDC_ISSUER:'https://auth.example.com',OIDC_JWKS_URL:'https://auth.example.com/keys',BRIDGE_KEY:c.bridgeKey};
 assert.equal(config({...env,GATEWAY_URL:'http://gateway.railway.internal:8080'}).gateway,'http://gateway.railway.internal:8080');
 for(const host of ['gateway.railway.internal.evil.com','railway.internal','gateway.example.com'])assert.throws(()=>config({...env,GATEWAY_URL:`http://${host}:8080`}));
});
test('typed operation inputs reject traversal-independent malformed requests',()=>{
 assert.equal(actions.file_read.schema.safeParse({path:'C:\\test',limit:999999}).success,false);
 assert.equal(actions.desktop_interact.schema.safeParse({op:'shortcut',keys:['DELETE_ALL']}).success,false);
 assert.equal(actions.file_write.schema.safeParse({path:'C:\\test',text:'x',expected_sha256:'wrong'}).success,false);
 assert.equal(actions.terminal_execute.schema.safeParse({cwd:'C:\\test',command:'fixture',interactive:true,stdin:'hello\n'}).success,true);
 assert.equal(actions.terminal_input.schema.safeParse({target_task_id:'fixture-terminal',text:'hello\n'}).success,true);
 assert.equal(actions.terminal_input.schema.safeParse({target_task_id:'fixture-terminal',text:'x'.repeat(16385)}).success,false);
 assert.equal(actions.terminal_input.schema.safeParse({target_task_id:'short',eof:true}).success,false);
});
test('real MCP SDK discovery, schema errors, permission errors and auth challenge',async()=>{
 const listener=app(c).listen(0,'127.0.0.1');await new Promise<void>(resolve=>listener.once('listening',resolve));
 const address=listener.address();assert(address&&typeof address!=='string');c.resource=`http://127.0.0.1:${address.port}/mcp`;
 const client=new Client({name:'test',version:'1.0.0'});try{
  const noAuth=await fetch(c.resource,{method:'POST',headers:{'Content-Type':'application/json'},body:'{}'});assert.equal(noAuth.status,401);assert.match(noAuth.headers.get('www-authenticate')??'',/resource_metadata/);
  const badOrigin=await fetch(c.resource,{method:'POST',headers:{Origin:'https://evil.example','Content-Type':'application/json'},body:'{}'});assert.equal(badOrigin.status,403);
  await client.connect(new StreamableHTTPClientTransport(new URL(c.resource),{requestInit:{headers:{Authorization:`Bearer ${await token('computer:read')}`}}}));
  assert.equal(client.getServerVersion()?.version,serverVersion);
  assert.equal((await fetch(new URL('/healthz',c.resource)).then(response=>response.json()) as {version:string}).version,serverVersion);
  const tools=await client.listTools();assert.equal(tools.tools.length,Object.keys(actions).length+compatibilityTools.length+10);assert(tools.tools.every(t=>t.inputSchema.type==='object'));assert(tools.tools.every(t=>t.outputSchema?.type==='object'));for(const name of compatibilityTools)assert(tools.tools.some(t=>t.name===name));
  assert.equal(tools.tools.find(t=>t.name==='task_output')?.annotations?.readOnlyHint,true);
  const terminal=tools.tools.find(t=>t.name==='terminal_execute');const properties=terminal?.inputSchema.properties as Record<string,unknown>;assert(properties.interactive&&properties.stdin);assert(tools.tools.some(t=>t.name==='terminal_input'));
  const viewer=tools.tools.find(t=>t.name==='desktop_view');assert.equal(viewer?.annotations?.readOnlyHint,true);assert.equal(viewer?._meta?.['openai/outputTemplate'],'ui://opencommander/desktop-v1.html');
  const template=await client.readResource({uri:'ui://opencommander/desktop-v1.html'});assert.equal(template.contents[0]?.mimeType,'text/html;profile=mcp-app');assert('text' in template.contents[0]!&&template.contents[0].text.includes('ui/notifications/tool-result'));
  const modern=new ModernClient({name:'modern-test',version:'1.0.0'},{versionNegotiation:{mode:{pin:'2026-07-28'}}});
  try{
   await modern.connect(new ModernTransport(new URL(c.resource),{requestInit:{headers:{Authorization:`Bearer ${await token('computer:read')}`}}}));
   assert.equal(modern.getProtocolEra(),'modern');assert.equal((await modern.listTools()).tools.length,tools.tools.length);
  }finally{await modern.close()}
  const denied=await client.callTool({name:'terminal_execute',arguments:{device_id:'test-device',idempotency_key:'test-request',command:'echo hi',cwd:'C:\\test'}});assert.equal(denied.isError,true);assert.match(JSON.stringify(denied),/control scope/);
  const inputDenied=await client.callTool({name:'terminal_input',arguments:{device_id:'test-device',idempotency_key:'test-input-request',target_task_id:'fixture-terminal',text:'hello\n'}});assert.equal(inputDenied.isError,true);assert.match(JSON.stringify(inputDenied),/control scope/);
  const invalid=await client.callTool({name:'file_read',arguments:{device_id:'test-device',idempotency_key:'test-request',path:'C:\\test',limit:999999}});assert.equal(invalid.isError,true);
  const unavailable=await client.callTool({name:'device_list',arguments:{}});assert.equal(unavailable.isError,true);
 }finally{await client.close();await new Promise<void>((resolve,reject)=>listener.close(e=>e?reject(e):resolve()));}
});
test('production-mode verifier validates asymmetric JWKS without accepting development tokens',async()=>{
 const {privateKey,publicKey}=await generateKeyPair('RS256');const jwk={...await exportJWK(publicKey),kid:'fixture-key',alg:'RS256'};
 const jwks=createServer((_req,res)=>{res.setHeader('Content-Type','application/json');res.end(JSON.stringify({keys:[jwk]}))}).listen(0,'127.0.0.1');await new Promise<void>(r=>jwks.once('listening',r));
 const address=jwks.address();assert(address&&typeof address!=='string');
 // Only the fixture JWKS transport is loopback. Production config rejects non-HTTPS JWKS.
 const cfg={...c,dev:false,jwksUrl:`http://127.0.0.1:${address.port}/keys`};const verify=authenticator(cfg);
 try{
  const good=await new SignJWT({scope:'computer:read'}).setIssuer(c.issuer).setSubject('alice').setJti('asymmetric-token').setAudience(c.resource).setIssuedAt().setExpirationTime('1m').setProtectedHeader({alg:'RS256',kid:'fixture-key'}).sign(privateKey);
  assert.equal((await verify(`Bearer ${good}`)).subject,'alice');await assert.rejects(verify(`Bearer ${await token()}`));
 }finally{await new Promise<void>(r=>jwks.close(()=>r()))}
});
test('request correlation never logs tokens, private paths or tool input content',async()=>{
 const events:Record<string,unknown>[]=[];
 const gateway=createServer((_req,res)=>{res.setHeader('Content-Type','application/json');res.end(JSON.stringify({task_id:'fixture-task-123',status:'queued'}))}).listen(0,'127.0.0.1');await new Promise<void>(r=>gateway.once('listening',r));
 const gw=gateway.address();assert(gw&&typeof gw!=='string');const cfg={...c,gateway:`http://127.0.0.1:${gw.port}`};
 const listener=app(cfg,event=>events.push(event)).listen(0,'127.0.0.1');await new Promise<void>(r=>listener.once('listening',r));const address=listener.address();assert(address&&typeof address!=='string');cfg.resource=`http://127.0.0.1:${address.port}/mcp`;
 const access=await new SignJWT({scope:'computer:read computer:control'}).setProtectedHeader({alg:'HS256'}).setIssuer(cfg.issuer).setSubject('private-owner').setJti('fixture-private-token').setAudience(cfg.resource).setIssuedAt().setExpirationTime('1m').sign(new TextEncoder().encode(cfg.devKey));
 const client=new Client({name:'trace-regression',version:'1.0.0'});
 try{
  const unauth=await fetch(cfg.resource,{method:'POST',headers:{'Content-Type':'application/json','X-OpenCommander-Request-ID':'private-injected-header'},body:JSON.stringify({jsonrpc:'2.0',id:1,method:'initialize',params:{password:'private-body-password'}})});assert.equal(unauth.status,401);assert.match(unauth.headers.get('x-opencommander-request-id')??'',/^[a-f0-9-]{36}$/);
  await client.connect(new StreamableHTTPClientTransport(new URL(cfg.resource),{requestInit:{headers:{Authorization:`Bearer ${access}`}}}));
  const result=await client.callTool({name:'file_write',arguments:{device_id:'fixture-device',idempotency_key:'fixture-idempotency',path:'C:\\private-path',text:'private-file-content'}});assert.equal(result.isError,undefined);
  const record=events.find(e=>e.event==='mcp_tool_result');assert(record);assert.equal(record.task_id,'fixture-task-123');assert.equal(result._meta?.request_id,record.request_id);
  assert(events.some(e=>e.event==='mcp_request'&&e.request_id===record.request_id&&e.authenticated===true));
  const logged=JSON.stringify(events);for(const secret of [access,'private-body-password','private-injected-header','private-file-content','private-path','private-owner',cfg.bridgeKey])assert(!logged.includes(secret));
 }finally{await client.close();await new Promise<void>(r=>listener.close(()=>r()));await new Promise<void>(r=>gateway.close(()=>r()));}
});

test('desktop_screenshot returns a completed image in the original tool call without resubmitting the task',async()=>{
 let submissions=0,reads=0;
 const png='iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl2YwAAAABJRU5ErkJggg==';
 const gateway=createServer((req,res)=>{
  res.setHeader('Content-Type','application/json');
  if(req.method==='POST'&&req.url==='/api/tasks'){submissions++;res.end(JSON.stringify({task_id:'screenshot-fixture',status:'queued'}));}
  else if(req.method==='GET'&&req.url==='/api/tasks/screenshot-fixture'){reads++;res.end(JSON.stringify(reads<3?{task_id:'screenshot-fixture',status:reads===1?'dispatched':'running'}:{task_id:'screenshot-fixture',status:'succeeded',result:{png_base64:png,image_width:1,image_height:1}}));}
  else{res.statusCode=404;res.end('{}');}
 }).listen(0,'127.0.0.1');await new Promise<void>(resolve=>gateway.once('listening',resolve));
 const address=gateway.address();assert(address&&typeof address!=='string');const cfg={...c,gateway:`http://127.0.0.1:${address.port}`};
 const listener=app(cfg).listen(0,'127.0.0.1');await new Promise<void>(resolve=>listener.once('listening',resolve));const mcp=listener.address();assert(mcp&&typeof mcp!=='string');cfg.resource=`http://127.0.0.1:${mcp.port}/mcp`;
 const access=await new SignJWT({scope:'computer:read'}).setProtectedHeader({alg:'HS256'}).setIssuer(cfg.issuer).setSubject('alice').setJti('screenshot-token').setAudience(cfg.resource).setIssuedAt().setExpirationTime('1m').sign(new TextEncoder().encode(cfg.devKey));
 const client=new Client({name:'screenshot-regression',version:'1.0.0'});
 try{
  await client.connect(new StreamableHTTPClientTransport(new URL(cfg.resource),{requestInit:{headers:{Authorization:`Bearer ${access}`}}}));
  const result=await client.callTool({name:'desktop_screenshot',arguments:{device_id:'fixture-device',idempotency_key:'screenshot-request'}});
  assert.equal(result.isError,undefined);assert.equal(result.structuredContent?.status,'succeeded');assert.equal(result.structuredContent?.task_id,'screenshot-fixture');
  assert.deepEqual(result.content[0],{type:'image',data:png,mimeType:'image/png'});assert(!JSON.stringify(result.structuredContent).includes(png));
  assert.equal(submissions,1);assert.equal(reads,3);
 }finally{await client.close();await new Promise<void>(resolve=>listener.close(()=>resolve()));await new Promise<void>(resolve=>gateway.close(()=>resolve()));}
});
