# OpenCommander Motion Graphics Plugin

The previous MangaMotion character animation tools have been replaced by **motiongraphics**.

Cloud AI / ChatGPT proposes editable typed actions. Windows agent validates allowed project directory, passes JSON into `scripts/mcp-control.mjs` in the local Motion Graphics Studio folder, and uses the existing Remotion engine. **No local AI models**.

## Operations

- `list` — list v1.0 graphics projects.
- `inspect` — get canvas, layers, text, shapes and keyframe tracks.
- `set_text` — create a new scene with one headline/caption updated.
- `patch` — create a new scene with numeric keyframe changes.
- `compile_motion` — transform a structured cloud AI motion plan into an editable JSON timeline.
- `import_image` — import a static image to a regular image layer, not anime segmentation.
- `render_still` / `render_video` — render a preview PNG or H.264 MP4 using Remotion.

The tool schema accepts `schema_version:"1.0"`, actions `title_reveal`, `slide_in`, `fade_in`, `zoom_in`, `pulse`, `count_up`, `wipe_in`, `float`, `stagger_cards` and `GraphicsStudio` composition.

Path to the active app on the paired Windows device:

`C:\Users\Administrator\Pictures\project\MangaMotion`

The existing folder is retained for compatibility. Internal app/product name: **Motion Graphics Studio**.

Old anime body-part segmentation, PSD, mesh deform, head-turn, walking and waving actions have been retired from active source and MCP. Source snapshot/branch is archived in the app workspace.

Tests: `npm test --prefix apps/mcp-server`, `go test ./...` in gateway, `cargo check --manifest-path apps/windows-agent/Cargo.toml`.
