# Persistent owner approval — 14 October 2026

The user explicitly requested removal of repeated approval clicks in the native Windows agent. Agent 0.2.2 adds the local `policy.ask_before_sensitive` setting. Its default is `true`, including when an older configuration does not contain the field. This PC is explicitly configured with `allow_modify: true` and `ask_before_sensitive: false`.

Sensitive tasks from the authenticated, paired owner now execute under that saved approval without creating a native consent dialog. This includes terminal execution, allowlisted application launch, desktop interactions, clipboard access, process termination and individual-file deletion. Existing path, application, scope, signature, owner, deadline and cancellation checks continue to run. Each use of saved approval is recorded as `local_owner_preapproval_used` in the local activity log. Pausing, disconnecting or revoking the device still prevents work.

This setting permits commands with the Windows user's privileges. Terminal commands are not a filesystem sandbox. Do not describe the directory allowlist as confinement for shell commands.

The native window contains `Confirm sensitive actions (uncheck to trust the connected owner)`. Checking it and saving restores per-operation confirmation. Disabling modification permission also prevents silent sensitive execution. The agent now runs in tray/background mode on this PC; the saved choice persists across restart and upgrade. ChatGPT's own permission controls were not changed.

Verification:

- Eighteen Rust tests and Clippy pass, including old-config default, rejection of remote argument overrides, persistence, restoring confirmation, and native checkbox save/load.
- Seven MCP tests, lint, typecheck and build pass.
- `scripts/diagnose-chatgpt-integration.ps1 -Unattended` passes 25 production checks. The actual installed Windows agent executed PowerShell, launched an allowlisted Notepad fixture, and terminated that specific new process without a local approval driver or an `approval_pending` state. Profile file operations, OAuth refresh, ownership and schema checks also pass.

The first verification found a saved setting with confirmation re-enabled. Reapplying the authorized setting while the agent was stopped and restarting in background mode resolved the observed state mismatch. The final read-back is `ask_before_sensitive: false`. Pairing credentials were preserved byte-for-byte. Backup state and the prior executable are retained in `.runtime/trusted-owner-before-20261014-141954`.

The public MCP deployment `6779d84e-ca3d-4c46-aff1-14321c010349` updates tool descriptions to refer to the local approval policy and serves the signed 0.2.2 installer metadata. Results are in `artifacts/unattended-access-results.json`. Independent production checks are distinct from a user-observed ChatGPT conversation.
