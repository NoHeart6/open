import {test} from 'node:test';
import assert from 'node:assert/strict';
import {actions} from '../src/tools.js';

test('MangaMotion MCP registers PSD layered-rig import',()=>{
 const root='C:\\Users\\Administrator\\Pictures\\project\\MangaMotion';
 const good=actions.mangamotion.schema.safeParse({
   project_root:root,op:'import_psd',source_file:'character-layers.psd',new_name:'character-rig'
 });
 assert.equal(good.success,true);
 const unknown=actions.mangamotion.schema.safeParse({
   project_root:root,op:'run_local_ai',source_file:'character-layers.psd'
 });
 assert.equal(unknown.success,false);
});
