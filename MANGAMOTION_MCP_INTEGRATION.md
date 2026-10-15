# MangaMotion OpenCommander MCP Integration (development branch)

Branch: \`feature/mangamotion-mcp\`; based on OpenCommander 0.5.x.
Source: \`C:\Users\Administrator\Pictures\project\OpenCommander-MangaMotion-MCP\`.
Target app: \`C:\Users\Administrator\Pictures\project\MangaMotion\`.

This change introduces a **\`mangamotion\` MCP tool**. The MCP server registers an action schema; gateway forwards to Windows agent; the agent runs the MangaMotion deterministic CLI (\`scripts/mcp-control.mjs\`) using a bounded stdin JSON message. No local AI models are used.

## Operations

- \`list\`: list available JSON projects.
- \`inspect\`: project identity, part/layer names, tracks, source SHA-256.
- \`patch\`: apply 1–40 typed numeric keyframe edits; never overwrites original project; requires a distinct \`output\` filename.
- \`render_still\`: render one PNG frame; output goes to \`MangaMotion/out\`.
- \`render_video\`: render MP4 (long-running background task).
- Use existing \`task_status\` for completion and inspection.

**Example tool payload (illustrative; MCP wrapper adds device_id + idempotency_key):**

\`\`\`json
{
  "device_id": "PAIRED_DEVICE_ID",
  "idempotency_key": "mangamotion-inspect-example-001",
  "timeout_seconds": 60,
  "project_root": "C:\\Users\\Administrator\\Pictures\\project\\MangaMotion",
  "op": "inspect",
  "project": "raster-character-demo.json"
}
\`\`\`

### Example animation edit

\`\`\`json
{
  "device_id": "PAIRED_DEVICE_ID",
  "idempotency_key": "mangamotion-edit-example-001",
  "timeout_seconds": 60,
  "project_root": "C:\\Users\\Administrator\\Pictures\\project\\MangaMotion",
  "op": "patch",
  "project": "raster-character-demo.json",
  "output": "raster-character-prompt-edited.json",
  "changes": [
    {"target":"head","property":"rotation","frame":70,"value":12},
    {"target":"hair_sway","property":"rotation","frame":88,"value":4}
  ]
}
\`\`\`

Then render with \`op:"render_still"\`, \`project:"raster-character-prompt-edited.json"\`, \`frame:70\`, \`output:"head-turn.png"\`. Output and project file names are restricted to a single filename under managed project/output directories by MangaMotion CLI.

## Design

\`\`\`text
ChatGPT (cloud perception/planning)
  -> OpenCommander MCP server TS (typed tool)
  -> OpenCommander Go gateway (existing task system)
  -> OpenCommander Windows Rust agent ("mangamotion")
  -> node.exe --experimental-strip-types MangaMotion/scripts/mcp-control.mjs
  -> structured JSON scene inspect/patch + Remotion preview / MP4
\`\`\`

No shell script is accepted from the MCP caller. Agent validates \`project_root\` and CLI path under approved Windows roots. The target MangaMotion process parses a typed and bounded JSON contract. The source project must exist and validates as a scene graph. The tool uses existing modification approval rules.

## Integration status

**Tested:** TypeScript typecheck and full MCP test suite, Rust cargo check; CLI inspect/patch/still smoke tests via existing OpenCommander terminal tool.

**Not done:** This worktree has not been merged into the installed live OpenCommander release, built into an installer, or deployed. Users cannot call \`mangamotion\` as a native tool until both MCP server and Windows agent are upgraded/reloaded. Do not confuse the built source with a live tool. This integration is additive and avoids modifying concurrent uncommitted changes in the main OpenCommander checkout.

**Current scope:** Structured project-level operations. Cloud image segmentation / arbitrary prompt parsing / PSD automatic rig reconstruction require further modules.
