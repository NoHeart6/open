"""Open Studio suite: local project analysis and offline simulators."""
from pathlib import Path
from collections import defaultdict
from datetime import datetime, timezone
from hashlib import sha256
import json, re, subprocess

ROOT=Path(__file__).resolve().parent
STATE=ROOT/".studio_suite"
REPO=Path.home()/"Documents"/"silika"/"open-roblox-release"
VALID=re.compile(r"^[A-Za-z0-9_.-]{1,65}$")
SCRIPTS={"Script","LocalScript","ModuleScript"}
GUI={"ScreenGui","Frame","TextLabel","TextButton","TextBox","ScrollingFrame","ImageLabel","ImageButton","UIPadding","UICorner","UIStroke","UIListLayout"}

def ident(s):
    if not isinstance(s,str) or not VALID.fullmatch(s):raise ValueError("Invalid identifier")
    return s

def filename(name,ext=".json"):
    STATE.mkdir(exist_ok=True)
    return STATE/(ident(name)+ext)

def get(name,default=None):
    f=filename(name)
    return json.loads(f.read_text(encoding="utf-8")) if f.exists() else default

def put(name,value):
    f=filename(name);tmp=f.with_suffix(".tmp")
    tmp.write_text(json.dumps(value,ensure_ascii=False,indent=2),encoding="utf-8")
    tmp.replace(f);return value

def utc():return datetime.now(timezone.utc).isoformat()
def limited(data,n=100):return data[:max(1,min(int(n),200))]
def confirm(cmd):
    if cmd.get("apply") is not True or cmd.get("confirm_persistent_changes") is not True:
        raise ValueError("Studio writes require apply:true and confirm_persistent_changes:true on a disposable place")

def inventory(cmd,studio,refresh=False):
    if refresh or cmd.get("refresh") or get("inventory") is None:
        result=studio({"op":"suite_manifest","limit":min(max(int(cmd.get("limit",700)),1),1000),
            "source_limit":min(max(int(cmd.get("source_limit",5000)),0),12000)},timeout=45)
        put("inventory",result|{"captured":utc()})
    return get("inventory")

def scan(c,studio):
    x=inventory(c,studio,True)
    return {"nodes":x["total"],"scripts":x["scripts"],"remotes":x["remotes"],"ui":x["ui"],
      "truncated":x["truncated"],"captured":x["captured"],
      "items":limited([{k:v for k,v in n.items() if k!="source"} for n in x["nodes"]],c.get("preview_limit",60))}

def deps(c,studio):
    x=inventory(c,studio);paths={n["path"] for n in x["nodes"] if n["class"] in SCRIPTS}
    links=[];unresolved=[]
    for n in x["nodes"]:
        if n["class"] not in SCRIPTS:continue
        for m in re.finditer(r"\brequire\s*\(\s*([^)\n]{1,160})\s*\)",n.get("source","")):
            ref=m.group(1).strip();p=None
            if ref.startswith("script.Parent."):
                p=n["path"].rsplit("/",1)[0]+"/"+ref.split(".")[-1]
            elif ref.startswith("game."):p=ref[5:].replace(".","/")
            if p in paths:links.append({"from":n["path"],"to":p})
            else:unresolved.append({"script":n["path"],"reference":ref})
    graph=defaultdict(list)
    for e in links:graph[e["from"]].append(e["to"])
    colors={};stack=[];cycles=[]
    def visit(node):
        colors[node]=1;stack.append(node)
        for to in graph[node]:
            if colors.get(to)==1:cycles.append(stack[stack.index(to):]+[to])
            elif not colors.get(to):visit(to)
        stack.pop();colors[node]=2
    for p in paths:
        if not colors.get(p):visit(p)
    return {"scripts":len(paths),"resolved":len(links),"edges":limited(links),
        "unresolved":limited(unresolved,60),"cycles":cycles[:20],"truncated":x["truncated"],
        "note":"Regex-derived graph; dynamic require and aliased modules are not reliably resolved."}

RULES=[("dynamic-eval",r"\b(loadstring|setfenv|getfenv)\s*\(","Review dynamic code execution","high"),
 ("busy-loop",r"while\s+true\s+do","Check loop yielding/cancellation","medium"),
 ("legacy-wait",r"(?<!task\.)\bwait\s*\(","Prefer explicit task scheduler","low"),
 ("remote-handler",r"\.OnServerEvent\s*:\s*Connect","Verify input validation and authorization","high")]
