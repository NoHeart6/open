import express from "express";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import { z } from "zod";

const PORT = Number(process.env.PORT ?? 8787);
const GATEWAY_URL = process.env.GATEWAY_URL ?? "http://localhost:8080";
const GATEWAY_TOKEN = process.env.GATEWAY_TOKEN ?? "";
const DIAGNOSTIC = process.env.OC_DIAGNOSTIC === "1";

function audit(tool: string, owner: string, ok: boolean, ms: number) {
  console.log(JSON.stringify({ t: "audit", tool, owner, ok, ms }));
}

const OWNER_OVERRIDE = process.env.GATEWAY_OWNER;

async function gateway<T>(path: string, opts: RequestInit & { owner: string; idempotency?: string }): Promise<T> {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (GATEWAY_TOKEN) headers.authorization = `Bearer ${GATEWAY_TOKEN}`;
  headers["x-owner-id"] = OWNER_OVERRIDE ?? opts.owner;
  if (opts.idempotency) headers["x-idempotency-key"] = opts.idempotency;
  const r = await fetch(`${GATEWAY_URL}${path}`, { ...opts, headers });
  if (!r.ok) throw new Error(`gateway ${r.status}: ${await r.text()}`);
  return (await r.json()) as T;
}

function ownerFrom(req: express.Request): string {
  const h = req.header("authorization") ?? "";
  if (!h && DIAGNOSTIC) return "diagnostic-owner";
  return h.slice(0, 64) || "unknown";
}

const DeviceId = z.string().min(1).max(128);
const TaskId = z.string().min(1);

