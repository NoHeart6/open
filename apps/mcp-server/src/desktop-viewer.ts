export const desktopViewerUri='ui://opencommander/desktop-v1.html';
export const desktopViewerHtml=String.raw`<!doctype html>
<html lang="id"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Screenshot OpenCommander</title>
<style>body{margin:0;padding:12px;font:14px system-ui;background:transparent;color:inherit}img{display:block;max-width:100%;height:auto;border:1px solid #888;border-radius:4px}p{margin:0 0 10px}a{display:inline-block;margin-top:10px;color:inherit}#image[hidden],a[hidden]{display:none}</style></head>
<body><p id="status" role="status">Menunggu screenshot…</p><img id="image" alt="Screenshot desktop Windows" hidden><a id="download" download="OpenCommander-screenshot.png" hidden>Simpan screenshot</a>
<script>
let sequence=0;const pending=new Map();let polling=false;
function request(method,params){return new Promise((resolve,reject)=>{const id=++sequence;const timeout=setTimeout(()=>{pending.delete(id);reject(new Error('Host tidak merespons'));},10000);pending.set(id,{resolve,reject,timeout});window.parent.postMessage({jsonrpc:'2.0',id,method,params},'*');});}
function show(response){
 const data=response?.structuredContent||{};const image=response?._meta?.['opencommander/image']||response?.content?.find(item=>item.type==='image');
 if(image&&['image/png','image/jpeg'].includes(image.mimeType)&&typeof image.data==='string'&&/^[A-Za-z0-9+/]+={0,2}$/.test(image.data)){
  const url='data:'+image.mimeType+';base64,'+image.data;const element=document.getElementById('image');element.onerror=()=>{document.getElementById('status').textContent='Screenshot tidak bisa dimuat oleh browser.';};element.src=url;element.hidden=false;
  const link=document.getElementById('download');link.href=url;link.download='OpenCommander-screenshot.'+(image.mimeType==='image/jpeg'?'jpg':'png');link.hidden=false;
  const size=data.result?.screenshot||data.result||{};document.getElementById('status').textContent='Screenshot '+(size.image_width||'')+' × '+(size.image_height||'')+' — '+(data.result?.verification_state||'hasil tangkapan layar');return;
 }
 document.getElementById('status').textContent=data.status==='succeeded'?'Task selesai tanpa screenshot. Pastikan izin screenshot aktif dan capture_after tidak dimatikan.':['failed','cancelled','expired','uncertain'].includes(data.status)?'Screenshot tidak tersedia: '+(data.error||data.status):'Menunggu hasil tangkapan layar…';
 if(data.task_id&&!polling&&!['failed','cancelled','expired','uncertain','succeeded'].includes(data.status))poll(data.task_id);
}
async function poll(task_id){polling=true;try{for(let count=0;count<60;count++){await new Promise(resolve=>setTimeout(resolve,750));const response=window.openai?.callTool?await window.openai.callTool('desktop_view',{task_id}):await request('tools/call',{name:'desktop_view',arguments:{task_id}});show(response);if(['succeeded','failed','cancelled','expired','uncertain'].includes(response?.structuredContent?.status))break;}}catch(error){document.getElementById('status').textContent=String(error.message||error);}finally{polling=false;}}
window.addEventListener('message',event=>{if(event.source!==window.parent)return;const message=event.data;if(message?.jsonrpc!=='2.0')return;if(pending.has(message.id)){const item=pending.get(message.id);pending.delete(message.id);clearTimeout(item.timeout);message.error?item.reject(new Error(message.error.message)):item.resolve(message.result);}else if(message.method==='ui/notifications/tool-result')show(message.params);});
function legacy(){const metadata=window.openai?.toolResponseMetadata;const envelope=metadata?.mcp_tool_result||metadata?.call_tool_result;if(envelope)show(envelope.result||envelope);else if(window.openai?.toolOutput)show({structuredContent:window.openai.toolOutput,_meta:metadata});}
window.addEventListener('openai:set_globals',legacy);legacy();
request('ui/initialize',{appInfo:{name:'OpenCommander screenshot',version:'1.0.0'},appCapabilities:{},protocolVersion:'2026-01-26'}).then(()=>window.parent.postMessage({jsonrpc:'2.0',method:'ui/notifications/initialized'},'*')).catch(()=>{});
</script></body></html>`;

export function desktopResult(result:Record<string,unknown>,viewer=false){
 const output=result.result as Record<string,unknown>|undefined;
 const nested=output?.screenshot as Record<string,unknown>|undefined;
 const image=typeof output?.png_base64==='string'||typeof output?.jpeg_base64==='string'?output:typeof nested?.png_base64==='string'||typeof nested?.jpeg_base64==='string'?nested:undefined;
 if(!image)return {content:[{type:'text' as const,text:JSON.stringify(result)}],structuredContent:result};
 const {png_base64,jpeg_base64,jpeg_image_width,jpeg_image_height,...metadata}=image;
 const jpeg=typeof jpeg_base64==='string'&&jpeg_base64.length<=133336?jpeg_base64:undefined;
 if(jpeg){metadata.image_width=jpeg_image_width;metadata.image_height=jpeg_image_height;}
 const compact={...result,result:image===output?metadata:{...output,screenshot:metadata}};
 const structuredContent=jpeg?{...compact,result:{...compact.result,image_data:{mime_type:'image/jpeg',base64:jpeg}}}:compact;
 const block={type:'image' as const,data:jpeg??png_base64 as string,mimeType:jpeg?'image/jpeg':'image/png'};
 return {content:[block,{type:'text' as const,text:JSON.stringify(compact)}],structuredContent,...viewer?{_meta:{'opencommander/image':block}}:{}};
}
