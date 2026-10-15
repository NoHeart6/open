import {test} from 'node:test';
import assert from 'node:assert/strict';
import {actions} from '../src/tools.js';
const root='C:\\Users\\Administrator\\Pictures\\project\\MangaMotion';
test('CreativeStudio can be rendered through the same motiongraphics MCP tool',()=>{
 const inspect=actions.motiongraphics.schema.safeParse({
   project_root:root,op:'inspect',project:'creative-showcase.json',composition:'CreativeStudio'
 });
 assert.equal(inspect.success,true);
 const render=actions.motiongraphics.schema.safeParse({
   project_root:root,op:'render_video',composition:'CreativeStudio',
   project:'creative-showcase.json',output:'my-creative.mp4'
 });
 assert.equal(render.success,true);
 const edit=actions.motiongraphics.schema.safeParse({
   project_root:root,op:'set_text',composition:'CreativeStudio',
   project:'creative-showcase.json',output:'creative-edited.json',
   target:'finale_footer',text:'Make it move.'
 });
 assert.equal(edit.success,true);
});
test('Reject arbitrary tool operations but retain code-authored Remotion scenes',()=>{
 assert.equal(actions.motiongraphics.schema.safeParse({
   project_root:root,op:'eval_javascript',composition:'CreativeStudio',javascript:'alert(1)'
 }).success,false);
});
