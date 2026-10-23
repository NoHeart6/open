"""Local Roblox Studio bridge. Standard Python library only."""
import argparse
from collections import deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parent
TOKEN_FILE = ROOT / "bridge_token.txt"
URL = "http://127.0.0.1:34872"
lock = threading.RLock()
jobs, waiting = {}, deque()
studio = {"last_seen": 0.0, "client": "", "place": ""}
started = time.time()

def token():
    if not TOKEN_FILE.exists():
        raise RuntimeError("Missing token. Run python bridge.py install")
    return TOKEN_FILE.read_text(encoding="utf-8").strip()

def online():
    return time.time() - studio["last_seen"] < 7

def clean_old():
    with lock:
        for jid, job in list(jobs.items()):
            if time.time() - job["created"] > 600:
                del jobs[jid]
        while waiting and waiting[0] not in jobs:
            waiting.popleft()

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, fmt, *args):
        if self.path != "/studio/next" and sys.stderr is not None:
            super().log_message(fmt, *args)
    def respond(self, status, body):
        raw = json.dumps(body, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)
    def authorized(self):
        if not secrets.compare_digest(self.headers.get("X-Bridge-Token", ""), token()):
            self.respond(401, {"error": "Invalid bridge token"})
            return False
        return True
    def read_json(self):
        try:
            size = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            raise ValueError("Invalid length")
        if not 1 <= size <= 1024 * 1024:
            raise ValueError("Request size must be 1 byte to 1 MB")
        data = json.loads(self.rfile.read(size))
        if not isinstance(data, dict):
            raise ValueError("JSON object required")
        return data
    def do_GET(self):
        if not self.authorized():
            return
        clean_old()
        if self.path == "/health":
            with lock:
                self.respond(200, {"server": "ready", "studio_online": online(), "studio_place": studio["place"], "studio_client": studio["client"], "queued": len(waiting), "uptime_seconds": int(time.time() - started)})
            return
        if self.path == "/studio/next":
            client = self.headers.get("X-Bridge-Client", "")[:128]
            if not client:
                self.respond(400, {"error": "Missing client ID"})
                return
            with lock:
                studio.update(last_seen=time.time(), client=client, place=self.headers.get("X-Bridge-Place", "")[:128])
                command = None
                while waiting:
                    jid = waiting.popleft()
                    job = jobs.get(jid)
                    if job and job["state"] == "queued":
                        job.update(state="running", studio_client=client, claimed=time.time())
                        command = {"id": jid, "command": job["command"]}
                        break
            self.respond(200, command or {"id": None})
            return
        if self.path.startswith("/api/jobs/"):
            jid = self.path.split("/")[-1]
            with lock:
                job = jobs.get(jid)
                result = None if not job else {"id": jid, "state": job["state"], "result": job.get("result"), "error": job.get("error")}
            self.respond(200 if job else 404, result or {"error": "Job not found"})
            return
        self.respond(404, {"error": "Unknown endpoint"})
    def do_POST(self):
        if not self.authorized():
            return
        try:
            data = self.read_json()
        except (ValueError, json.JSONDecodeError) as exc:
            self.respond(400, {"error": str(exc)})
            return
        if self.path == "/api/commands":
            command = data.get("command")
            if not isinstance(command, dict) or not isinstance(command.get("op"), str):
                self.respond(400, {"error": "command with string op required"})
                return
            with lock:
                if not online():
                    self.respond(409, {"error": "Roblox Studio plugin offline. Open Studio with a place, allow localhost HTTP, and ensure bridge is running."})
                    return
                if len(waiting) >= 60:
                    self.respond(429, {"error": "Queue full"})
                    return
                jid = uuid.uuid4().hex
                jobs[jid] = {"created": time.time(), "state": "queued", "command": command}
                waiting.append(jid)
            self.respond(202, {"id": jid, "state": "queued"})
            return
        if self.path == "/studio/result":
            client = self.headers.get("X-Bridge-Client", "")
            jid = str(data.get("id", ""))
            with lock:
                job = jobs.get(jid)
                if not job or job["state"] != "running" or job.get("studio_client") != client:
                    self.respond(409, {"error": "Job missing, done, or owned by different Studio"})
                    return
                job["state"] = "done" if data.get("ok") is True else "failed"
                job["result"] = data.get("result")
                job["error"] = data.get("error")
            self.respond(200, {"ok": True})
            return
        self.respond(404, {"error": "Unknown endpoint"})

