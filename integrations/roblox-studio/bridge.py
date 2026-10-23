"""Local Roblox Studio bridge. Standard Python library only."""
import argparse
from collections import deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import re
import shutil
import hashlib
from datetime import datetime, timezone
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


SNAPSHOTS = ROOT / ".snapshots"

def source_hash(source):
    return hashlib.sha256(source.encode("utf-8")).hexdigest()

def snapshot_script_data(data):
    source = data["source"]
    if not isinstance(source, str) or len(source.encode("utf-8")) > 850000:
        raise ValueError("Script source must fit in 850 KB")
    SNAPSHOTS.mkdir(parents=True, exist_ok=True)
    sid = uuid.uuid4().hex
    record = {
        "id": sid,
        "at": datetime.now(timezone.utc).isoformat(),
        "path": data["path"],
        "class": data.get("class", ""),
        "sha256": source_hash(source),
        "source": source,
    }
    dest = SNAPSHOTS / (sid + ".json")
    with dest.open("x", encoding="utf-8") as f:
        json.dump(record, f, ensure_ascii=False)
    return {k: v for k, v in record.items() if k != "source"}

def lookup_snapshot(sid):
    if not isinstance(sid, str) or len(sid)!=32 or any(c not in "0123456789abcdef" for c in sid):
        raise ValueError("Invalid snapshot_id")
    f = SNAPSHOTS / (sid + ".json")
    data = json.loads(f.read_text(encoding="utf-8"))
    if data.get("id") != sid or source_hash(data.get("source", "")) != data.get("sha256"):
        raise ValueError("Backup corrupt or modified")
    return data

def studio_command(command, timeout=45):
    jid = request("POST", "/api/commands", {"command": command})["id"]
    deadline = time.monotonic()+timeout
    while time.monotonic() < deadline:
        result = request("GET", "/api/jobs/" + jid)
        if result["state"]=="done":
            return result["result"]
        if result["state"]=="failed":
            raise RuntimeError(result.get("error") or "Studio command failed")
        time.sleep(0.25)
    raise TimeoutError("Studio command deadline exceeded")

def prepare_script_backups(command):
    # Store the prior source before ANY edit; set expected_source to block races.
    operations = command.get("actions") if command.get("op")=="batch" else [command]
    for item in operations or []:
        if not isinstance(item, dict) or item.get("op") not in ("set_script","replace_script"):
            continue
        path = item.get("path")
        if not path:
            continue
        try:
            current = studio_command({"op": "read_script", "path": path}, timeout=15)
        except RuntimeError as exc:
            if item.get("op")=="set_script" and "Instance not found" in str(exc):
                continue
            # In batches a prior action may create the script. Let Studio handle it.
            if command.get("op")=="batch" and "Instance not found" in str(exc):
                continue
            raise
        previous = current["source"]
        if item.get("expected_source") is not None and item["expected_source"] != previous:
            raise ValueError("Source changed; refusing to overwrite unsaved edits")
        snapshot = snapshot_script_data(current)
        item.setdefault("expected_source", previous)
        print("Backup ID:",snapshot["id"],"for",path,file=sys.stderr)



PLACE_BACKUPS = ROOT / ".place_backups"
PLACE_EXTENSIONS = {".rbxl", ".rbxlx"}

def validate_place_path(filename):
    p = Path(filename).resolve(strict=True)
    user = Path.home().resolve()
    roots = [user / folder for folder in ("Documents", "Desktop", "Pictures")]
    if p.suffix.lower() not in PLACE_EXTENSIONS or not p.is_file():
        raise ValueError("Existing .rbxl or .rbxlx place file required")
    if not any(p.is_relative_to(root.resolve()) for root in roots):
        raise ValueError("Place file must be under Documents, Desktop, or Pictures")
    if p.stat().st_size > 120 * 1024 * 1024:
        raise ValueError("Place exceeds local backup size limit")
    return p

def file_digest(p):
    digest=hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda:f.read(512*1024),b""):
            digest.update(chunk)
    return digest.hexdigest()

def backup_place_file(filename):
    p=validate_place_path(filename)
    PLACE_BACKUPS.mkdir(exist_ok=True)
    uid=uuid.uuid4().hex
    dest=PLACE_BACKUPS / (uid+p.suffix.lower())
    temp=PLACE_BACKUPS / (uid+".partial")
    shutil.copy2(p,temp)
    try:
        expected=file_digest(p)
        if file_digest(temp)!=expected:
            raise IOError("Backup verification mismatch")
        temp.replace(dest)
    finally:
        if temp.exists(): temp.unlink()
    metadata={"id":uid,"at":datetime.now(timezone.utc).isoformat(),
        "source_path":str(p),"sha256":expected,"bytes":dest.stat().st_size,
        "suffix":p.suffix.lower()}
    with (PLACE_BACKUPS/(uid+".json")).open("x",encoding="utf-8") as f:
        json.dump(metadata,f,ensure_ascii=False)
    return metadata | {"message":"Only the saved place file is copied. Unsaved Studio edits are NOT included."}

