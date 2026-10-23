"""Windows-local governance, extension catalog and project metadata for Open.

The credential vault uses DPAPI for the current Windows user. No secret reading
operation is exposed through MCP; plugins cannot execute arbitrary Python.
"""
from __future__ import annotations
from datetime import datetime,timezone
from hashlib import sha256
from pathlib import Path
from collections import Counter,defaultdict
import ctypes, ctypes.wintypes, json, os, re, sys, threading
import developer_suite_core as core

AUDIT_LOCK=threading.RLock()
READ_OPS={
 "ping","capabilities","project_info","tree","find","selection","properties","get_attributes",
 "tags","read_script","script_search","run_state","logs","diagnostics","playtest_status",
 "ui_scan","device_list","device_status","suite_manifest","suite_perf","assert_state",
}
DENIED_OPS={"execute","eval","shell","python","arbitrary_code","arbitrary_exec"}
IRREVERSIBLE={"delete","terrain_clear","insert_asset","place_restore"}
STUDIO_MUTATIONS={"create","update","set_script","replace_script","set_attributes","set_tags",
 "move","duplicate","pivot","terrain_fill","batch","playtest_start","test_run","device_resolution",
 "device_orientation","device_select","test_stop"}
CAPABILITIES={
 "read":{"name":"Project inspection","operations":["suite_scan","suite_dependencies","v7_semantic_search"]},
 "test":{"name":"Verification","operations":["suite_scenario","v7_property_tests","v7_mutation_tests"]},
 "design":{"name":"GUI and world design","operations":["suite_gui","suite_world","suite_theme"]},
 "governance":{"name":"Policy / audit","operations":["v7_action_policy","v7_audit_trail"]},
 "extensions":{"name":"Declarative plugin manifests","operations":["v7_registry","v7_plugin_validate","v7_plugin_register"]},
}
def decision(command):
    if not isinstance(command,dict) or not isinstance(command.get("op"),str):
        raise ValueError("Command must contain operation")
    operation=command["op"]
    if operation in DENIED_OPS:return {"decision":"deny","reason":"Untrusted arbitrary execution is unsupported"}
    if operation in IRREVERSIBLE:
        approved=command.get("confirm_persistent_changes") is True
        return {"decision":"allow" if approved else "require_confirmation",
            "reason":"Destructive or external asset operation needs explicit confirmation"}
    if operation=="batch":
        actions=command.get("actions",[])
        if not isinstance(actions,list) or len(actions)>40:
            return {"decision":"deny","reason":"Invalid batch actions"}
        if any(isinstance(item,dict) and (item.get("op") in IRREVERSIBLE or
            item.get("op")=="batch") for item in actions):
            return {"decision":"allow" if command.get("confirm_persistent_changes") is True
                else "require_confirmation",
                "reason":"Batch contains destructive or nested operations"}
    if operation in STUDIO_MUTATIONS:
        approved=command.get("confirm_persistent_changes") is True
        return {"decision":"allow" if approved else "require_confirmation",
            "reason":"Studio state mutation requires explicit confirmation"}
    if operation.startswith("v7_"):
        if operation=="v7_plugin_invoke":
            return {"decision":"allow" if command.get("approved") is True else "require_confirmation",
                "reason":"Plugin invocation requires user approval, even for read operations"}
        return {"decision":"allow","reason":"Local declarative operation"}
    if operation.startswith("suite_"):
        action=command.get("action","")
        if command.get("apply") is True:
            return {"decision":"allow" if command.get("confirm_persistent_changes") is True else "require_confirmation",
                "reason":"Suite writes to Studio need confirmation"}
        if operation in ("suite_assets",) and action=="insert":
            return {"decision":"require_confirmation","reason":"Asset insertion requires approval"}
        return {"decision":"allow","reason":"Suite local analysis, simulation or controlled action"}
    if operation in READ_OPS:return {"decision":"allow","reason":"Read-only operation"}
    return {"decision":"unknown","reason":"Not classified; inspect operation and use existing bridge safeguards"}

def action_policy(c,studio):
    target=c.get("command")
    if not isinstance(target,dict):raise ValueError("command object is required")
    return {"operation":target.get("op"),**decision(target),
       "enforcement":"v7 plugin invocation and guarded SDK actions; historical Studio bridge operations use their existing safeguards.",
       "doesNotGrantPermission":True}

def _audit_path():
    return core.filename("action_audit",".jsonl")