def lint(c,studio):
    x=inventory(c,studio);findings=[]
    for n in x["nodes"]:
        if n["class"] not in SCRIPTS:continue
        src=n.get("source","")
        for rid,pat,msg,level in RULES:
            for match in list(re.finditer(pat,src))[:4]:
                findings.append({"rule":rid,"severity":level,"path":n["path"],
                 "line":src.count("\n",0,match.start())+1,"recommendation":msg})
        if n["class"]=="LocalScript" and "DataStoreService" in src:
            findings.append({"rule":"client-data-store","severity":"high","path":n["path"]})
        if n.get("sourceTruncated"):findings.append({"rule":"truncated-source","path":n["path"]})
    return {"findings":limited(findings,c.get("max_results",100)),
      "scripts":x["scripts"],"truncated":x["truncated"],"note":"Heuristic review, not a Luau compiler or formal security audit."}

def security(c,studio):
    x=inventory(c,studio);findings=[];remotes=[]
    for n in x["nodes"]:
        if n["class"] in {"RemoteEvent","RemoteFunction"}:remotes.append(n["path"])
        if n["class"] not in SCRIPTS:continue
        src=n.get("source","")
        if "OnServerEvent" in src and not any(k in src for k in ("typeof(","type(","IsA(","math.clamp(","FindFirstChild(")):
            findings.append({"path":n["path"],"severity":"review","reason":"Remote handler lacks visible validation markers"})
        if "HttpService" in src and "PostAsync" in src:
            findings.append({"path":n["path"],"severity":"review","reason":"Review external HTTP transmissions"})
    return {"remotes":limited(remotes),"findings":limited(findings),
     "truncated":x["truncated"],"note":"Patterns are not proof of safety or vulnerability."}

def accessibility(c,studio):
    x=inventory(c,studio);issues=[];strings=[]
    for n in x["nodes"]:
        if n["class"] not in GUI:continue
        if n["class"] in {"TextButton","ImageButton"} and n.get("sizeX",0)<44:
            issues.append({"path":n["path"],"problem":"Touch target narrower than 44px"})
        if n["class"] in {"TextButton","TextLabel","TextBox"} and n.get("textSize",20)<14:
            issues.append({"path":n["path"],"problem":"Small text"})
        if n.get("text"):strings.append({"path":n["path"],"text":n["text"]})
    languages=get("translations",{})
    if c.get("action")=="translate":
        lang=ident(c["language"]);key=ident(c["key"])
        languages.setdefault(lang,{})[key]=str(c["text"])[:400]
        put("translations",languages)
    return {"issues":limited(issues),"strings":limited(strings),
      "locales":list(languages),"note":"Editor viewport only; contrast and assistive behavior require actual device tests."}

def memory(c,studio):
    db=get("memory",{});key=ident(c.get("key","project"));action=c.get("action","get")
    if action=="list":return {"entries":db}
    if action=="get":return {"key":key,"value":db.get(key)}
    value=c.get("value")
    if len(json.dumps(value,ensure_ascii=False))>8000:raise ValueError("Entry too large")
    if action=="put":db[key]={"value":value,"updated":utc()}
    elif action=="append":
        old=db.get(key,{}).get("value",[])
        if not isinstance(old,list):raise ValueError("Entry must be a list")
        db[key]={"value":(old+[value])[-100:],"updated":utc()}
    else:raise ValueError("action: get/list/put/append")
    put("memory",db);return {"saved":True,"key":key}

def sandbox(c,studio):
    namespace=ident(c.get("namespace","test"))
    if namespace.lower()=="production":raise ValueError("Production namespace blocked")
    db=get("sandbox_"+namespace,{"schema_version":1,"values":{},"history":[]})
    action=c.get("action","get")
    if action=="get":return {"namespace":namespace,"schema_version":db["schema_version"],
      "values":db["values"].get(ident(c["key"])) if c.get("key") else db["values"]}
    if action=="put":
        k=ident(c["key"]);v=c.get("value")
        if len(json.dumps(v,ensure_ascii=False))>10000:raise ValueError("Value exceeds 10KB")
        if len(db["values"])>=500 and k not in db["values"]:raise ValueError("Sandbox full")
        db["values"][k]=v
    elif action=="migrate":
        if int(c["version"])!=db["schema_version"]+1:raise ValueError("Only next schema version allowed")
        for old,new in c.get("renames",{}).items():
            old=ident(old);new=ident(new)
            if new in db["values"]:raise ValueError("Key collision")
            if old in db["values"]:db["values"][new]=db["values"].pop(old)
        db["schema_version"]+=1
    elif action=="reset":
        if not c.get("confirm_reset"):raise ValueError("confirm_reset required")
        db={"schema_version":1,"values":{},"history":[]}
    else:raise ValueError("action: get/put/migrate/reset")
    db["history"]=(db["history"]+[{"at":utc(),"action":action}])[-100:]
    put("sandbox_"+namespace,db)
    return {"saved":True,"namespace":namespace,"keys":len(db["values"]),
        "schema_version":db["schema_version"],"note":"Local JSON sandbox. Never accesses Roblox DataStore."}

