import {test} from 'node:test';
import assert from 'node:assert/strict';
import {desktopResult,desktopViewerUri} from '../src/desktop-viewer.js';

test('screenshot results supply native image plus private viewer data without polluting structured observations',()=>{
 const png=Buffer.alloc(700000,123).toString('base64');
 for(const nested of [false,true]){
  const image={png_base64:png,image_width:1280,image_height:800};
  const raw={task_id:'fixture-screenshot-task',status:'succeeded',result:nested?{operation:'click',verified:false,verification_state:'sent',screenshot:image}:image};
  const response=desktopResult(raw,true);
  assert.equal(response.content[0]!.type,'image');
  assert.equal(response._meta?.['opencommander/image'].data,png);
  assert(!JSON.stringify(response.structuredContent).includes(png));
  assert(JSON.stringify(response).length<2*1024*1024);
  assert.equal(raw.result,nested?raw.result:image);
 }
 assert.match(desktopViewerUri,/^ui:\/\//);
 const pending=desktopResult({task_id:'queued-task',status:'queued'},true);assert.equal(pending.content[0]!.type,'text');
});

test('JPEG survives structured-only tool clients while text and viewer results stay within budget',()=>{
 const jpeg=Buffer.alloc(100000,123).toString('base64');const png=Buffer.alloc(500000,123).toString('base64');
 for(const nested of [false,true]){
  const image={png_base64:png,jpeg_base64:jpeg,jpeg_image_width:960,jpeg_image_height:600,image_width:1280,image_height:800};
  const response=desktopResult({status:'succeeded',task_id:'jpeg-task',result:nested?{screenshot:image,operation:'click'}:image},true);
  assert.deepEqual(response.structuredContent.result.image_data,{mime_type:'image/jpeg',base64:jpeg});
  assert.deepEqual(response.content[0],{type:'image',mimeType:'image/jpeg',data:jpeg});
  const text=response.content.find(block=>block.type==='text');assert(text&&text.type==='text');assert(!text.text.includes(jpeg));
  assert(!JSON.stringify(response).includes(png));assert(JSON.stringify(response).length<500000);
  const metadata=nested?(response.structuredContent.result as {screenshot:{image_width:number}}).screenshot:response.structuredContent.result;
  assert.equal(metadata.image_width,960);
 }
});