def audit_append(event):
    with AUDIT_LOCK:
        file=_audit_path()
        if file.exists() and file.stat().st_size>8*1024*1024:
            raise ValueError("Audit log exceeds 8 MB; archive it manually before writing")
        previous="0"*64
        if file.exists():
            with file.open("rb") as stream:
                for line in stream:
                    if line.strip():
                        last=json.loads(line)
                        previous=last["hash"]
        payload={"at":core.utc(),"operation":str(event.get("operation","unknown"))[:75],
            "outcome":str(event.get("outcome","unknown"))[:45],
            "context":str(event.get("context","local"))[:90],
            "previous":previous}
        canonical=json.dumps(payload,sort_keys=True,separators=(",",":"))
        payload["hash"]=sha256(canonical.encode("utf-8")).hexdigest()
        with file.open("a",encoding="utf-8") as stream:
            stream.write(json.dumps(payload,separators=(",",":"))+"\n")
            stream.flush()
            os.fsync(stream.fileno())
        return payload["hash"]

def audit_trail(c,studio):
    action=c.get("action","verify");p=_audit_path()
    if action=="append":
        outcome=c.get("outcome","manual")
        if outcome not in ("approved","denied","failed","completed","manual"):
            raise ValueError("Unknown audit outcome")
        return {"saved":True,"hash":audit_append({"operation":c.get("operation","manual"),
                 "outcome":outcome,"context":c.get("context","local")})}
    if not p.exists():return {"entries":0,"valid":True,"items":[]}
    previous="0"*64;records=[];valid=True
    with p.open(encoding="utf-8") as stream:
        for i,line in enumerate(stream):
            item=json.loads(line)
            proof=item.get("hash")
            expected=sha256(json.dumps({k:v for k,v in item.items() if k!="hash"},
                sort_keys=True,separators=(",",":")).encode()).hexdigest()
            if proof!=expected or item.get("previous")!=previous:valid=False
            previous=proof
            if i<10000:records.append(item)
    return {"entries":len(records),"valid":valid,"items":records[-min(40,max(1,int(c.get("limit",15)))):],
      "limitations":"Local hash chain detects editing but is not an independent signed or immutable audit service."}

class DATA_BLOB(ctypes.Structure):
    _fields_=[("cbData",ctypes.wintypes.DWORD),("pbData",ctypes.POINTER(ctypes.c_ubyte))]
def _blob(data):
    buffer=ctypes.create_string_buffer(data)
    return DATA_BLOB(len(data),ctypes.cast(buffer,ctypes.POINTER(ctypes.c_ubyte))),buffer

def _protect_dpapi(secret):
    if sys.platform!="win32":raise OSError("Windows DPAPI required")
    from ctypes import windll
    crypt=windll.crypt32;kernel=windll.kernel32
    kernel.LocalFree.argtypes=[ctypes.c_void_p]
    kernel.LocalFree.restype=ctypes.c_void_p
    crypt.CryptProtectData.argtypes=[ctypes.POINTER(DATA_BLOB),ctypes.c_wchar_p,
        ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,ctypes.wintypes.DWORD,
        ctypes.POINTER(DATA_BLOB)]
    crypt.CryptProtectData.restype=ctypes.wintypes.BOOL
    crypt.CryptUnprotectData.argtypes=[ctypes.POINTER(DATA_BLOB),ctypes.c_void_p,
        ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,ctypes.wintypes.DWORD,
        ctypes.POINTER(DATA_BLOB)]
    crypt.CryptUnprotectData.restype=ctypes.wintypes.BOOL
    input_blob,owned=_blob(secret)
    output=DATA_BLOB()
    if not crypt.CryptProtectData(ctypes.byref(input_blob),"Open Developer Suite",
        None,None,None,0x1,ctypes.byref(output)):
        raise ctypes.WinError()
    try:return ctypes.string_at(output.pbData,output.cbData)
    finally:kernel.LocalFree(output.pbData)

def _unprotect_dpapi(cipher):
    if sys.platform!="win32":raise OSError("Windows DPAPI required")
    from ctypes import windll
    crypt=windll.crypt32;kernel=windll.kernel32
    kernel.LocalFree.argtypes=[ctypes.c_void_p]
    kernel.LocalFree.restype=ctypes.c_void_p
    input_blob,owned=_blob(cipher);output=DATA_BLOB()
    if not crypt.CryptUnprotectData(ctypes.byref(input_blob),None,None,None,None,0x1,ctypes.byref(output)):
        raise ctypes.WinError()
    try:return ctypes.string_at(output.pbData,output.cbData)
    finally:kernel.LocalFree(output.pbData)

