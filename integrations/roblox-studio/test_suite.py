import sys, tempfile, unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
import developer_suite_core as core
import developer_suite_workflows as wf

MANIFEST={"total":5,"scripts":2,"remotes":1,"ui":1,"truncated":False,"sourceBytes":130,
 "nodes":[
 {"path":"ReplicatedStorage/Core","name":"Core","class":"ModuleScript",
  "source":"return 1\nwhile true do task.wait(1) end"},
 {"path":"ReplicatedStorage/Caller","name":"Caller","class":"ModuleScript",
  "source":"local x=require(script.Parent.Core)\nreturn x"},
 {"path":"ReplicatedStorage/Notify","name":"Notify","class":"RemoteEvent"},
 {"path":"StarterGui/GUI/Button","name":"Button","class":"TextButton",
  "sizeX":30,"sizeY":20,"text":"Start","textSize":12},
 {"path":"Workspace/Baseplate","name":"Baseplate","class":"Part"}]}

def studio(command,timeout=45):
    if command["op"]=="suite_manifest":return MANIFEST.copy()
    if command["op"]=="suite_perf":
        return {"objects":20,"scripts":2,"parts":1,"ui":1,"memoryMb":23.5,"running":False}
    if command["op"]=="batch":return {"count":len(command["actions"])}
    if command["op"]=="playtest_start":return {"id":"testing"}
    if command["op"]=="playtest_status":return {"state":"done","result":{"success":True}}
    if command["op"]=="logs":return {"logs":[]}
    return {"op":command["op"],"ok":True}

class SuiteTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.old=core.STATE
        core.STATE=Path(self.tmp.name)/"suite"
    def tearDown(self):
        core.STATE=self.old
        self.tmp.cleanup()
    def test_inventory(self):
        info=core.scan({},studio)
        self.assertEqual(info["scripts"],2)
        self.assertEqual(info["remotes"],1)
    def test_graph(self):
        dep=core.deps({},studio)
        self.assertEqual(dep["resolved"],1)
        self.assertFalse(dep["cycles"])
    def test_static(self):
        core.scan({},studio)
        rules=[x["rule"] for x in core.lint({},studio)["findings"]]
        self.assertIn("busy-loop",rules)
    def test_security_audit(self):
        core.scan({},studio)
        self.assertEqual(len(core.security({},studio)["remotes"]),1)
    def test_accessibility(self):
        core.scan({},studio)
        self.assertEqual(len(core.accessibility({},studio)["issues"]),2)
    def test_memory(self):
        core.memory({"action":"put","key":"kernel","value":{"name":"Linux"}},studio)
        self.assertEqual(core.memory({"key":"kernel"},studio)["value"]["value"]["name"],"Linux")
    def test_data_store_isolated_and_migrated(self):
        core.sandbox({"action":"put","namespace":"test","key":"old","value":{"files":[]}},studio)
        r=core.sandbox({"action":"migrate","namespace":"test","version":2,"renames":{"old":"new"}},studio)
        self.assertEqual(r["schema_version"],2)
        self.assertIsNotNone(core.sandbox({"namespace":"test","key":"new"},studio)["values"])
        with self.assertRaises(ValueError):
            core.sandbox({"namespace":"production","action":"put","key":"x","value":1},studio)
    def test_economy_double_spend(self):
        r=core.economy({"initial":{"a":50},"events":[
         {"txid":"1","kind":"transfer","from":"a","to":"b","amount":20},
         {"txid":"1","kind":"transfer","from":"a","to":"b","amount":20},
         {"txid":"2","kind":"transfer","from":"a","to":"b","amount":999}]},studio)
        self.assertEqual(r["balances"],{"a":30,"b":20})
        self.assertEqual(len(r["rejected"]),2)
    def test_contracts_breaking_change(self):
        core.contracts({"action":"define","name":"api","fields":{"id":{"type":"number"}}},studio)
        c=core.contracts({"action":"define","name":"api","fields":{"id":{"type":"string"}}},studio)
        self.assertFalse(c["saved"])
        val=core.contracts({"action":"validate","fields":{"id":{"type":"number","required":True}},"data":{"id":"a"}},studio)
        self.assertFalse(val["passed"])
    def test_npc_states(self):
        x=core.npc({"states":["idle","run","sleep"],"start":"idle",
            "transitions":[{"from":"idle","event":"go","to":"run"}],
            "events":["go"]},studio)
        self.assertEqual(x["state"],"run")
        self.assertEqual(x["unreachable"],["sleep"])
    def test_planner_dependencies(self):
        core.planner({"action":"add","id":"a"},studio)
        core.planner({"action":"add","id":"b","depends_on":["a"]},studio)
        with self.assertRaises(ValueError):
            core.planner({"action":"complete","id":"b"},studio)
        core.planner({"action":"complete","id":"a"},studio)
        self.assertIn("b",core.planner({},studio)["ready"])
    def test_assets_and_rights(self):
        core.assets({"action":"add","name":"x","asset_id":123},studio)
        with self.assertRaises(ValueError):
            core.assets({"action":"insert","name":"x","apply":True,"confirm_persistent_changes":True},studio)
    def test_timeline(self):
        output=core.timeline({"duration":10,"tracks":[{"kind":"audio","at":3}]},studio)
        self.assertEqual(output["plan"]["duration"],10)
    def test_gui_is_real_plan_but_guarded(self):
        cmd={"name":"Terminal","elements":[{"class":"TextButton","name":"Run","text":"Execute"}]}
        dry=wf.gui(cmd,studio)
        self.assertEqual(dry["count"],2)
        with self.assertRaises(ValueError):
            wf.gui({**cmd,"apply":True},studio)
        applied=wf.gui({**cmd,"apply":True,"confirm_persistent_changes":True},studio)
        self.assertEqual(applied["created"],2)
    def test_world_determinism(self):
        cmd={"name":"Town","count":5,"seed":17}
        self.assertEqual(wf.world(cmd,studio)["preview"],wf.world(cmd,studio)["preview"])
    def test_theme(self):
        wf.theme({"action":"define","name":"dark","colors":{"background":[10,11,12]}},studio)
        x=wf.theme({"action":"apply","name":"dark","assignments":[
         {"path":"StarterGui/T/Label","property":"TextColor3","token":"background"}],
         "apply":True,"confirm_persistent_changes":True},studio)
        self.assertTrue(x["response"])
    def test_scenario_passes_checks(self):
        cases=[{"mode":"play","duration":5,"checks":[{"path":"Workspace","scope":"server","exists":True}]}]
        result=wf.scenario({"cases":cases,"apply":True,"confirm_persistent_changes":True},studio)
        self.assertTrue(result["passed"])
    def test_perf_baseline(self):
        wf.performance({"action":"baseline"},studio)
        self.assertEqual(wf.performance({},studio)["deltaFromBaseline"]["memoryMb"],0)
    def test_network_not_falsely_applied(self):
        x=wf.network({},studio)
        self.assertFalse(x["automatically_applied"])
        with self.assertRaises(ValueError):wf.network({"apply":True},studio)
    def test_release_gate_not_publish(self):
        self.assertFalse(wf.release({"approve_release":True},studio)["published"])
    def test_dashboard_and_doc(self):
        core.scan({},studio)
        self.assertTrue(Path(wf.dashboard({},studio)["path"]).is_file())
        self.assertTrue(Path(core.docs({},studio)["file"]).is_file())
    def test_git_only_read(self):
        result=core.git({},studio)
        self.assertIn("branch",result)
    def test_export(self):
        core.scan({},studio)
        x=wf.export({},studio)
        self.assertEqual(x["count"],2)
        self.assertTrue(all(item["status"]=="exported" for item in x["files"]))
    def test_visual_diff(self):
        from PIL import Image
        home=Path(self.tmp.name)/"home"
        (home/"Documents").mkdir(parents=True)
        before=home/"Documents"/"before.png";after=home/"Documents"/"after.png"
        Image.new("RGB",(30,30),(0,0,0)).save(before)
        Image.new("RGB",(30,30),(255,255,255)).save(after)
        with patch.object(Path,"home",return_value=home):
            result=wf.visual({"before":str(before),"after":str(after)},studio)
        self.assertFalse(result["passed"])
        self.assertEqual(result["pixels_changed"],900)
    def test_lint_does_not_mutate_scripts(self):
        core.scan({},studio)
        original=core.get("inventory")
        core.lint({},studio)
        self.assertEqual(original,core.get("inventory"))
    def test_suite_registry(self):
        self.assertEqual(len(set(core.HANDLERS)|set(wf.HANDLERS)),26)

if __name__=="__main__":
    unittest.main()
