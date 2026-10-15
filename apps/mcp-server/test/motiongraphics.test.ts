import {test} from 'node:test';
import assert from 'node:assert/strict';
import {actions} from '../src/tools.js';
const root='C:\\Users\\Administrator\\Pictures\\project\\MangaMotion';
test('Motion Graphics MCP exposes clean graphic motion operations',()=>{
 const a=actions.motiongraphics.schema.safeParse({project_root:root,op:'compile_motion',
  project:'graphics-demo.json',output:'new-graphics.json',
  motion_plan:{schema_version:'1.0',source:'cloud_ai',prompt:'Reveal title, stagger cards',actions:[
   {type:'title_reveal',target:'hero',start_frame:5,end_frame:68},
   {type:'stagger_cards',target:'cards',start_frame:74,end_frame:155,stagger_frames:10}
  ]}});
 assert.equal(a.success,true);
});
test('text edits, image import, graphic patch and render are accepted',()=>{
 const actions1=[
  {project_root:root,op:'set_text',project:'graphics-demo.json',output:'revised.json',target:'hero',text:'MOTION GRAPHICS'},
  {project_root:root,op:'import_image',source_file:'logo.png',new_name:'logo_demo'},
  {project_root:root,op:'render_still',project:'graphics-demo.json',frame:40,output:'preview.png'},
  {project_root:root,op:'patch',project:'graphics-demo.json',output:'patched.json',changes:[{target:'hero',property:'font_size',frame:70,value:112}]}
 ];
 for(const a of actions1)assert.equal(actions.motiongraphics.schema.safeParse(a).success,true);
});
test('anime operations and action vocabulary are not registered',()=>{
 assert.equal('mangamotion' in actions,false);
 assert.equal(actions.motiongraphics.schema.safeParse({project_root:root,op:'import_psd',source_file:'layers.psd'}).success,false);
 assert.equal(actions.motiongraphics.schema.safeParse({project_root:root,op:'compile_motion',
  motion_plan:{schema_version:'1.0',source:'cloud_ai',prompt:'wrong',actions:[
   {type:'wave',target:'hero',start_frame:0,end_frame:90}
  ]}}).success,false);
});
