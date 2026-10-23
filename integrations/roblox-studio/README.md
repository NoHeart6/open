# ChatGPT <> Roblox Studio local bridge

ChatGPT biasa + konektor Open + plugin Roblox Studio. Tidak memerlukan API key OpenAI.

## Mulai
- Setup sekali: setup.bat
- Server otomatis menyala saat login Windows (Task Scheduler: ChatGPTRobloxStudioBridge). start.bat tersedia untuk start manual.
- Restart Roblox Studio, buka place; di tab Plugins klik ChatGPT Bridge.
- Izinkan akses HTTP ke 127.0.0.1 jika Roblox meminta izin.
- Lihat CONNECTED pada panel plugin.
- Minta ChatGPT dengan @open: "Lihat Explorer di Roblox Studio dengan python bridge.py send --op tree --root game --depth 2, lalu buat GUI sesuai permintaanku."
- Semua file ada di C:\Users\Administrator\Documents\roblox_plugin_connector

## CLI
- py -3 bridge.py status
- py -3 bridge.py send --op ping
- py -3 bridge.py send --op tree --root StarterGui --depth 2
- py -3 bridge.py send --op selection
- py -3 bridge.py send --op read_script --path ReplicatedStorage/Config
- py -3 bridge.py send --file command.json

## Operasi JSON
ping, tree, selection, properties, read_script, create, update, set_script, delete, duplicate, move, select, batch, undo, redo.
Contoh file command.json:
{"op":"batch","actions":[{"op":"create","parent":"StarterGui","class":"ScreenGui","name":"NotebookUI"},{"op":"create","parent":"StarterGui/NotebookUI","class":"Frame","name":"Paper","properties":{"Size":{"__type":"UDim2","xs":0.8,"xo":0,"ys":0.8,"yo":0},"BackgroundColor3":{"__type":"Color3","r":250,"g":246,"b":232}}}]}
Script:
{"op":"set_script","path":"ReplicatedStorage/NotebookConfig","class":"ModuleScript","source":"return { Theme = 'Paper' }"}
Properties:
{"op":"update","path":"StarterGui/NotebookUI","properties":{"ResetOnSpawn":false}}
Traversal:
{"op":"tree","root":"game","depth":3,"limit":150}
Supported typed values: Color3(r,g,b 0..255), UDim2(xs,xo,ys,yo), UDim(s,o), Vector2(x,y), Vector3(x,y,z), CFrame(x,y,z), Enum(value:Enum.Font.Gotham), Ref(path).

Perubahan edit-mode bisa di-Undo lewat Ctrl+Z/ChangeHistoryService. Perintah batch adalah satu transaksi Undo dan dibatalkan bila gagal. Save place tetap dilakukan di Roblox Studio.

Server lokal listen 127.0.0.1:34872 dengan token dari bridge_token.txt. Jangan bagikan token.
ChatGPT biasa tidak bisa secara otomatis memakai server lokal tanpa konektor Open aktif.
Jika offline: periksa py -3 bridge.py status dan bridge.log, jalankan start.bat, izinkan HTTP localhost di Studio, restart plugin.

## V2.0 operations

Capabilities and project_info report available Studio services and supported operations.
Tree and find inspect Explorer (root/query/class/limit).
Read_script, script_search, set_script, replace_script, open_script work with the Luau editor.
ScriptEditorService.UpdateSourceAsync handles open-editor script changes.

Set_script accepts source_file (relative to the bridge directory, 800 KB max)
to avoid MCP command-size limits.
Get_attributes, set_attributes, tags, set_tags work with metadata and CollectionService.
Pivot moves Models/Parts with typed CFrame. Terrain_fill and terrain_clear support
block, ball and cylinder, with center/size/radius/height/material. Insert_asset uses
InsertService.LoadAsset under the logged-in Roblox user permissions.

Regular editing includes create/update/delete/move/duplicate/select/batch/undo/redo.
Data-model operations are ChangeHistoryService recorded. NOTE: Studio Undo for
ScriptEditorService changes does not always restore open-editor source. Save the prior
source from read_script and explicitly restore with set_script and expected_source.
Use desktop UI automation for saving, publishing, playtests, animation editor and
Studio-only menus. No local plugin can expose all UI-only or permission-gated actions.

Supported typed properties: Color3, BrickColor, UDim, UDim2, Vector2, Vector3,
CFrame, Enum, Ref, NumberRange, NumberSequence, ColorSequence.

Commands:
find: {"op":"find","root":"Workspace","query":"door","limit":30}
script_search: {"op":"script_search","root":"game","query":"require("}
set_attributes: {"op":"set_attributes","path":"Workspace/Door","attributes":{"Locked":true}}
set_script from file: {"op":"set_script","path":"ReplicatedStorage/System","class":"ModuleScript","source_file":"scripts/System.luau"}
terrain_fill: {"op":"terrain_fill","shape":"ball","center":{"x":0,"y":0,"z":0},"radius":12,"material":"Grass"}
pivot: {"op":"pivot","path":"Workspace/House","cframe":{"__type":"CFrame","x":0,"y":10,"z":0}}
## Version 3.0.1 - simulation, diagnostics, safe source snapshots

