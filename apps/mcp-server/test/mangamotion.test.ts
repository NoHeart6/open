import {test} from 'node:test';
import assert from 'node:assert/strict';
import {actions} from '../src/tools.js';

const root='C:\\Users\\Administrator\\Pictures\\project\\MangaMotion';

test('MangaMotion MCP exposes typed inspect and patch without shell injection',()=>{
 const inspect=actions.mangamotion.schema.safeParse({project_root:root,op:'inspect',project:'raster-character-demo.json'});
 assert.equal(inspect.success,true);
 const patch=actions.mangamotion.schema.safeParse({project_root:root,op:'patch',project:'raster-character-demo.json',
  output:'new-project.json',changes:[{target:'head',property:'rotation',frame:70,value:12}]});
 assert.equal(patch.success,true);
 const shell=actions.mangamotion.schema.safeParse({project_root:root,op:'terminal_execute',command:'echo nope'});
 assert.equal(shell.success,false);
});
test('MangaMotion MCP rejects arbitrary track properties and malformed payloads',()=>{
 assert.equal(actions.mangamotion.schema.safeParse({project_root:root,op:'patch',
  changes:[{target:'head',property:'eval',frame:70,value:12}]}).success,false);
 assert.equal(actions.mangamotion.schema.safeParse({project_root:root,op:'patch',
  changes:[{target:'head',property:'rotation',frame:-1,value:12}]}).success,false);
 assert.equal(actions.mangamotion.schema.safeParse({project_root:root,op:'patch',
  changes:[{target:'head',property:'rotation',frame:70,value:Number.POSITIVE_INFINITY}]}).success,false);
});
