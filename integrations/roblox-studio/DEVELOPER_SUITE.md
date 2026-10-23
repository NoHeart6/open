# Open Roblox Studio Developer Suite

Version 6.0.0. A unified developer suite built on the Open MCP 5.0 automation, not five separate server installations.

## Capabilities and limits

All 26 commands are registered through the existing roblox_studio MCP tool. Each command is passed as the tool's command JSON object and begins with suite_. They use the paired local Python bridge, so the Windows device and bridge.py must be online.

| Feature | Command | Implementation | Scope |
|---|---|---|---|
| 1 Project Architecture Scanner | suite_scan | Live inventory of Studio services, scripts, UI and remotes | Bounded to 1,000 nodes and 380 KB of source |
| 2 Dependency Graph | suite_dependencies | Infers static module links, unresolved requires, cycles | Regex/static only |
| 3 Luau Static Analysis | suite_static | Detects common code risks and source truncation | Heuristic, not Luau LSP |
| 4 Runtime State Inspector | assert_state, playtest_start | Edit-time properties and runtime server/client assertions | Requires observable properties |
| 5 Project Memory | suite_memory | Local key/value decisions and lists | Saved on paired Windows PC |
| 6 GUI Builder | suite_gui | Creates declarative ScreenGui, text, frame, control hierarchy | User-defined blueprint, not automatic art generation |
| 7 Design System Manager | suite_theme | Stores RGB tokens and can apply colors to selected instances | Explicit target paths |
| 8 Procedural World Generator | suite_world | Seeded, reproducible geometry layouts | Blocks and folders, not textured worlds |
| 9 Asset Manager | suite_assets | Catalogues permissions, IDs and insertion | Rights not independently verified |
| 10 Animation & Audio Workflow | suite_timeline | Validates and stores event, audio, animation and tween timelines | Plan/serialization; no playback engine |
| 11 DataStore Sandbox | suite_data | Local JSON sandbox with versioned key rename migrations | Never touches Roblox production DataStore |
| 12 RemoteEvent Security Audit | suite_security | Flags potentially unsafe validation and HTTP patterns | Review only, not a security guarantee |
| 13 Economy and Inventory Simulation | suite_economy | Atomic synthetic transactions and duplicate-ID rejection | Money ledger; inventory implementation remains game-specific |
| 14 API Contract Manager | suite_contracts | Field-type validation and breaking-change detection | JSON-like data models |
| 15 NPC Behavior Designer | suite_npc | Finite-state design, reachability, deterministic event simulation | Not linked to a live NPC by default |
| 16 Developer Dashboard | suite_dashboard | Saves readable local HTML project report | Static local file |
| 17 AI Task Planner | suite_plan | Dependency-aware task queue | AI chooses task decomposition; no background agent |
| 18 Git Source Sync | suite_git, suite_export | Read-only git status and guarded local Luau exports | No automatic Rojo synchronization or pushes |
| 19 Accessibility and Localization Audit | suite_accessibility | Flags small touch targets/text and stores locale entries | Edit preview; must visually validate |
| 20 Documentation Generator | suite_document | Markdown inventory and inferred dependency links | Snapshot-derived, may be incomplete |
| 21 Scenario Test Engine | suite_scenario | Runs 1–5 sequential real playtest cases with runtime assertions | One disposable Studio place |
| 22 Project Recovery | place_backup, place_restore | SHA-256 backup and non-destructive recovered copy of saved place | Unsaved editor state excluded |
| 23 Self-Healing Workflow | diagnose_errors, repair_verify | Diagnosis hints, patch with source guard, test and rollback | Patch must be authored externally |
| 24 Visual Regression | suite_visual | Pixel difference and bounding box for two local screenshots | Requires supplied images, does not capture them |
| 25 Performance and Network | suite_perf, suite_network | Studio memory/object snapshot, baseline comparison, network profile plan | No FPS or injected latency measurement |
| 26 Release Gate | suite_release | Gate against completed inventory/test result + approval | Does NOT publish to Roblox Cloud |