The Studio plugin now exposes run_state; test_run, test_pause, test_resume and test_stop; logs, diagnostics, and logs_clear. Run and Stop require confirm_persistent_changes=true. Test simulation uses RunService:Run() WITHOUT a player, not Roblox's usual Play button. RunService:Stop() does not restore the place to its pre-simulation state. Test in a disposable place or save a backup first.

The plugin retains 240 messages observed by LogService.MessageOut, including warnings and errors. Query logs with since, severity, query and limit. Messages from isolated contexts may be missing, and clearing plugin logs does not clear Roblox Studio's other output windows.

Python bridge.py additionally handles snapshot_script, list_snapshots and restore_script commands via the standard roblox_studio command interface. Snapshots save Luau editor source, an immutable ID, timestamp, path and SHA-256 under the PRIVATE .snapshots folder on the paired Windows computer. On set_script / replace_script, the bridge automatically saves an existing source and adds an expected_source precondition before modifying. restore_script requires a matching expected_sha256, unless force=true is deliberately specified; it preserves the replaced source in another backup first.

Examples:
{"op":"run_state"}
{"op":"test_run","confirm_persistent_changes":true}
{"op":"logs","query":"Exception","limit":30}
{"op":"diagnostics","since":0,"severity":"warning"}
{"op":"test_stop","confirm_persistent_changes":true}
{"op":"snapshot_script","path":"ReplicatedStorage/Config"}
{"op":"list_snapshots","path":"ReplicatedStorage/Config"}
{"op":"restore_script","snapshot_id":"32-char-hex-id","expected_sha256":"64-char-current-source-sha256"}

For visual checks use Open desktop_screenshot and desktop_view with Roblox Studio foreground, then repair using desktop_interact and roblox_studio tools. Screenshots capture the desktop and are subject to local permission settings. Real multi-client playtesting, network simulation, saving and publishing require Roblox Studio UI operations through Open's existing desktop controls, not a documented automated guarantee.

Run local offline regression tests with py -3 test_bridge.py from the integration folder. Never commit bridge_token.txt, generated ChatGPTStudioBridge.luau, .snapshots or personal game backups.
## Version 4.0 � Studio player tests, VirtualInput, device simulation

Open's Roblox Studio plugin supports asynchronous playtests using Roblox StudioTestService. The local plugin currently supports **play**, **run** (no player) and **multiplayer** (1�8 simulated clients), and records structured completion data. Tests are started from a saved or disposable place. A test run can leave edited world state, so `confirm_persistent_changes:true` is required. Tests use temporary scripts, a RemoteEvent, and configuration StringValue; they are removed after the test finishes. A test that fails to start or fails to record enough players is not reported as successful.

Operations:

- `playtest_start`: `{"op":"playtest_start","mode":"play","duration":12,"confirm_persistent_changes":true,"expected_ui_min":1,"steps":[{"type":"key","key":"Space","delay":0.2}]}`. Returns a test ID immediately.
- `playtest_status`: `{"op":"playtest_status","id":"<test ID>"}`. Returns `state`, `result`, per-client `events`, `players` and `expectedPlayers`. A successful test requires observing the requested players and no input or required-GUI errors.
- `mode:"multiplayer"` with `players:2` uses StudioTestService's multiple-client mode; multiplayer tests use a minimum duration of 18 seconds to allow client joins.
- Input steps: `{"type":"key","key":"Space"}`, `{"type":"click","x":400,"y":300}`, or `{"type":"text","text":"hello"}`. The LocalScript creates input with `UserInputService:CreateVirtualInput()`, restricted to the game's own UI. Input reporting is best-effort.
- Client UI audit sends `ui_audit` and optional `ui_missing` events. Use `expected_ui_min` to require an observed minimum number of visible GuiObjects. Screen objects normally clone to PlayerGui when the character spawns; a zero count should not be interpreted as evidence of a correct layout.
- `ui_scan`: `{"op":"ui_scan","root":"StarterGui","limit":80}` returns edit-mode GuiObject positions/sizes and their visibility. Those are not a replacement for checking runtime GUI on each device.
- `device_list`, `device_status`, `device_select` (`device_id`), `device_resolution` (`width`, `height`), `device_orientation` (`Portrait`, `LandscapeLeft`, `LandscapeRight`), `device_stop`. All use StudioDeviceSimulatorService. Restore the default device with `device_stop` after testing.

Visual screenshot validation remains through the **Open desktop_screenshot / desktop_view tools**, not through an unsupported Roblox screenshot API. Open can focus the Studio view, capture images, compare GUI visually, and use desktop_interact to test controls. End-to-end playtest source instrumentation reports input submission; it does not prove a specific GUI handler executed unless the game includes a corresponding test assertion.

Roblox's StudioTestService, VirtualInput and StudioDeviceSimulatorService are Studio-only; they do not run in published experiences. Publishing, multiplayer tests on live servers, persistence tests, performance benchmarking and full asset QA remain outside this release.

References:
- https://create.roblox.com/docs/reference/engine/classes/StudioTestService
- https://create.roblox.com/docs/reference/engine/classes/VirtualInput
- https://create.roblox.com/docs/reference/engine/classes/StudioDeviceSimulatorService
- https://create.roblox.com/docs/studio/testing-modes
