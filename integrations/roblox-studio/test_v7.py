import json,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
import developer_suite_core as core
import developer_suite_v7_intelligence as intelligence
import developer_suite_v7_governance as policy

SOURCE=[
 {"path":"ReplicatedStorage/SaveManager","class":"ModuleScript","source":"local SaveManager={}\nfunction SaveManager.writeFile(path, content) return true end\nreturn SaveManager","name":"SaveManager"},
 {"path":"ReplicatedStorage/Boot","class":"ModuleScript","source":"local Save=require(script.Parent.SaveManager)\nreturn Save","name":"Boot"},
 {"path":"ServerScriptService/Runner","class":"Script","source":"local Runner=require(game.ReplicatedStorage.Boot)\nreturn Runner","name":"Runner"}
]
MANIFEST={"total":len(SOURCE),"scripts":3,"ui":0,"remotes":0,"truncated":False,
 "nodes":SOURCE}

def studio(cmd,timeout=45):
    if cmd["op"]=="suite_manifest":return dict(MANIFEST)
    if cmd["op"]=="assert_state":return {"passed":True,"results":[{"pass":True}]}
    if cmd["op"]=="suite_perf":return {"objects":10,"scripts":3,"memoryMb":35.5}
    raise ValueError(cmd)

class Intelligence(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self._state=core.STATE
        core.STATE=Path(self.temp.name)/"suite"
    def tearDown(self):
        core.STATE=self._state
        self.temp.cleanup()

    def test_semantic_search(self):
        result=intelligence.semantic({"query":"save file to storage"},studio)
        self.assertTrue(result["results"])
        self.assertEqual(result["results"][0]["path"],"ReplicatedStorage/SaveManager")
        self.assertIn("lexical",result["method"].lower())

    def test_semantic_invalid_query(self):
        with self.assertRaises(ValueError):
            intelligence.semantic({"query":""},studio)

    def test_reverse_impact(self):
        result=intelligence.impact({"path":"ReplicatedStorage/SaveManager"},studio)
        self.assertIn("ReplicatedStorage/Boot",result["potentiallyAffected"])
        self.assertIn("ServerScriptService/Runner",result["potentiallyAffected"])

    def test_properties_seed_reproducible(self):
        for kind in intelligence.PROPERTIES:
            args={"property":kind,"cases":100,"seed":24}
            a=intelligence.property_tests(args,studio)
            b=intelligence.property_tests(args,studio)
            self.assertEqual(a["samples"],b["samples"])
            self.assertTrue(a["passed"],kind)

    def test_mutation_detects_reference_regressions(self):
        for kind in intelligence.PROPERTIES:
            result=intelligence.mutation_tests({"property":kind},studio)
            self.assertTrue(result["baselinePassed"])
            self.assertGreaterEqual(result["killed"],20)

    def test_behavior_spec_validated(self):
        result=intelligence.behavior({"action":"define","name":"boot","checks":[
            {"path":"Workspace","exists":True}]},studio)
        self.assertTrue(result["saved"])
        self.assertTrue(intelligence.behavior({"action":"verify","name":"boot"},studio)["passed"])

    def test_runtime_spec_requires_real_test(self):
        intelligence.behavior({"action":"define","name":"runtime","checks":[
            {"scope":"server","path":"ReplicatedStorage/Boot","exists":True}]},studio)
        self.assertTrue(intelligence.behavior({"action":"verify","name":"runtime"},studio)["requires_runtime"])

    def test_coverage_not_fake_branch_metrics(self):
        intelligence.behavior({"action":"define","name":"test","checks":[
            {"path":"ReplicatedStorage/Boot","exists":True}]},studio)
        cov=intelligence.coverage({},studio)
        self.assertEqual(cov["associatedScripts"],1)
        self.assertIn("not branch",cov["metric"])

    def test_trace_scrubs_payloads(self):
        event=intelligence.event_trace({"action":"record","name":"run","events":[
            {"id":"a","kind":"mouse","secret":"do_not_keep"}]},studio)
        self.assertEqual(event["saved"],1)
        grouped=intelligence.trace_inspect({"name":"run"},studio)
        self.assertNotIn("do_not_keep",json.dumps(grouped))

    def test_manifest_deterministic(self):
        first=intelligence.build_manifest({"action":"save"},studio)
        second=intelligence.build_manifest({},studio)
        self.assertEqual(first["manifest"]["digest"],second["manifest"]["digest"])
        self.assertTrue(first["manifest"]["complete"])

    def test_incremental_detects_change(self):
        intelligence.build_manifest({"action":"save"},studio)
        original=MANIFEST["nodes"]
        changed=[dict(x) for x in original]
        changed[0]["source"]+="\nlocal newer=true"
        with patch.dict(MANIFEST,{"nodes":changed}):
            result=intelligence.incremental({"refresh":True},studio)
        self.assertIn("ReplicatedStorage/SaveManager",result["changed"])

    def test_policy_denies_arbitrary_eval(self):
        self.assertEqual(policy.decision({"op":"eval"})["decision"],"deny")
        self.assertEqual(policy.decision({"op":"delete"})["decision"],"require_confirmation")
        self.assertEqual(policy.decision({"op":"delete","confirm_persistent_changes":True})["decision"],"allow")

    def test_batch_destructive_requires_confirmation(self):
        cmd={"op":"batch","actions":[{"op":"delete","path":"Workspace/Unit"}]}
        self.assertEqual(policy.decision(cmd)["decision"],"require_confirmation")
        self.assertEqual(policy.decision({**cmd,"confirm_persistent_changes":True})["decision"],"allow")

    def test_policy_suite_apply_requires_confirmation(self):
        self.assertEqual(policy.decision({"op":"suite_gui","apply":True})["decision"],"require_confirmation")

    def test_audit_chain_detects_tampering(self):
        policy.audit_append({"operation":"read_script","outcome":"approved"})
        policy.audit_append({"operation":"suite_scan","outcome":"completed"})
        self.assertTrue(policy.audit_trail({},studio)["valid"])
        file=core.filename("action_audit",".jsonl")
        raw=file.read_text(encoding="utf-8")
        file.write_text(raw.replace("suite_scan","suite_delete"),encoding="utf-8")
        self.assertFalse(policy.audit_trail({},studio)["valid"])

    def test_vault_current_user_dpapi_roundtrip(self):
        if sys.platform!="win32":self.skipTest("Windows DPAPI only")
        secret=b"fixture-secret-only"
        encrypted=policy._protect_dpapi(secret)
        self.assertNotIn(secret,encrypted)
        self.assertEqual(policy._unprotect_dpapi(encrypted),secret)
        v=policy.vault({"action":"put","name":"test","secret":secret.decode()},studio)
        self.assertTrue(v["stored"])
        self.assertTrue(policy.vault({"name":"test"},studio)["exists"])
        self.assertTrue(policy.vault({"action":"delete","name":"test","confirm_delete":True},studio)["deleted"])

    def test_provenance_requires_rights(self):
        with self.assertRaises(ValueError):policy.asset_provenance({"action":"register","name":"asset"},studio)
        result=policy.asset_provenance({"action":"register","name":"asset","asset_id":32,
             "owner":"test","source_url":"https://example.test/a","license":"CC0"},studio)
        self.assertTrue(result["registered"])

    def test_multi_place_does_not_call_cloud(self):
        x=policy.multi_place({"action":"add","name":"lobby","place_id":"12345"},studio)
        self.assertTrue(x["saved"])

    def test_teleport_retry_budget(self):
        x=policy.teleport({"steps":["request","enqueue","start","error","retry","start","arrive"]},studio)
        self.assertEqual(x["final"],"arrived")
        self.assertEqual(x["retries"],1)

    def test_compatibility_matrix(self):
        policy.compatibility({"action":"set","feature":"shell","platform":"mobile","status":"passed"},studio)
        self.assertEqual(policy.compatibility({},studio)["matrix"]["shell"]["mobile"]["status"],"passed")

    def test_resource_budget(self):
        policy.resource_budget({"action":"set","limits":{"objects":20,"memoryMb":40}},studio)
        self.assertTrue(policy.resource_budget({},studio)["passed"])

    def test_plugin_validator_rejects_arbitrary_code(self):
        with self.assertRaises(ValueError):
            policy.plugin_validate({"manifest":{"name":"danger","version":"1.0.0",
              "operations":[{"alias":"exec","target":"shell"}]}},studio)

    def test_extension_register_and_invoke_guarded(self):
        manifest={"name":"inspector","version":"1.0.0",
          "operations":[{"alias":"search","target":"v7_semantic_search"}]}
        self.assertTrue(policy.plugin_register({"manifest":manifest},studio)["registered"])
        with self.assertRaises(ValueError):
            policy.invoke({"name":"inspector","alias":"search","args":{"query":"save file"}},studio)
        output=policy.invoke({"name":"inspector","alias":"search","approved":True,
          "args":{"query":"save file"}},studio)
        self.assertTrue(output["result"]["results"])

    def test_extension_replacement_requires_consent(self):
        manifest={"name":"inspector","version":"1.0.0",
          "operations":[{"alias":"search","target":"v7_semantic_search"}]}
        policy.plugin_register({"manifest":manifest},studio)
        with self.assertRaises(ValueError):policy.plugin_register({"manifest":manifest},studio)

    def test_registry_discovery(self):
        self.assertIn("extensions",policy.registry({},studio))
        self.assertGreaterEqual(len(intelligence.HANDLERS),10)
        self.assertGreaterEqual(len(policy.HANDLERS),12)

if __name__=="__main__":
    unittest.main()