def request(method, path, data=None, timeout=5):
    headers = {"X-Bridge-Token": token()}
    payload = None
    if data is not None:
        payload = json.dumps(data, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(URL + path, data=payload, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as res:
            return json.loads(res.read())
    except urllib.error.HTTPError as exc:
        try:
            err = json.loads(exc.read())
        except Exception:
            err = {"error": str(exc)}
        raise RuntimeError(err.get("error", str(exc))) from exc

def install():
    if not TOKEN_FILE.exists():
        TOKEN_FILE.write_text(secrets.token_hex(32), encoding="utf-8")
    source = (ROOT / "plugin.template.luau").read_text(encoding="utf-8").replace("__BRIDGE_TOKEN__", token())
    (ROOT / "ChatGPTStudioBridge.luau").write_text(source, encoding="utf-8")
    xml_source = source.replace("]]>", "]]]]><![CDATA[>")
    rbxmx = ('<roblox version="4"><External>null</External><Item class="Script" referent="RBX0"><Properties>'
             '<bool name="Disabled">false</bool><string name="Name">ChatGPT Studio Bridge</string>'
             '<ProtectedString name="Source"><![CDATA[' + xml_source + ']]></ProtectedString>'
             '</Properties></Item></roblox>')
    folder = Path(os.environ["LOCALAPPDATA"]) / "Roblox" / "Plugins"
    folder.mkdir(parents=True, exist_ok=True)
    destination = folder / "ChatGPTStudioBridge.rbxmx"
    destination.write_text(rbxmx, encoding="utf-8")
    print(json.dumps({"installed": True, "plugin": str(destination), "source": str(ROOT / "ChatGPTStudioBridge.luau"), "next": "Restart Roblox Studio, open a place."}, indent=2))

def start():
    try:
        status = request("GET", "/health", timeout=1)
        print(json.dumps({"already_running": True, **status}, indent=2))
        return
    except Exception:
        pass
    log = open(ROOT / "bridge.log", "a", encoding="utf-8")
    p = subprocess.Popen([sys.executable, str(ROOT / "bridge.py"), "serve"], cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                         creationflags=(getattr(subprocess, "DETACHED_PROCESS", 0) | getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)))
    log.close()
    for _ in range(30):
        time.sleep(0.2)
        try:
            status = request("GET", "/health", timeout=0.5)
            print(json.dumps({"started": True, "pid": p.pid, **status}, indent=2))
            return
        except Exception:
            if p.poll() is not None:
                break
    raise RuntimeError("Bridge not started; inspect bridge.log")

def send(args):
    if args.file:
        command = json.loads(Path(args.file).read_text(encoding="utf-8-sig"))
    elif args.json:
        command = json.loads(args.json)
    else:
        command = {"op": args.op}
        if args.path:
            command["path"] = args.path
        if args.root:
            command["root"] = args.root
        if args.depth is not None:
            command["depth"] = args.depth
    def resolve_sources(action):
        if action.get("op") == "set_script" and "source_file" in action:
            supplied = action.pop("source_file")
            if not isinstance(supplied, str):
                raise ValueError("source_file must be a string")
            source_path = (ROOT / supplied).resolve()
            if not source_path.is_relative_to(ROOT):
                raise ValueError("source_file must be inside bridge project directory")
            if source_path.stat().st_size > 800000:
                raise ValueError("Source file exceeds 800 KB")
            action["source"] = source_path.read_text(encoding="utf-8-sig")
        if action.get("op") == "batch":
            for item in action.get("actions", []):
                if isinstance(item, dict):
                    resolve_sources(item)
    resolve_sources(command)
    if not isinstance(command, dict) or not isinstance(command.get("op"), str):
        raise ValueError("Command requires op")
    jid = request("POST", "/api/commands", {"command": command})["id"]
    print("Job ID:", jid, file=sys.stderr)
    if args.no_wait:
        print(json.dumps({"id": jid, "state": "queued"}))
        return
    deadline = time.monotonic() + args.timeout
    while time.monotonic() < deadline:
        status = request("GET", "/api/jobs/" + jid)
        if status["state"] in ("done", "failed"):
            print(json.dumps(status, ensure_ascii=False, indent=2))
            if status["state"] == "failed":
                raise RuntimeError(status.get("error") or "Studio command failed")
            return
        time.sleep(0.3)
    print(json.dumps({"id": jid, "state": "pending", "message": "Inspect via job subcommand"}))
    raise TimeoutError("Response timeout")

def main():
    parser = argparse.ArgumentParser(description="ChatGPT-Roblox Studio local bridge")
    sub = parser.add_subparsers(dest="action", required=True)
    for name in ("install", "start", "serve", "status"):
        sub.add_parser(name)
    job = sub.add_parser("job")
    job.add_argument("id")
    cmd = sub.add_parser("send")
    cmd.add_argument("--file")
    cmd.add_argument("--json")
    cmd.add_argument("--op", default="ping")
    cmd.add_argument("--path")
    cmd.add_argument("--root")
    cmd.add_argument("--depth", type=int)
    cmd.add_argument("--timeout", type=float, default=45)
    cmd.add_argument("--no-wait", action="store_true")
    args = parser.parse_args()
    try:
        if args.action == "install":
            install()
        elif args.action == "start":
            start()
        elif args.action == "serve":
            ThreadingHTTPServer(("127.0.0.1", 34872), Handler).serve_forever(poll_interval=0.25)
        elif args.action == "status":
            print(json.dumps(request("GET", "/health"), indent=2))
        elif args.action == "job":
            print(json.dumps(request("GET", "/api/jobs/" + args.id), indent=2, ensure_ascii=False))
        elif args.action == "send":
            send(args)
    except Exception as exc:
        print("ERROR:", exc, file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