def vault(c,studio):
    name=core.ident(c.get("name","default"))
    action=c.get("action","exists")
    dest=core.filename("vault_"+name,".secret")
    if action=="exists":return {"exists":dest.is_file(),"name":name}
    if action=="delete":
        if c.get("confirm_delete") is not True:raise ValueError("confirm_delete required")
        dest.unlink(missing_ok=True)
        audit_append({"operation":"vault_delete","outcome":"completed","context":name})
        return {"deleted":True,"name":name}
    if action=="put":
        raw=c.get("secret")
        if not isinstance(raw,str) or not 1<=len(raw.encode("utf-8"))<=4096:
            raise ValueError("secret must be 1..4096 UTF-8 bytes")
        if dest.exists() and not c.get("confirm_replace"):
            raise ValueError("Existing secret requires confirm_replace:true")
        cipher=_protect_dpapi(raw.encode("utf-8"))
        tmp=dest.with_suffix(".tmp")
        with tmp.open("wb") as stream:stream.write(cipher)
        tmp.replace(dest)
        audit_append({"operation":"vault_put","outcome":"completed","context":name})
        return {"stored":True,"name":name,"bytes":len(cipher),
            "note":"Windows DPAPI current-user encrypted. No plaintext retrieval operation is exposed."}
    raise ValueError("Vault supports put/exists/delete, never get")

def asset_provenance(c,studio):
    db=core.get("provenance",{})
    action=c.get("action","get")
    if action=="list":return {"items":db}
    name=core.ident(c["name"])
    if action=="get":return {"item":db.get(name)}
    if action=="register":
        required=("asset_id","source_url","owner","license")
        if any(not c.get(k) for k in required):raise ValueError("Asset id, source_url, owner and license are required")
        item={"asset_id":str(c["asset_id"])[:70],"source_url":str(c["source_url"])[:300],
              "owner":str(c["owner"])[:140],"license":str(c["license"])[:140],
              "sha256":str(c.get("sha256",""))[:64],"updated":core.utc()}
        if item["sha256"] and not re.fullmatch("[a-f0-9]{64}",item["sha256"]):
            raise ValueError("Invalid SHA-256")
        db[name]=item;core.put("provenance",db)
        return {"registered":True,"item":item,
          "note":"Provenance is user-submitted metadata, not an independent authenticity verification."}
    raise ValueError("Unknown provenance action")

def multi_place(c,studio):
    data=core.get("multi_place",{})
    action=c.get("action","list")
    if action=="list":return {"places":data}
    name=core.ident(c["name"])
    if action=="add":
        pid=str(c.get("place_id",""))
        if not re.fullmatch("[0-9]{1,20}",pid):raise ValueError("Place ID must be numeric")
        data[name]={"place_id":pid,"role":str(c.get("role",""))[:100],
          "depends_on":[core.ident(x) for x in c.get("depends_on",[])],"updated":core.utc()}
        core.put("multi_place",data)
        return {"saved":True,"name":name}
    if action=="remove":
        if c.get("confirm_remove") is not True:raise ValueError("confirm_remove required")
        data.pop(name,None);core.put("multi_place",data)
        return {"removed":True}
    raise ValueError("Unknown multi-place action")

def teleport(c,studio):
    scenario=c.get("steps",[])
    if not isinstance(scenario,list) or not 1<=len(scenario)<=50:
        raise ValueError("1..50 teleport steps required")
    states={"idle","requested","queued","teleporting","arrived","failed","retry"}
    state="idle";history=[state];attempts=0
    transitions={
        ("idle","request"):"requested",("requested","enqueue"):"queued",
        ("queued","start"):"teleporting",("teleporting","arrive"):"arrived",
        ("teleporting","error"):"failed",("failed","retry"):"retry",
        ("retry","start"):"teleporting",
    }
    for event in scenario:
        key=(state,event)
        if key not in transitions:raise ValueError("Impossible transition "+str(key))
        state=transitions[key];history.append(state)
        if event=="retry":
            attempts+=1
            if attempts>3:raise ValueError("Retry budget exceeded")
    return {"final":state,"history":history,"retries":attempts,
      "scope":"Offline journey state-machine simulation; Roblox TeleportService is not invoked."}

def compatibility(c,studio):
    matrix=core.get("compatibility",{})
    action=c.get("action","get")
    if action=="get":return {"matrix":matrix}
    if action=="set":
        feature=core.ident(c["feature"])
        platform=c.get("platform")
        if platform not in ("desktop","mobile","tablet","console","vr"):
            raise ValueError("Invalid target platform")
        status=c.get("status")
        if status not in ("untested","passed","failed","blocked"):
            raise ValueError("Invalid test status")
        matrix.setdefault(feature,{})[platform]={"status":status,
             "evidence":str(c.get("evidence",""))[:260],"at":core.utc()}
        core.put("compatibility",matrix)
        return {"saved":True,"feature":feature,"platform":platform}
    raise ValueError("Unknown compatibility action")

