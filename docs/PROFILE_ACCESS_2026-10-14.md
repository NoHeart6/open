# User profile access — 14 October 2026

The owner explicitly requested access to all folders belonging to this Windows account. The installed agent previously approved only `C:\Users\Administrator\Documents\OpenCommander Workspace`. Requests targeting other folders failed with `Path outside approved roots`.

Agent 0.2.1 is installed and its policy now approves `C:\Users\Administrator`. Pairing credentials were preserved byte-for-byte, existing settings and encrypted journals were backed up before upgrade, and the agent reconnected to the existing gateway.

The previous code rejected any approved root containing the agent state directory. This prevented approving a whole user profile because state is stored under Local AppData. Runtime exclusions now protect the active state directory, the installed state directory and the running agent executable directory independently of approved roots. They are reapplied whenever policy is loaded and are not serialized as editable settings. Native permission editing supports this broader root.

Directory listing and recursive searches skip excluded locations. Scans skip unreadable entries rather than failing the entire search. Existing credential-location exclusions, junction/UNC/alternate-stream rejection, hardlink checks, compare-and-swap editing and native consent for sensitive operations remain enforced. Approving a profile is not an OS sandbox: locally approved shell commands run with the user's Windows permissions.

`system_info` now returns `agent_version` and `workspace`, including the Windows profile, approved roots and modification/screenshot permissions. ChatGPT can use this result to select an allowed directory instead of guessing a drive root.

Verified through the actual Railway MCP endpoint and installed Windows agent:

- List, create a fixture directory, write, read and search in Desktop, Documents, Downloads and AppData/Roaming.
- List the approved user profile.
- Deny remote file reads of internal agent configuration and directories outside the approved profile.
- Preserve OAuth audience and requested scopes after offline refresh; deny unowned devices and malformed requests.

Temporary verification files were removed. Results are in `artifacts/profile-access-results.json`; Rust regression tests and Clippy also pass. The encrypted backup and old executable are in `.runtime/profile-access-before-20261014-140544`.

This makes file and terminal working directories available throughout this user's profile. It does not implement Hermes' full memory, tool, browser, scheduling or delegation feature set. Sensitive commands still require local approval; existing credential exclusions and Windows permissions can still deny a path.