def read_place_backup(uid):
    if not isinstance(uid,str) or len(uid)!=32 or any(c not in "0123456789abcdef" for c in uid):
        raise ValueError("Invalid place backup ID")
    data=json.loads((PLACE_BACKUPS/(uid+".json")).read_text(encoding="utf-8"))
    if data.get("id")!=uid or data.get("suffix") not in PLACE_EXTENSIONS:
        raise ValueError("Place backup metadata invalid")
    p=PLACE_BACKUPS/(uid+data["suffix"])
    if not p.is_file() or p.stat().st_size!=data["bytes"] or file_digest(p)!=data["sha256"]:
        raise ValueError("Place backup has been modified or corrupted")
    return data,p

def restore_place_copy(uid):
    data,source=read_place_backup(uid)
    outdir=PLACE_BACKUPS/"recovered"
    outdir.mkdir(exist_ok=True)
    output=outdir/(f"{Path(data['source_path']).stem}-{uid[:12]}"+data["suffix"])
    if output.exists(): raise FileExistsError("Recovery output already exists")
    shutil.copy2(source,output)
    if file_digest(output)!=data["sha256"]:
        output.unlink(missing_ok=True)
        raise IOError("Recovered file checksum mismatch")
    return {"recovered_path":str(output),"sha256":data["sha256"],"original_path":data["source_path"],
        "note":"Original file unchanged. Open recovered copy in Studio manually and save it after verification."}

def validate_checks(checks):
    if not isinstance(checks,list) or len(checks)>25:
        raise ValueError("checks must be an array of at most 25 assertions")
    for item in checks:
        if not isinstance(item,dict) or item.get("scope") not in ("client","server"):
            raise ValueError("Each runtime assertion needs scope client or server")
        path=item.get("path")
        if not isinstance(path,str) or len(path)>300 or not path:
            raise ValueError("Invalid assertion path")
        if item.get("property")=="Source":
            raise ValueError("Source assertions are not supported")
    return checks

def repair_verify(command):
    path=command.get("path")
    source=command.get("source")
    expected=command.get("expected_sha256")
    if not isinstance(path,str) or not isinstance(source,str) or not isinstance(expected,str):
        raise ValueError("repair_verify requires path, source, expected_sha256")
    if len(source.encode("utf-8"))>800000 or len(expected)!=64:
        raise ValueError("Invalid source size or expected_sha256")
    checks=validate_checks(command.get("checks",[]))
    if not checks: raise ValueError("repair_verify needs at least one runtime assertion; a launched player alone does not verify the fix")
    mode=command.get("mode","play")
    if mode not in ("play","multiplayer"):
        raise ValueError("repair_verify mode must be play or multiplayer")
    duration=int(command.get("duration",14))
    if not 5<=duration<=90:
        raise ValueError("duration must be from 5 to 90 seconds")
    if command.get("confirm_persistent_changes") is not True:
        raise ValueError("repair_verify requires confirm_persistent_changes=true on a disposable place")
    before=studio_command({"op":"read_script","path":path})
    if source_hash(before["source"])!=expected:
        raise ValueError("Script has changed since expected_sha256; refusing edit")
    saved=snapshot_script_data(before)
    changed=False
    test_id=None
    detail={"backup_id":saved["id"],"path":path}
    try:
        studio_command({"op":"set_script","path":path,"source":source,
                        "expected_source":before["source"]})
        changed=True
        check_source=studio_command({"op":"read_script","path":path})
        if source_hash(check_source["source"])!=source_hash(source):
            raise ValueError("Editor source did not match requested fix")
        started=studio_command({"op":"playtest_start","mode":mode,
            "players":int(command.get("players",2)),"duration":duration,
            "checks":checks,"expected_ui_min":int(command.get("expected_ui_min",0)),
            "confirm_persistent_changes":True})
        test_id=started["id"]
        deadline=time.monotonic()+duration+65
        while time.monotonic()<deadline:
            status=studio_command({"op":"playtest_status","id":test_id},timeout=20)
            if status.get("state") in ("done","failed"):
                detail["test"]=status
                if status.get("state")!="done" or not (status.get("result") or {}).get("success"):
                    raise RuntimeError("Playtest assertions failed")
                detail["state"]="verified"
                detail["applied_sha256"]=source_hash(source)
                return detail
            time.sleep(1.0)
        raise TimeoutError("Playtest result not available; repair left unchanged until safe recovery")
    except Exception as exc:
        detail["error"]=str(exc)
        if changed:
            # Do not modify the editor while a test may still be running.
            if test_id and "test" not in detail:
                detail["state"]="manual_recovery_required"
                return detail
            try:
                current=studio_command({"op":"read_script","path":path},timeout=25)
                if source_hash(current["source"])!=source_hash(source):
                    raise ValueError("Another editor changed source; refusing rollback")
                snapshot_script_data(current)
                studio_command({"op":"set_script","path":path,
                    "source":before["source"],"expected_source":current["source"]},timeout=25)
                detail["state"]="rolled_back"
            except Exception as rollback_error:
                detail["state"]="manual_recovery_required"
                detail["rollback_error"]=str(rollback_error)
        else:
            detail["state"]="rejected"
        return detail


