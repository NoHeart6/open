"""Offline regression tests for Roblox Studio bridge local snapshots."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import bridge


class BackupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.patch = patch.object(bridge, "SNAPSHOTS", Path(self.temp.name))
        self.patch.start()

    def tearDown(self):
        self.patch.stop()
        self.temp.cleanup()

    def test_save_and_load_snapshot(self):
        saved=bridge.snapshot_script_data({
            "path":"ReplicatedStorage/Config","class":"ModuleScript","source":"return { test = true }"})
        self.assertRegex(saved["id"],r"^[a-f0-9]{32}$")
        self.assertNotIn("source",saved)
        restored=bridge.lookup_snapshot(saved["id"])
        self.assertEqual(restored["source"],"return { test = true }")
        self.assertEqual(restored["sha256"],bridge.source_hash(restored["source"]))

    def test_corrupt_snapshot_rejected(self):
        snap=bridge.snapshot_script_data({"path":"ServerScriptService/X","source":"return 1"})
        path=bridge.SNAPSHOTS/(snap["id"]+".json")
        path.write_text(path.read_text(encoding="utf-8").replace("return 1","return 9"),encoding="utf-8")
        with self.assertRaises(ValueError):
            bridge.lookup_snapshot(snap["id"])

    def test_invalid_ids_rejected(self):
        for name in ("../secret","abc","g"*32):
            with self.subTest(name=name),self.assertRaises(ValueError):
                bridge.lookup_snapshot(name)

    def test_automatic_backup_blocks_unsaved_change(self):
        state={"op":"set_script","path":"ReplicatedStorage/X","source":"return 5",
               "expected_source":"return 2"}
        with patch.object(bridge,"studio_command",return_value={
            "path":"ReplicatedStorage/X","source":"return 3","class":"ModuleScript"}):
            with self.assertRaises(ValueError):
                bridge.prepare_script_backups(state)
        self.assertEqual(list(bridge.SNAPSHOTS.glob("*.json")),[])

    def test_automatic_backup_adds_compare_guard(self):
        state={"op":"replace_script","path":"ReplicatedStorage/X","search":"3","replacement":"4"}
        with patch.object(bridge,"studio_command",return_value={
            "path":"ReplicatedStorage/X","source":"return 3","class":"ModuleScript"}):
            bridge.prepare_script_backups(state)
        self.assertEqual(state["expected_source"],"return 3")
        self.assertEqual(len(list(bridge.SNAPSHOTS.glob("*.json"))),1)


if __name__=="__main__":
    unittest.main()
