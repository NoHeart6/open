import {readFileSync} from 'node:fs';
export const serverVersion=String((JSON.parse(readFileSync(new URL('../package.json',import.meta.url),'utf8')) as {version:string}).version);
export interface Config {
 port:number; host:string; resource:string; issuer:string; jwksUrl:string;
 gateway:string; bridgeKey:string; devKey?:string; dev:boolean;
}
export function config(env:NodeJS.ProcessEnv=process.env):Config {
 const dev=env.OPENCOMMANDER_DEV==='1';
 const bridgeKey=env.BRIDGE_KEY_FILE?readFileSync(env.BRIDGE_KEY_FILE,'utf8').trim():env.BRIDGE_KEY??'';
 const c:Config={port:Number(env.MCP_PORT??3000),host:env.MCP_HOST??'127.0.0.1',resource:env.MCP_RESOURCE??'',issuer:env.OIDC_ISSUER??'',jwksUrl:env.OIDC_JWKS_URL??'',gateway:env.GATEWAY_URL??'http://127.0.0.1:8080',bridgeKey,devKey:env.DEV_TOKEN_KEY,dev};
 if(c.bridgeKey.length<32||!c.resource||!c.issuer||(!dev&&!c.jwksUrl))throw new Error('Missing secure authentication configuration');
 const resource=new URL(c.resource);const issuer=new URL(c.issuer);const gateway=new URL(c.gateway);
 if(resource.username||resource.password||resource.hash||resource.search)throw new Error('Invalid MCP resource URL');
 if(dev){
  if(c.host!=='127.0.0.1'||![resource.hostname,issuer.hostname,gateway.hostname].every(h=>h==='127.0.0.1')||!c.devKey||c.devKey.length<32)throw new Error('Development mode is restricted to loopback with a separate strong token key');
 }else{
  if(resource.protocol!=='https:'||issuer.protocol!=='https:'||new URL(c.jwksUrl).protocol!=='https:')throw new Error('Production OAuth requires HTTPS');
  const railwayPrivate=/^[a-z0-9][a-z0-9-]*\.railway\.internal$/.test(gateway.hostname);
  if(gateway.protocol!=='https:'&&!['127.0.0.1','gateway'].includes(gateway.hostname)&&!railwayPrivate)throw new Error('Internal HTTP gateway must use loopback, Compose or Railway private networking');
 }
 return c;
}