def economy(c,studio):
    balance={ident(k):int(v) for k,v in c.get("initial",{}).items()}
    if any(v<0 for v in balance.values()):raise ValueError("Negative balance")
    events=c.get("events",[])
    if not isinstance(events,list) or len(events)>150:raise ValueError("Max 150 transactions")
    supply=sum(balance.values());seen=set();ledger=[];reject=[]
    for idx,e in enumerate(events):
        tid=ident(str(e.get("txid",idx)))
        if tid in seen:reject.append({"index":idx,"reason":"duplicate txid"});continue
        kind=e.get("kind");amount=e.get("amount")
        if type(amount)!=int or amount<=0:reject.append({"index":idx,"reason":"invalid amount"});continue
        source=ident(e.get("from","bank"));dest=ident(e.get("to","bank"))
        if kind in ("transfer","debit") and (balance.get(source,0)<amount or (kind=="transfer" and source==dest)):
            reject.append({"index":idx,"reason":"insufficient funds or self-transfer"});continue
        if kind=="credit":balance[dest]=balance.get(dest,0)+amount;supply+=amount
        elif kind=="debit":balance[source]-=amount;supply-=amount
        elif kind=="transfer":
            balance[source]-=amount;balance[dest]=balance.get(dest,0)+amount
        else:reject.append({"index":idx,"reason":"unknown transaction kind"});continue
        seen.add(tid);ledger.append({"txid":tid,"kind":kind,"amount":amount})
    assert sum(balance.values())==supply
    return {"balances":balance,"applied":len(ledger),"rejected":reject,"ledger":limited(ledger),
      "invariant":"balanced supply","note":"Synthetic balances only, not Roblox commerce."}

def contracts(c,studio):
    name=ident(c.get("name","api"));db=get("contracts",{})
    action=c.get("action","get")
    if action=="get":return {"contract":db.get(name)}
    schema=c.get("fields",{})
    if not isinstance(schema,dict) or len(schema)>100:raise ValueError("Max 100 contract fields")
    for k,v in schema.items():
        ident(k)
        if not isinstance(v,dict) or v.get("type") not in ("string","number","boolean","table"):
            raise ValueError("Contract field type unsupported")
    if action=="define":
        old=db.get(name,{})
        breaking=[k for k,v in old.get("fields",{}).items() if k not in schema or v.get("type")!=schema[k].get("type") or (not v.get("required") and schema[k].get("required"))]
        if breaking and c.get("approve_breaking") is not True:return {"saved":False,"breaking":breaking}
        db[name]={"version":old.get("version",0)+1,"fields":schema,"updated":utc()}
        put("contracts",db)
        return {"saved":True,"breaking":breaking,"contract":db[name]}
    if action=="validate":
        item=c.get("data",{})
        if not isinstance(item,dict):raise ValueError("data must be a JSON object")
        problems=[];types={"string":str,"number":(int,float),"boolean":bool,"table":(dict,list)}
        for k,v in schema.items():
            if v.get("required") and k not in item:problems.append("missing "+k)
            if k in item and (not isinstance(item[k],types[v["type"]]) or (v["type"]=="number" and type(item[k])==bool)):
                problems.append("wrong type "+k)
        return {"passed":not problems,"issues":problems}
    raise ValueError("action: get/define/validate")

def npc(c,studio):
    states=[ident(i) for i in c.get("states",[])]
    if not states or len(set(states))!=len(states) or len(states)>35:raise ValueError("1..35 unique states required")
    start=c.get("start",states[0])
    if start not in states:raise ValueError("Invalid start state")
    trans={}
    for e in c.get("transitions",[]):
        a=ident(e["from"]);k=ident(e["event"]);b=ident(e["to"])
        if a not in states or b not in states or (a,k) in trans:raise ValueError("Invalid or conflicting transition")
        trans[(a,k)]=b
    reached={start};todo=[start]
    while todo:
        a=todo.pop()
        for (s,_),b in trans.items():
            if s==a and b not in reached:reached.add(b);todo.append(b)
    current=start;history=[start]
    for event in c.get("events",[])[:150]:
        current=trans.get((current,event),current);history.append(current)
    return {"state":current,"history":history,"unreachable":sorted(set(states)-reached),
      "transitions":len(trans),"note":"Offline finite-state machine simulator; NPC is not edited."}