def resource_budget(c,studio):
    limits=core.get("resource_budget",{})
    action=c.get("action","check")
    name=core.ident(c.get("name","project"))
    if action=="set":
        budget={}
        for k,v in c.get("limits",{}).items():
            if k not in ("objects","scripts","parts","ui","memoryMb"):
                raise ValueError("Budget key unsupported")
            if isinstance(v,bool) or not isinstance(v,(int,float)) or v<0:
                raise ValueError("Budget values must be nonnegative")
            budget[k]=v
        limits[name]=budget;core.put("resource_budget",limits)
        return {"saved":True,"name":name,"limits":budget}
    if action=="check":
        measures=studio({"op":"suite_perf"},timeout=25)
        budget=limits.get(name,{})
        results={k:{"actual":measures.get(k),"limit":v,
            "passed":measures.get(k) is not None and measures[k]<=v}
            for k,v in budget.items()}
        return {"results":results,"passed":bool(budget) and all(v["passed"] for v in results.values()),
          "scope":"Studio object count/memory only; not network throughput or CPU time."}
    raise ValueError("Unknown budget action")

SAFE_EXTENSION_TARGETS={"suite_scan","suite_dependencies","suite_static",
 "suite_security","suite_accessibility","suite_perf","suite_network",
 "suite_release","suite_git","suite_document","v7_semantic_search",
 "v7_code_impact","v7_property_tests","v7_mutation_tests",
 "v7_test_coverage","v7_build_manifest","v7_incremental_build"}

def validate_extension(manifest):
    if not isinstance(manifest,dict):raise ValueError("manifest must be JSON object")
    name=core.ident(manifest.get("name"))
    version=manifest.get("version")
    if not isinstance(version,str) or not re.fullmatch(r"\d+\.\d+\.\d+",version):
        raise ValueError("Version must use x.y.z")
    caps=manifest.get("operations")
    if not isinstance(caps,list) or not 1<=len(caps)<=15:
        raise ValueError("1..15 extension operations required")
    actions={}
    for entry in caps:
        if not isinstance(entry,dict):raise ValueError("Invalid extension operation")
        alias=core.ident(entry.get("alias"))
        target=entry.get("target")
        if target not in SAFE_EXTENSION_TARGETS:raise ValueError("Only approved read-only targets allowed")
        if alias in actions:raise ValueError("Duplicate extension alias")
        actions[alias]=target
    return {"name":name,"version":version,"operations":actions,"permissions":["read"],
      "note":"Declarative aliases only. No custom Python, JS or untrusted arbitrary execution."}

def plugin_validate(c,studio):
    return validate_extension(c.get("manifest"))

def plugin_register(c,studio):
    item=validate_extension(c.get("manifest"))
    name=item["name"]
    current=core.get("extensions",{})
    if name in current and not c.get("confirm_replace"):
        raise ValueError("Extension exists; confirm_replace required")
    current[name]=item;core.put("extensions",current)
    audit_append({"operation":"plugin_register","outcome":"completed","context":name})
    return {"registered":True,"name":name,"version":item["version"],"operations":item["operations"]}

def registry(c,studio):
    extensions=core.get("extensions",{})
    return {"builtins":CAPABILITIES,"extensions":extensions,
      "version":"7.0.0","notes":"Registry discovers declared operations; execution needs explicit approval and stays within safe read-only targets."}

def invoke(c,studio):
    if c.get("approved") is not True:raise ValueError("approved:true required for extension invocation")
    name=core.ident(c.get("name"));alias=core.ident(c.get("alias"))
    extension=core.get("extensions",{})[name]
    target=extension["operations"][alias]
    args=c.get("args",{})
    if not isinstance(args,dict) or len(json.dumps(args))>5000:raise ValueError("Extension args too large")
    import developer_suite_core as coremod
    import developer_suite_workflows as workflows
    import developer_suite_v7_intelligence as intelligence
    handlers={**coremod.HANDLERS,**workflows.HANDLERS,**intelligence.HANDLERS}
    handler=handlers[target]
    audit_append({"operation":"plugin_invoke:"+target,"outcome":"approved","context":name})
    output=handler(dict(args),studio)
    return {"extension":name,"alias":alias,"target":target,"result":output}

HANDLERS={
 "v7_action_policy":action_policy,"v7_audit_trail":audit_trail,"v7_secrets_vault":vault,
 "v7_asset_provenance":asset_provenance,"v7_multi_place":multi_place,
 "v7_teleport_test":teleport,"v7_compatibility":compatibility,
 "v7_resource_budget":resource_budget,"v7_plugin_validate":plugin_validate,
 "v7_plugin_register":plugin_register,"v7_registry":registry,"v7_plugin_invoke":invoke
}
