import express from 'express';
import {createMcpHandler} from '@modelcontextprotocol/server';
import {toNodeHandler} from '@modelcontextprotocol/node';
import {authenticator} from './auth.js';
import {config,serverVersion,type Config} from './config.js';
import {server} from './tools.js';
import {pathToFileURL} from 'node:url';
import {randomUUID} from 'node:crypto';
import {resolve} from 'node:path';

export function app(c:Config,record:(event:Record<string,unknown>)=>void=event=>console.log(JSON.stringify(event))){
 const a=express();a.disable('x-powered-by');
 a.use((req,res,next)=>{
  const requestId=randomUUID();const started=Date.now();res.locals.requestId=requestId;res.setHeader('X-OpenCommander-Request-ID',requestId);
  res.once('finish',()=>{if(req.path==='/mcp')record({event:'mcp_request',at:new Date().toISOString(),request_id:requestId,http_method:req.method,rpc_method:['initialize','tools/list','tools/call','ping','notifications/initialized'].includes(req.body?.method)?req.body.method:'other',authenticated:res.locals.authenticated===true,status:res.statusCode,duration_ms:Date.now()-started});});next();
 });
 a.use(express.json({limit:'2mb'}));
 const authenticate=authenticator(c);
 a.use((_req,res,next)=>{res.setHeader('Cache-Control','no-store');res.setHeader('X-Content-Type-Options','nosniff');next();});
 a.get('/healthz',(_req,res)=>res.json({status:'ok',version:serverVersion}));
 a.use('/releases/windows',express.static(resolve(process.env.OPENCOMMANDER_RELEASE_DIR??'releases/windows'),{index:false,dotfiles:'deny',redirect:false,setHeaders:res=>{res.setHeader('Cache-Control','no-store');res.setHeader('X-Content-Type-Options','nosniff');}}));
 a.get(['/.well-known/oauth-protected-resource','/.well-known/oauth-protected-resource/mcp'],(_req,res)=>res.json({resource:c.resource,authorization_servers:[c.issuer],scopes_supported:['computer:read','computer:control'],bearer_methods_supported:['header']}));
 a.post('/mcp',async(req,res)=>{
  const origin=req.headers.origin;const host=req.headers.host;
  if(host!==new URL(c.resource).host|| (origin&&origin!==new URL(c.resource).origin)){res.status(403).json({error:'Origin or Host rejected'});return;}
  let id;
  try{id=await authenticate(req.headers.authorization);}catch{
   res.setHeader('WWW-Authenticate',`Bearer resource_metadata="${new URL('/.well-known/oauth-protected-resource',c.resource)}", scope="computer:read"`);
   res.status(401).json({error:'Authentication required'});return;
  }
  const authenticated=id;res.locals.authenticated=true;
  const handler=createMcpHandler(()=>server(c,authenticated,{requestId:String(res.locals.requestId),record}));
  try{await toNodeHandler(handler)(req,res,req.body);}catch{if(!res.headersSent)res.status(500).json({error:'MCP request failed'});}
 });
 a.all('/mcp',(_req,res)=>res.status(405).setHeader('Allow','POST').json({error:'Stateless MCP endpoint accepts POST'}));
 a.use((_err:unknown,_req:express.Request,res:express.Response,next:express.NextFunction)=>{if(res.headersSent){next(_err);return}res.status(400).json({error:{code:'INVALID_REQUEST',message:'Malformed or oversized request'}});});
 return a;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){const c=config();const listener=app(c).listen(c.port,c.host,()=>console.log(JSON.stringify({event:'mcp_listening',port:c.port})));for(const signal of ['SIGINT','SIGTERM'] as const)process.on(signal,()=>{listener.close(()=>process.exit(0));setTimeout(()=>process.exit(1),5000).unref();});}
