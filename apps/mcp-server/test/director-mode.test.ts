import {test} from 'node:test';
import assert from 'node:assert/strict';
import {actions} from '../src/tools.js';
const root='C:\\Users\\Administrator\\Pictures\\project\\MangaMotion';
test('Director Studio professional mode is recognized by MCP',()=>{
 const inspect=actions.motiongraphics.schema.safeParse({project_root:root,op:'inspect',project:'director-showcase.json',composition:'DirectorStudio'});
 const change=actions.motiongraphics.schema.safeParse({project_root:root,op:'set_text',project:'director-showcase.json',composition:'DirectorStudio',output:'director-updated.json',target:'finale_cta',text:'MOTION DESIGN FOR ALL'});
 const still=actions.motiongraphics.schema.safeParse({project_root:root,op:'render_still',project:'director-showcase.json',composition:'DirectorStudio',output:'director-preview.png',frame:340});
 const video=actions.motiongraphics.schema.safeParse({project_root:root,op:'render_video',project:'director-showcase.json',composition:'DirectorStudio',output:'director-final.mp4'});
 for(const r of [inspect,change,still,video])assert.equal(r.success,true);
});
test('Director Studio rejects arbitrary execution tool calls',()=>{
 const invalid=actions.motiongraphics.schema.safeParse({project_root:root,op:'eval',composition:'DirectorStudio',javascript:'process.exit()'});
 assert.equal(invalid.success,false);
});
