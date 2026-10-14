import {generateKeyPairSync,createPrivateKey,createPublicKey,sign,verify} from 'node:crypto';
if(process.argv[2]==='generate'){
 const {privateKey,publicKey}=generateKeyPairSync('ed25519');
 const pub=publicKey.export({format:'der',type:'spki'});
 process.stdout.write(JSON.stringify({private_key_base64:privateKey.export({format:'der',type:'pkcs8'}).toString('base64'),public_key_hex:pub.subarray(pub.length-32).toString('hex')}));
}else{
 let input='';for await(const chunk of process.stdin)input+=chunk;
 const {private_key_base64,payload}=JSON.parse(input);
 const key=createPrivateKey({key:Buffer.from(private_key_base64,'base64'),format:'der',type:'pkcs8'});
 const bytes=Buffer.from(JSON.stringify(payload));const signature=sign(null,bytes,key);
 if(!verify(null,bytes,createPublicKey(key),signature))throw new Error('Release signing self-check failed');
 process.stdout.write(JSON.stringify({payload:bytes.toString('base64'),signature:signature.toString('base64')}));
}
