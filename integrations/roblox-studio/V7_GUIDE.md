# Open Developer Suite 7.0

This release adds 22 commands to Open's Roblox Studio integration. They operate through the existing roblox_studio MCP tool (op=command), the paired Windows bridge, and the local project snapshot.

## New operations by feature

| Requested feature | Implemented operation | Verified implementation boundary |
|---|---|---|
| Semantic code search | v7_semantic_search | Bounded lexical ranking with fixed bilingual synonyms, NOT neural embeddings |
| Code impact predictor | v7_code_impact | Reverse static references, NOT causal runtime impact |
| Property-based tests | v7_property_tests | Reproducible random testing of four built-in reference models |
| Mutation testing | v7_mutation_tests | Known bugs on local reference models, NOT game source mutation |
| Test coverage mapper | v7_test_coverage | Behavior spec associations, NOT actual branch/line coverage |
| Behavior specification | v7_behavior_spec | Stores testable checks; supports editor assertions and runtime handoff |
| Event recorder | v7_event_trace | Captures supplied event metadata; no global automatic interception |
| Trace inspection | v7_trace_inspect | Correlates supplied events by ID and summarizes playtest data |
| Deterministic build | v7_build_manifest | Stable SHA-256 inventory of bounded Studio source, NOT reproducible .rbxl |
| Incremental build | v7_incremental_build | Reports source additions/removals/changes, NOT incremental code compilation |
| Asset provenance | v7_asset_provenance | Asset ownership/licensing metadata entered by user |
| Secrets vault | v7_secrets_vault | Windows current-user DPAPI ciphertext and no plaintext get API |
| Action policy | v7_action_policy | Policy classification; HTTP relay enforces confirmation for destructive actions |
| Command audit trail | v7_audit_trail | Local hash chain for operation name/outcome without raw source/secret data |
| Multi-place management | v7_multi_place | Local place ID and dependency manifest only |
| Teleport journey tester | v7_teleport_test | Offline transition/retry simulator; no TeleportService invocation |
| Compatibility matrix | v7_compatibility | Saves device status and test evidence |
| Resource budgets | v7_resource_budget | Studio memory and object-count limits |
| Plugin validator | v7_plugin_validate | Validated declarative manifest; no arbitrary Python imports |
| Capability registry | v7_registry | Lists installed built-in and declared extension capabilities |
| Extensible SDK | v7_plugin_register | Registers only allowlisted read-only aliases |
| Extension execution | v7_plugin_invoke | Invokes allowlisted built-in handler with explicit approved:true |

## Examples of MCP command objects

Code search:

{"op":"v7_semantic_search","query":"save virtual files from terminal","refresh":true,"limit":8}

Impact:

{"op":"v7_code_impact","path":"ReplicatedStorage/Kernel"}

Reproducible property checks:

{"op":"v7_property_tests","property":"economy_conservation","cases":200,"seed":123}

Store and verify an editor spec:

{"op":"v7_behavior_spec","action":"define","name":"linux_terminal","checks":[{"path":"StarterGui/LinuxDesktop/Terminal","exists":true}]}

{"op":"v7_behavior_spec","action":"verify","name":"linux_terminal"}

Source checkpoint and change report:

{"op":"v7_build_manifest","name":"checkpoint","action":"save","refresh":true}

{"op":"v7_incremental_build","baseline":"checkpoint","refresh":true}

Evaluate an action:

{"op":"v7_action_policy","command":{"op":"delete","path":"Workspace/OldPart"}}

Register a declarative extension:

{"op":"v7_plugin_register","manifest":{"name":"project_reader","version":"1.0.0","operations":[{"alias":"search","target":"v7_semantic_search"}]}}

{"op":"v7_plugin_invoke","name":"project_reader","alias":"search","approved":true,"args":{"query":"filesystem storage"}}

Audit:

{"op":"v7_audit_trail","action":"verify"}

## Security and boundaries

- The localhost relay rejects delete, terrain_clear, insert_asset and batches with destructive actions unless confirm_persistent_changes:true is supplied. Other historical Studio mutations retain the preexisting safeguards. This policy is not a universal authorization sandbox.
- The extension registry NEVER loads third-party Python/JS or executes arbitrary system commands. It provides declared aliases to fixed allowlisted read-only built-in operations.
- The audit chain detects casual log edits but is not remotely signed or immutable. A privileged local attacker could recreate it.
- Windows DPAPI encrypts vault content at rest for the local user; there is deliberately NO plaintext retrieval command. Do not paste production secrets into ChatGPT or CLI parameters: requests and prompts may expose them before encryption. Use a separately provisioned OS credential provider for real infrastructure credentials.
- Bounded script reading can miss large, truncated or inaccessible code. Reports show their limitations.
- Mutation tests and property tests operate on local reference models, not actual Luau scripts, and do not prove a game is bug-free.
- Cloud data, live teleport, true network fault injection, remote profiler and publication are not implemented by these commands.
- Files in .studio_suite/ and *.secret are ignored by Git and remain local.

## Verification

From repository root:

py -3 integrations/roblox-studio/test_v7.py
py -3 integrations/roblox-studio/test_suite.py
py -3 integrations/roblox-studio/test_v5.py
py -3 integrations/roblox-studio/test_bridge.py
npm --prefix apps/mcp-server run typecheck
npm --prefix apps/mcp-server test

Roblox Studio must restart after a new plugin version is installed. Refresh the Open connection in ChatGPT after a successful Railway deployment so the new input schema is available.
