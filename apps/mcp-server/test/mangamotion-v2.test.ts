import {test} from 'node:test';
import assert from 'node:assert/strict';
import {actions} from '../src/tools.js';

const root='C:\\Users\\Administrator\\Pictures\\project\\MangaMotion';

test('MangaMotion MCP accepts a structured cloud AI motion plan',()=>{
 const result=actions.mangamotion.schema.safeParse({
  project_root:root,op:'compile_motion',project:'raster-character-demo.json',
  output:'compiled-proposal.json',motion_plan:{
   schema_version:'0.1',source:'cloud_ai',prompt:'Gerakkan kepala dan rambut',
   actions:[{type:'turn_head',start_frame:15,end_frame:100,intensity:0.8},
            {type:'hair_sway',start_frame:0,end_frame:149,repetitions:2}]
  }
 });
 assert.equal(result.success,true);
});

test('MangaMotion MCP rejects unbounded model plans and unsafe actions',()=>{
 const bad=actions.mangamotion.schema.safeParse({
  project_root:root,op:'compile_motion',
  motion_plan:{schema_version:'0.1',source:'local_ai',prompt:'ignored',actions:[
   {type:'execute_script',start_frame:0,end_frame:60}]}
 });
 assert.equal(bad.success,false);
});

test('MangaMotion MCP supports source files for incoming user images',()=>{
 assert.equal(actions.mangamotion.schema.safeParse({
  project_root:root,op:'import_image',source_file:'character.png',new_name:'my-avatar'
 }).success,true);
});
