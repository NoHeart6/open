# OpenCommander plugin 0.5.0

Control a paired Windows computer and its local WordCraft editor through authenticated MCP tools. This package includes the MCP manifest, Windows/WordCraft skill, and OpenCommander icons.

## Connect

For ChatGPT hosts/accounts supporting custom remote MCP servers, open [Plugins](https://chatgpt.com/plugins), add `https://mcp-production-394c.up.railway.app/mcp`, and authenticate with OAuth. Pair the Windows agent and configure local permissions before operating the device. Start with `device_list` and `system_info`; refresh an existing connection after schema updates.

Extracting this ZIP alone does not install a connection into a ChatGPT account. Other hosts may import the plugin manifest if they support its format. No account credentials or device identity are included.

## WordCraft

Download WordCraft portable from the [GitHub release](https://github.com/NoHeart6/open/releases/tag/v0.5.0). Run `wordcraft.exe --control 7981` and allow the app/CLI and document folders in the local OpenCommander UI. Use `wordcraft` with the actual absolute `cli_path` to `wordcraft-cli.exe` and `tool: list_commands`; use `wordcraft_compose` for structured DOCX creation.

See the [full connection guide](https://github.com/NoHeart6/open/blob/main/docs/PLUGIN_ACCESS.md) and [Windows guide](https://github.com/NoHeart6/open/blob/main/docs/INSTALL_WINDOWS.md). OpenCommander is MIT licensed; the separate WordCraft archive retains its upstream MIT OR Apache-2.0 licenses.
