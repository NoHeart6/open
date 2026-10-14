import {createRemoteJWKSet,jwtVerify,SignJWT} from 'jose';
import type {Config} from './config.js';
export interface Identity {subject:string; scope:string; jti:string; expires:number}
export function authenticator(c:Config){
 const key=c.dev ? new TextEncoder().encode(c.devKey) : createRemoteJWKSet(new URL(c.jwksUrl));
 return async (header:string|undefined):Promise<Identity>=>{
  if(!header?.startsWith('Bearer '))throw new Error('Bearer token required');
  const {payload}=await jwtVerify(header.slice(7),key,{issuer:c.issuer,audience:c.resource,algorithms:c.dev?['HS256']:['RS256','ES256','EdDSA'],requiredClaims:['exp','iat','sub','jti']});
  if(!payload.sub||payload.sub.length>256||/[\u0000-\u001f\u007f]/.test(payload.sub)||typeof payload.scope!=='string'||!payload.jti||!payload.scope.split(' ').includes('computer:read'))throw new Error('Token subject, identifier or scope invalid');
  if(typeof payload.exp!=='number'||typeof payload.iat!=='number'||payload.exp-payload.iat>3600)throw new Error('Access token lifetime must not exceed one hour');
  return {subject:payload.sub,scope:payload.scope,jti:payload.jti,expires:payload.exp};
 };
}
export async function assertion(c:Config,id:Identity):Promise<string>{
 return new SignJWT({scope:id.scope,token_jti:id.jti,token_expires:id.expires}).setSubject(id.subject).setIssuer('opencommander-bridge').setAudience('opencommander-gateway').setIssuedAt().setExpirationTime('15s').setProtectedHeader({alg:'HS256'}).sign(new TextEncoder().encode(c.bridgeKey));
}
