# Open × Roblox Studio 5.0 — verification and recovery

## Operations

Roblox Studio plugin version **5.0.0** extends the v4 edit-mode and playtest commands. The Windows Python relay adds higher-level workflows. All commands use the existing `roblox_studio` MCP tool with `op: "command"`; the JSON below goes into its `command` argument.

### Functional assertions

For editor assertions (no game mutation):

```json
{"op":"assert_state","checks":[
  {"id":"terminal","path":"StarterGui/LinuxGui/Terminal","exists":true},
  {"id":"prompt","path":"StarterGui/LinuxGui/Terminal/Prompt","property":"Text","contains":"$"}
]}
```

The response has `passed`, `total`, and one result per check. For runtime tests:

```json
{"op":"playtest_start","mode":"play","duration":14,"confirm_persistent_changes":true,
 "checks":[
   {"id":"shell-visible","scope":"client","path":"PlayerGui/LinuxGui/Terminal","property":"Visible","equals":true},
   {"id":"world-state","scope":"server","path":"ReplicatedStorage/KernelState","attribute":"Booted","equals":true}
 ]}
```

Use `playtest_status` with the returned test ID. Supported predicates: `exists`, `equals`, `not_equals`, `contains`, `gt`, `lt`; properties and primitive attributes are supported. Runtime checks run at test completion on the server and after the supplied input steps on the client. `assert_pass` and `assert_fail` are included in the result. When a runtime assertion fails, the result has `success: false`. This is not a replacement for comprehensive game-specific integration tests, particularly DataStore persistence.

### Diagnose and guarded repair

```json
{"op":"diagnose_errors","since":0}
```

Reads Studio's recent error log buffer and attempts to associate errors with script paths, lines and local excerpts. A suggested location is a **hint**, not proof of the root cause. No code is generated or executed from error messages.

```json
{"op":"repair_verify","path":"ReplicatedStorage/Kernel",
 "source":"return { booted = true }",
 "expected_sha256":"<sha256-of-current-editor-source>",
 "confirm_persistent_changes":true,"mode":"play","duration":14,
 "checks":[{"id":"kernel-alive","scope":"server","path":"ReplicatedStorage/Kernel","exists":true}]}
```

Workflow: compare current source against the SHA-256 precondition, create a local source snapshot, apply the supplied code, run a real player playtest, evaluate the **required** runtime assertions, and either retain the change (`verified`) or restore the original source (`rolled_back`). If the test does not finish or another editor changed the source, the result is `manual_recovery_required`, and no unsafe automatic overwrite is attempted. For large scripts, use `source_file` in the relay's approved local directory (maximum 800 KB). The AI proposes a fix; the plugin never claims to autonomously write correct code.

### Full saved-place file backups

Roblox Studio does not expose a reliable programmatic Save-to-File for the entire place through a normal plugin. **First save the place in Studio** using File → Save to File / Download a Copy. Then:

```json
{"op":"place_backup","path":"C:\\Users\\Administrator\\Documents\\MyGame.rbxl"}
```

The relay copies the entire saved `.rbxl` / `.rbxlx` file into the private `.place_backups` folder, hashes it with SHA-256 and records metadata. Only existing files under the Windows user's Documents, Desktop or Pictures are accepted, up to 120 MB. It does **not** include unsaved edits, DataStores on Roblox servers, unpublished assets, or secrets.

```json
{"op":"place_backups","limit":30}
{"op":"place_restore","snapshot_id":"<32-hex-id>"}
```

Restore checks the hash and writes a **new recovered copy** under `.place_backups/recovered/` instead of overwriting the original place. Open that recovered place file explicitly in Studio, inspect it, and save as desired. Never assume restoration changes the currently open place automatically.

### Safety and limitations

- Backups, saved places, token files and private game source remain on the paired Windows computer; do not commit `.place_backups`, `.snapshots`, or `bridge_token.txt`.
- The playtest harness can change world state; use an expendable place copy and set `confirm_persistent_changes: true` only after choosing it.
- Only primitive equality and numeric/string comparisons are supported. Complex game logic needs test instrumentation with observable values or attributes.
- `diagnose_errors` only sees LogService messages captured while the Studio plugin was running; missing messages do not demonstrate an absence of bugs.
- Game-specific fixes are supplied by ChatGPT or the developer; a passing test confirms its assertions, not every possible behavior.

Offline tests: `py -3 test_bridge.py`, `py -3 test_v5.py`. MCP tests: `npm --prefix apps/mcp-server run typecheck`, `npm --prefix apps/mcp-server test`.