## Examples

### Scan and audit

{"op":"suite_scan","preview_limit":40}

{"op":"suite_dependencies"}

{"op":"suite_security"}

{"op":"suite_static"}

{"op":"suite_accessibility"}

{"op":"suite_perf","action":"baseline"}

### GUI blueprint (dry run first)

{"op":"suite_gui","name":"LinuxDesktop","elements":[
  {"class":"Frame","name":"Panel","size":[1,0,0,52],"background":[34,40,48]},
  {"class":"TextButton","name":"TerminalButton","parent":"StarterGui/LinuxDesktop/Panel",
   "size":[0,130,0,44],"text":"Terminal","text_size":18}
]}

To apply, send the same command with apply:true and confirm_persistent_changes:true. Use a disposable place and verify the preview first.

### Local DataStore sandbox

{"op":"suite_data","action":"put","namespace":"linux_dev","key":"file1","value":{"name":"hello.txt","content":"Hello"}}

{"op":"suite_data","action":"get","namespace":"linux_dev","key":"file1"}

{"op":"suite_data","action":"migrate","namespace":"linux_dev","version":2,"renames":{"file1":"file_1"}}

No real Roblox DataStore keys are accessed.

### Runtime test suite

{"op":"suite_scenario","apply":true,"confirm_persistent_changes":true,"cases":[
 {"mode":"play","duration":14,
  "checks":[{"scope":"server","path":"ReplicatedStorage/KernelState","attribute":"Booted","equals":true}]},
 {"mode":"multiplayer","players":2,"duration":24,
  "checks":[{"scope":"client","path":"PlayerGui/LinuxDesktop","exists":true}]}
]}

This runs genuine StudioTestService sessions sequentially. Each case must define assertions. It stops when a case fails. No overnight/background execution.

### Visual regression

{"op":"suite_visual","before":"C:\\Users\\Administrator\\Pictures\\before.png",
 "after":"C:\\Users\\Administrator\\Pictures\\after.png",
 "pixel_threshold":10,"max_changed_ratio":0.01}

This only compares two images already saved by the user. It does not take screenshots.

### Recovery and release

Use the existing place_backup operation to copy a saved .rbxl/.rbxlx file, and place_restore to create a separate recovered file.

{"op":"suite_release","approve_release":true}

Even a passing gate does not publish any place; Roblox Cloud authorization and an independently confirmed publication action would still be needed.

## Security

- Scripts and project snapshots remain inside the paired Windows user's local .studio_suite directory. They are ignored by Git.
- Project source scanning is bounded, and scripts may be partially scanned. Never make safety claims about unseen source.
- Actions that mutate Studio require apply:true and confirm_persistent_changes:true. The real tests can change temporary world state.
- The asset catalog's license field is provided by the user; it does not prove asset licensing.
- The visual comparer accepts only local image files under Documents, Pictures, or Desktop. It does not access or upload photos by itself.
- The suite uses no external dependencies for its main Python operations; Pillow (installed on this Windows PC) is required only for image comparisons.
- Git and release commands are read-only. Publishing, payment processing, and production DataStore writes are not implemented.

## Testing

Run in integrations/roblox-studio:

  py -3 test_suite.py
  py -3 test_v5.py
  py -3 test_bridge.py

Run from repository root:

  npm --prefix apps/mcp-server run typecheck
  npm --prefix apps/mcp-server test
  git diff --check

Studio implementation is still Roblox-dependent: open a saved test place, start bridge.py, and confirm ping returns version 6.0.0. Refresh the Open plugin connector schema in ChatGPT after Railway has deployed the corresponding MCP commit.

Relevant Roblox docs: https://create.roblox.com/docs/studio/testing-modes and https://create.roblox.com/docs/cloud-services/data-stores