def diagnose_errors(command):
    since=int(command.get("since",0))
    data=studio_command({"op":"logs","since":since,"limit":120},timeout=25)
    errors=[item for item in data.get("logs",[])
            if "error" in str(item.get("severity","")).lower()]
    issues=[]
    pattern=re.compile(r"(?P<path>(?:ServerScriptService|ReplicatedStorage|StarterPlayer|StarterGui|Workspace)[/\w. -]+):(?P<line>\d+):")
    for log in errors[:30]:
        msg=str(log.get("text",""))[:1800]
        found=pattern.search(msg)
        issue={"id":log.get("id"),"message":msg,"severity":log.get("severity"),
               "path":None,"line":None}
        if found:
            location=found.group("path").replace(".", "/").replace("game/","")
            issue["path"]=location
            issue["line"]=int(found.group("line"))
            try:
                source=studio_command({"op":"read_script","path":location},timeout=15)["source"]
                lines=source.splitlines()
                ln=issue["line"]
                if 1<=ln<=len(lines):
                    issue["excerpt"]=[{"line":i+1,"text":lines[i][:220]}
                        for i in range(max(0,ln-3),min(len(lines),ln+2))]
            except (RuntimeError,ValueError,KeyError):
                pass
        issues.append(issue)
    return {"issues":issues,"count":len(issues),"nextSince":data.get("nextSince",since),
        "limitations":"Only LogService messages captured by this plugin; suspected paths are hints, not proof. ChatGPT must inspect context before proposing a patch."}

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
        if action.get("op") in ("set_script","repair_verify") and "source_file" in action:
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
    if not isinstance(command, dict) or not isinstance(command.get("op"), str):
        raise ValueError("Command requires op")
    op=command["op"]
    if op=="diagnose_errors":
        data=diagnose_errors(command)
        print(json.dumps({"state":"done","result":data},ensure_ascii=False,indent=2))
        return
    if op=="place_backup":
        data=backup_place_file(command["path"])
        print(json.dumps({"state":"done","result":data},indent=2))
        return
    if op=="place_backups":
        records=[]
        for f in sorted(PLACE_BACKUPS.glob("*.json"),reverse=True):
            try:
                info=json.loads(f.read_text(encoding="utf-8"))
                if info["id"] and info["suffix"] in PLACE_EXTENSIONS: records.append(info)
            except (ValueError,OSError,KeyError): pass
            if len(records)>=min(max(int(command.get("limit",30)),1),100):break
        print(json.dumps({"state":"done","result":{"backups":records}},indent=2))
        return
    if op=="place_restore":
        data=restore_place_copy(command.get("snapshot_id"))
        print(json.dumps({"state":"done","result":data},indent=2))
        return
    if op=="repair_verify":
        resolve_sources(command)
        result=repair_verify(command)
        print(json.dumps({"state":"done","result":result},ensure_ascii=False,indent=2))
        return
    if op.startswith("suite_"):
        import developer_suite_core as suite_core
        import developer_suite_workflows as suite_workflows
        function=suite_core.HANDLERS.get(op) or suite_workflows.HANDLERS.get(op)
        if not function:
            raise ValueError("Unknown developer suite operation")
        result=function(command,studio_command)
        print(json.dumps({"state":"done","result":result},ensure_ascii=False,indent=2))
        return
    if op=="snapshot_script":
        data=studio_command({"op":"read_script","path":command["path"]})
        print(json.dumps({"state":"done","result":snapshot_script_data(data)},indent=2))
        return
    if op=="list_snapshots":
        files=sorted(SNAPSHOTS.glob("*.json"),key=lambda x:x.name,reverse=True) if SNAPSHOTS.exists() else []
        items=[]
        for f in files:
            try:
                data=json.loads(f.read_text(encoding="utf-8"))
                if not command.get("path") or data.get("path")==command["path"]:
                    items.append({k:v for k,v in data.items() if k!="source"})
            except (ValueError, OSError):
                continue
            if len(items)>=min(int(command.get("limit",40)),100): break
        print(json.dumps({"state":"done","result":{"snapshots":items,"count":len(items)}},indent=2))
        return
    if op=="restore_script":
        snap=lookup_snapshot(command.get("snapshot_id"))
        current=studio_command({"op":"read_script","path":snap["path"]})
        actual=source_hash(current["source"])
        if actual!=command.get("expected_sha256") and command.get("force") is not True:
            raise ValueError("Restore requires matching expected_sha256 or explicit force:true; current source not overwritten")
        snapshot_script_data(current)
        command={"op":"set_script","path":snap["path"],"source":snap["source"],"expected_source":current["source"]}
        # Restore itself is a standard Studio command with a source compare guard.
    else:
        resolve_sources(command)
        prepare_script_backups(command)

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
