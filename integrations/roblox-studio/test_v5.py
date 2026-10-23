"""Local regression coverage for saved places and guarded repair cycles."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import bridge

class PlaceBackupTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.root=Path(self.tmp.name)
        self.patched=patch.object(bridge,"PLACE_BACKUPS",self.root/"backups")
        self.patched.start()
        self.path=self.root/"example.rbxl"
        self.path.write_bytes(b"RBXL backup test fixture")

    def tearDown(self):
        self.patched.stop()
        self.tmp.cleanup()

    def test_backup_and_recover_never_overwrites_source(self):
        with patch.object(bridge,"validate_place_path",return_value=self.path):
            item=bridge.backup_place_file(str(self.path))
        original=self.path.read_bytes()
        self.path.write_bytes(b"changed place")
        recovered=bridge.restore_place_copy(item["id"])
        self.assertEqual(Path(recovered["recovered_path"]).read_bytes(),original)
        self.assertEqual(self.path.read_bytes(),b"changed place")

    def test_tampered_backup_rejected(self):
        with patch.object(bridge,"validate_place_path",return_value=self.path):
            item=bridge.backup_place_file(str(self.path))
        (bridge.PLACE_BACKUPS/(item["id"]+".rbxl")).write_bytes(b"corrupt")
        with self.assertRaises(ValueError):
            bridge.restore_place_copy(item["id"])

    def test_recovery_does_not_overwrite_existing_file(self):
        with patch.object(bridge,"validate_place_path",return_value=self.path):
            item=bridge.backup_place_file(str(self.path))
        bridge.restore_place_copy(item["id"])
        with self.assertRaises(FileExistsError):
            bridge.restore_place_copy(item["id"])

    def test_bad_backup_id_rejected(self):
        with self.assertRaises(ValueError):
            bridge.restore_place_copy("../secret")

class RepairTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.patched=patch.object(bridge,"SNAPSHOTS",Path(self.tmp.name))
        self.patched.start()
        self.current="return 1"
        self.ops=[]

    def tearDown(self):
        self.patched.stop()
        self.tmp.cleanup()

    def simulated_studio(self,cmd,timeout=45):
        self.ops.append(cmd["op"])
        if cmd["op"]=="read_script":
            return {"path":"ReplicatedStorage/Unit","source":self.current,"class":"ModuleScript"}
        if cmd["op"]=="set_script":
            if self.current!=cmd["expected_source"]:raise RuntimeError("unexpected CAS")
            self.current=cmd["source"]
            return {"path":cmd["path"]}
        if cmd["op"]=="playtest_start":return {"id":"test-unit"}
        if cmd["op"]=="playtest_status":
            return {"state":"done","result":{"success":self.should_pass,"players":1}}
        raise AssertionError(cmd)

    def command(self):
        return {"op":"repair_verify","path":"ReplicatedStorage/Unit",
                "source":"return 2","expected_sha256":bridge.source_hash("return 1"),
                "confirm_persistent_changes":True,"checks":[{"scope":"server",
                "path":"ReplicatedStorage/Unit","property":"Name","equals":"Unit"}]}

    def test_approved_fix_retained(self):
        self.should_pass=True
        with patch.object(bridge,"studio_command",side_effect=self.simulated_studio):
            result=bridge.repair_verify(self.command())
        self.assertEqual(result["state"],"verified")
        self.assertEqual(self.current,"return 2")

    def test_failed_runtime_assertion_rolls_back(self):
        self.should_pass=False
        with patch.object(bridge,"studio_command",side_effect=self.simulated_studio):
            result=bridge.repair_verify(self.command())
        self.assertEqual(result["state"],"rolled_back")
        self.assertEqual(self.current,"return 1")
        self.assertGreaterEqual(len(list(bridge.SNAPSHOTS.glob("*.json"))),2)

    def test_stale_source_rejected_without_edit(self):
        c=self.command()
        c["expected_sha256"]=bridge.source_hash("return 99")
        with patch.object(bridge,"studio_command",side_effect=self.simulated_studio):
            with self.assertRaises(ValueError):
                bridge.repair_verify(c)
        self.assertEqual(self.current,"return 1")

    def test_test_confirmation_mandatory(self):
        c=self.command()
        c["confirm_persistent_changes"]=False
        with self.assertRaises(ValueError):
            bridge.repair_verify(c)

if __name__=="__main__":
    unittest.main()