def planner(c,studio):
    tasks=get("tasks",{});action=c.get("action","list")
    if action=="add":
        tid=ident(c["id"]);deps=[ident(k) for k in c.get("depends_on",[])]
        if tid in tasks or any(k not in tasks for k in deps):raise ValueError("Duplicate task or missing dependency")
        tasks[tid]={"description":str(c.get("description",""))[:300],"depends_on":deps,"done":False}
        put("tasks",tasks)
    elif action=="complete":
        tid=ident(c["id"])
        if any(not tasks[k]["done"] for k in tasks[tid]["depends_on"]):raise ValueError("Dependent task incomplete")
        tasks[tid]["done"]=True;put("tasks",tasks)
    elif action!="list":raise ValueError("action: add/complete/list")
    available=[k for k,v in tasks.items() if not v["done"] and all(tasks[d]["done"] for d in v["depends_on"])]
    return {"tasks":tasks,"ready":available}

def assets(c,studio):
    db=get("assets",{});action=c.get("action","list")
    if action=="list":return {"catalog":db}
    name=ident(c["name"])
    if action=="add":
        asset_id=int(c["asset_id"])
        if asset_id<=0:raise ValueError("Invalid asset ID")
        db[name]={"id":asset_id,"license":str(c.get("license","unverified"))[:120],"source":str(c.get("source",""))[:160]}
        put("assets",db)
        return {"saved":True,"name":name,"note":"License value is supplied by the user and is not verified by this tool."}
    if action=="insert":
        confirm(c)
        if db[name]["license"]=="unverified":raise ValueError("Record asset rights before inserting")
        return studio({"op":"insert_asset","asset_id":db[name]["id"],"parent":c.get("parent","Workspace"),
                       "name":c.get("instance_name",name)},timeout=55)
    raise ValueError("action: list/add/insert")

def timeline(c,studio):
    tracks=c.get("tracks",[]);length=float(c.get("duration",10))
    if not isinstance(tracks,list) or not 1<=len(tracks)<=30 or not 0<length<=240:raise ValueError("Invalid timeline")
    for tr in tracks:
        if tr.get("kind") not in {"event","animation","audio","tween"} or not 0<=float(tr.get("at",0))<=length:
            raise ValueError("Invalid track or timestamp")
    name=ident(c.get("name","sequence"))
    spec={"name":name,"duration":length,"tracks":tracks,"updated":utc()}
    put("timeline_"+name,spec)
    return {"plan":spec,"note":"Serialized event/animation/audio plan; playback requires a project script."}

def git(c,studio):
    if not REPO.is_dir():raise FileNotFoundError("Git checkout unavailable")
    def run(*args):
        result=subprocess.run(["git",*args],cwd=REPO,capture_output=True,text=True,timeout=15)
        if result.returncode:raise RuntimeError(result.stderr[:250])
        return result.stdout.strip()
    return {"head":run("log","-1","--oneline"),"branch":run("branch","--show-current"),
        "status":run("status","--short")[:2500],
        "note":"Read-only. Sync, commit and push require explicit approval."}

def docs(c,studio):
    x=inventory(c,studio);d=deps({},studio)
    name=ident(c.get("name","project_overview"))
    lines=["# Roblox project analysis","", "Generated: "+utc(), "",
        "Nodes: "+str(x["total"])+" | Scripts: "+str(x["scripts"])+" | GUI: "+str(x["ui"]), "",
        "## Scripts"]
    for n in x["nodes"]:
        if n["class"] in SCRIPTS:lines.append("- "+n["path"]+" ("+n["class"]+")")
    lines.extend(["","## Dependencies"])
    for item in d["edges"]:lines.append("- "+item["from"]+" -> "+item["to"])
    lines.extend(["","## Limitations","- Snapshot may be incomplete.","- No production data or unpublished assets inspected.",""])
    p=filename(name,".md");p.write_text("\n".join(lines),encoding="utf-8")
    return {"file":str(p),"scripts":x["scripts"],"edges":d["resolved"]}

HANDLERS={"suite_scan":scan,"suite_dependencies":deps,"suite_static":lint,"suite_security":security,
 "suite_accessibility":accessibility,"suite_memory":memory,"suite_data":sandbox,"suite_economy":economy,
 "suite_contracts":contracts,"suite_npc":npc,"suite_plan":planner,"suite_assets":assets,
 "suite_timeline":timeline,"suite_git":git,"suite_document":docs}