function createServer() {
  const server = new McpServer(
    { name: "opencommander", version: "0.1.0" },
    { instructions: "Windows remote control via OpenCommander. List devices first, check status before actions. Confirm destructive ops." }
  );

  const reg = (
    name: string,
    desc: string,
    schema: Record<string, z.ZodTypeAny>,
    ann: { readOnlyHint: boolean; destructiveHint: boolean; openWorldHint: boolean },
    handler: (args: Record<string, unknown>, owner: string) => Promise<unknown>
  ) => {
    server.registerTool(
      name,
      { title: name, description: desc, inputSchema: schema, annotations: ann },
      async (args: Record<string, unknown>, extra: unknown) => {
        const t0 = Date.now();
        // deno-lint-ignore no-explicit-any
        const owner = (extra as { requestInfo?: { headers?: Record<string, string> } })?.requestInfo?.headers?.["x-owner-id"] as string ?? "oauth-owner";
        try {
          const result = await handler(args, String(owner));
          audit(name, String(owner), true, Date.now() - t0);
          return { content: [{ type: "text" as const, text: JSON.stringify(result).slice(0, 20000) }], structuredContent: result as Record<string, unknown> };
        } catch (e) {
          audit(name, String(owner), false, Date.now() - t0);
          return { content: [{ type: "text" as const, text: `Error: ${(e as Error).message}` }], isError: true as const };
        }
      }
    );
  };

  if (DIAGNOSTIC) {
    reg("diag_ping", "Diagnostic read-only ping to prove ChatGPT Chat connectivity.", {},
      { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
      async () => ({ ok: true, server: "opencommander-diag", time: new Date().toISOString() }));
    reg("diag_system_info", "Diagnostic static system sample (no device access).", {},
      { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
      async () => ({ os: "Windows (sample)", note: "diagnostic only; pair a device for live data" }));
    return server;
  }

  reg("device_list", "List paired Windows devices for this owner.", {},
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (_, owner) => gateway("/internal/v1/devices", { method: "GET", owner }));
  reg("device_status", "Connection/heartbeat status of one device.", { device_id: DeviceId },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway(`/internal/v1/devices/${a.device_id}/status`, { method: "GET", owner }));
  reg("system_info", "OS, CPU, RAM, disk, uptime, network.", { device_id: DeviceId },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, idempotency: `sys-${Date.now()}`, body: JSON.stringify({ device_id: a.device_id, op: "system_info", params: {} }) }));
  reg("file_list", "List a directory on the device.", { device_id: DeviceId, path: z.string().min(1) },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, body: JSON.stringify({ device_id: a.device_id, op: "file_list", params: { path: a.path } }) }));
  reg("file_read", "Read a text file (capped).", { device_id: DeviceId, path: z.string().min(1), max_bytes: z.number().int().positive().max(1000000).optional() },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, body: JSON.stringify({ device_id: a.device_id, op: "file_read", params: a }) }));
  reg("file_write", "Atomic write with overwrite protection.", { device_id: DeviceId, path: z.string().min(1), content: z.string(), overwrite: z.boolean().optional() },
    { readOnlyHint: false, destructiveHint: true, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, idempotency: `fw-${a.device_id}-${a.path}-${Date.now()}`, body: JSON.stringify({ device_id: a.device_id, op: "file_write", params: a }) }));
  reg("file_manage", "Copy/move/rename/mkdir/zip/unzip/hash/delete(needs approval).", { device_id: DeviceId, action: z.enum(["copy", "move", "rename", "mkdir", "zip", "unzip", "hash", "delete"]), src: z.string().optional(), dst: z.string().optional(), approve: z.boolean().optional() },
    { readOnlyHint: false, destructiveHint: true, openWorldHint: false },
    async (a, owner) => {
      if (a.action === "delete" && a.approve !== true) throw new Error("delete requires approve:true (native consent on device)");
      return gateway("/internal/v1/tasks", { method: "POST", owner, idempotency: `fm-${Date.now()}-${Math.random()}`, body: JSON.stringify({ device_id: a.device_id, op: "file_manage", params: a }) });
    });
  reg("file_search", "Search files by name/content.", { device_id: DeviceId, root: z.string().min(1), pattern: z.string().min(1), max_results: z.number().int().positive().max(200).optional() },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, body: JSON.stringify({ device_id: a.device_id, op: "file_search", params: a }) }));
  reg("terminal_execute", "Run PowerShell/CMD with timeout; returns stdout/stderr/exit.", { device_id: DeviceId, shell: z.enum(["powershell", "cmd"]).optional(), command: z.string().min(1).max(8000), cwd: z.string().optional(), timeout_ms: z.number().int().positive().max(300000).optional() },
    { readOnlyHint: false, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, idempotency: `term-${Date.now()}-${Math.random()}`, body: JSON.stringify({ device_id: a.device_id, op: "terminal_execute", params: a }) }));
  reg("process_list", "List processes.", { device_id: DeviceId, filter: z.string().optional() },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, body: JSON.stringify({ device_id: a.device_id, op: "process_list", params: a }) }));
  reg("process_manage", "Kill/suspend/resume (kill needs approval).", { device_id: DeviceId, action: z.enum(["kill", "suspend", "resume"]), pid: z.number().int().positive(), approve: z.boolean().optional() },
    { readOnlyHint: false, destructiveHint: true, openWorldHint: false },
    async (a, owner) => {
      if (a.action === "kill" && a.approve !== true) throw new Error("kill requires approve:true");
      return gateway("/internal/v1/tasks", { method: "POST", owner, idempotency: `pm-${Date.now()}-${Math.random()}`, body: JSON.stringify({ device_id: a.device_id, op: "process_manage", params: a }) });
    });
  reg("application_launch", "Launch an app by path/ProgID.", { device_id: DeviceId, app: z.string().min(1), args: z.array(z.string()).optional() },
    { readOnlyHint: false, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, body: JSON.stringify({ device_id: a.device_id, op: "application_launch", params: a }) }));
  reg("desktop_screenshot", "Capture desktop; returns PNG resource.", { device_id: DeviceId, monitor: z.number().int().min(0).optional() },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, body: JSON.stringify({ device_id: a.device_id, op: "desktop_screenshot", params: a }) }));
  reg("desktop_windows", "List windows + active window.", { device_id: DeviceId },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, body: JSON.stringify({ device_id: a.device_id, op: "desktop_windows", params: {} }) }));
  reg("desktop_interact", "Mouse/keyboard/focus/shortcut/clipboard.", { device_id: DeviceId, action: z.enum(["move", "click", "double_click", "right_click", "drag", "scroll", "type", "key", "focus", "clipboard_get", "clipboard_set"]), x: z.number().optional(), y: z.number().optional(), text: z.string().optional(), keys: z.string().optional() },
    { readOnlyHint: false, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway("/internal/v1/tasks", { method: "POST", owner, body: JSON.stringify({ device_id: a.device_id, op: "desktop_interact", params: a }) }));
  reg("task_status", "Poll a task result.", { task_id: TaskId },
    { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway(`/internal/v1/tasks/${a.task_id}`, { method: "GET", owner }));
  reg("task_cancel", "Cancel a queued/running task.", { task_id: TaskId },
    { readOnlyHint: false, destructiveHint: false, openWorldHint: false },
    async (a, owner) => gateway(`/internal/v1/tasks/${a.task_id}/cancel`, { method: "POST", owner, body: "{}" }));

  return server;
}

const app = express();
app.use(express.json({ limit: "4mb" }));
app.get("/", (_req, res) => res.send("OpenCommander MCP"));
app.get("/health", (_req, res) => res.json({ ok: true }));
app.get("/.well-known/oauth-protected-resource", (_req, res) => {
  const base = process.env.MCP_PUBLIC_URL ?? `http://localhost:${PORT}`;
  res.json({ resource: base, authorization_servers: [process.env.OAUTH_ISSUER ?? `${base}`], scopes_supported: ["devices:read", "devices:write"] });
});
app.post("/mcp", async (req, res) => {
  const server = createServer();
  const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true });
  res.on("close", () => { transport.close(); server.close(); });
  await server.connect(transport);
  await transport.handleRequest(req, res, req.body);
});
app.get("/mcp", async (req, res) => {
  const server = createServer();
  const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true });
  res.on("close", () => { transport.close(); server.close(); });
  await server.connect(transport);
  await transport.handleRequest(req, res);
});

if (process.env.VITEST !== "1") {
  app.listen(PORT, () => console.log(`OpenCommander MCP on http://localhost:${PORT}/mcp diag=${DIAGNOSTIC}`));
}
export { app, createServer };
void ownerFrom;
