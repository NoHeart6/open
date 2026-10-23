"""Interactive editor plans, bounded runtime tests, visual comparisons, and release gates."""
from pathlib import Path
from html import escape
import json, random, time
import developer_suite_core as core

def rgb(v):
    if not isinstance(v,list) or len(v)!=3 or any(type(x)!=int or not 0<=x<=255 for x in v):
        raise ValueError("Color must be [r,g,b] 0..255")
    return {"__type":"Color3","r":v[0],"g":v[1],"b":v[2]}
def u2(v):
    if not isinstance(v,list) or len(v)!=4 or any(not isinstance(x,(int,float)) for x in v):
        raise ValueError("UDim2 requires [xScale,xOffset,yScale,yOffset]")
    return {"__type":"UDim2","xs":v[0],"xo":v[1],"ys":v[2],"yo":v[3]}
def v3(x,y,z):
    return {"__type":"Vector3","x":x,"y":y,"z":z}

def gui(c,studio):
    root=core.ident(c.get("name","OpenInterface"))
    elements=c.get("elements",[])
    if not isinstance(elements,list) or not 1<=len(elements)<=34:raise ValueError("1..34 GUI components required")
    created={"StarterGui/"+root}
    actions=[{"op":"create","parent":"StarterGui","class":"ScreenGui","name":root,
              "properties":{"ResetOnSpawn":False}}]
    for i,node in enumerate(elements):
        kind=node.get("class")
        if kind not in core.GUI-{"ScreenGui"}:raise ValueError("Unsupported GUI class")
        name=core.ident(node.get("name","Element"+str(i)))
        parent=node.get("parent","StarterGui/"+root)
        if parent not in created:raise ValueError("Parent must be earlier in blueprint")
        properties={}
        if kind not in {"UICorner","UIPadding","UIStroke","UIListLayout"}:
            properties={"Size":u2(node.get("size",[0,180,0,48])),
                        "Position":u2(node.get("position",[0,0,0,0]))}
        if kind in {"TextLabel","TextButton","TextBox"}:
            properties["Text"]=str(node.get("text",name))[:180]
            properties["TextSize"]=max(10,min(48,int(node.get("text_size",18))))
        if "background" in node:properties["BackgroundColor3"]=rgb(node["background"])
        if "text_color" in node:properties["TextColor3"]=rgb(node["text_color"])
        if kind=="UICorner":properties["CornerRadius"]={"__type":"UDim","s":0,"o":int(node.get("radius",7))}
        actions.append({"op":"create","parent":parent,"class":kind,"name":name,"properties":properties})
        full=parent+"/"+name
        if full in created:raise ValueError("Duplicate path")
        created.add(full)
    if not c.get("apply"):return {"preview":actions,"count":len(actions),"requires_confirmation":True}
    core.confirm(c)
    result=studio({"op":"batch","actions":actions},timeout=55)
    core.put("last_gui",{"root":"StarterGui/"+root,"at":core.utc(),"count":len(actions)})
    return {"root":"StarterGui/"+root,"created":len(actions),"response":result,
        "warning":"Studio Undo may be required if batch partially fails."}

def theme(c,studio):
    data=core.get("themes",{});action=c.get("action","get")
    name=core.ident(c.get("name","default"))
    if action=="get":return {"theme":data.get(name)}
    if action=="define":
        colors=c.get("colors",{})
        if not isinstance(colors,dict) or not 1<=len(colors)<=15:raise ValueError("1..15 tokens")
        data[name]={core.ident(k):rgb(v) for k,v in colors.items()}
        core.put("themes",data)
        return {"name":name,"tokens":list(data[name])}
    if action=="apply":
        core.confirm(c)
        targets=c.get("assignments",[])
        if not 1<=len(targets)<=35:raise ValueError("1..35 assignments")
        operations=[]
        for item in targets:
            prop=item["property"]
            if prop not in {"BackgroundColor3","TextColor3","ImageColor3","BorderColor3"}:raise ValueError("Not a theme property")
            operations.append({"op":"update","path":item["path"],"properties":{prop:data[name][item["token"]]}})
        return {"response":studio({"op":"batch","actions":operations},timeout=50)}
    raise ValueError("Theme action must be get/define/apply")

def world(c,studio):
    name=core.ident(c.get("name","GeneratedArea"))
    count=max(1,min(34,int(c.get("count",12))))
    cols=max(1,min(10,int(c.get("columns",5))))
    spacing=max(7,min(128,float(c.get("spacing",20))))
    rand=random.Random(int(c.get("seed",1)))
    actions=[{"op":"create","parent":"Workspace","class":"Folder","name":name}]
    for i in range(count):
        height=rand.choice([4,6,9,12])
        x=(i%cols)*spacing+rand.uniform(-2,2)
        z=(i//cols)*spacing+rand.uniform(-2,2)
        actions.append({"op":"create","parent":"Workspace/"+name,"class":"Part",
                        "name":"Structure"+str(i+1).zfill(3),
                        "properties":{"Anchored":True,"Size":v3(6,height,6),
                                      "Position":v3(x,height/2,z)}})
    if not c.get("apply"):return {"preview":actions,"count":count,"seed":c.get("seed",1)}
    core.confirm(c)
    return {"count":count,"root":"Workspace/"+name,
            "response":studio({"op":"batch","actions":actions},timeout=60)}

def scenario(c,studio):
    cases=c.get("cases",[])
    if not isinstance(cases,list) or not 1<=len(cases)<=5:raise ValueError("1..5 cases required")
    core.confirm(c)
    outputs=[]
    for i,case in enumerate(cases):
        checks=case.get("checks")
        if not isinstance(checks,list) or not checks:raise ValueError("Each case requires meaningful checks")
        if len(checks)>25 or len(case.get("steps",[]))>25:raise ValueError("Too many inputs/checks")
        for item in checks:
            if item.get("scope") not in ("client","server"):raise ValueError("Assertions need a runtime scope")
        mode=case.get("mode","play")
        if mode not in ("play","multiplayer"):raise ValueError("Scenario supports play/multiplayer")
        duration=max(5,min(60,int(case.get("duration",12))))
        started=studio({"op":"playtest_start","mode":mode,"players":min(8,max(1,int(case.get("players",2)))),
            "duration":duration,"steps":case.get("steps",[]),"checks":checks,
            "expected_ui_min":int(case.get("expected_ui_min",0)),
            "confirm_persistent_changes":True},timeout=40)
        ident=started.get("id")
        until=time.monotonic()+duration+65
        final=None
        while time.monotonic()<until:
            state=studio({"op":"playtest_status","id":ident},timeout=25)
            if state.get("state") in ("done","failed"):final=state;break
            time.sleep(1)
        outputs.append({"index":i,"id":ident,"passed":bool(final and final.get("state")=="done" and
            (final.get("result") or {}).get("success")),"detail":final})
        if not outputs[-1]["passed"]:break
    passed=all(o["passed"] for o in outputs) and len(outputs)==len(cases)
    core.put("scenario_last",{"passed":passed,"cases":[{"index":o["index"],"passed":o["passed"]} for o in outputs],
         "at":core.utc()})
    return {"passed":passed,"cases":outputs,
        "note":"Runtime telemetry comes from StudioTestService and is bounded; case stop on first failure."}

def performance(c,studio):
    action=c.get("action","sample")
    snap=studio({"op":"suite_perf"},timeout=35)
    if action=="baseline":
        core.put("perf_baseline",snap|{"at":core.utc()})
        return {"baseline":snap}
    baseline=core.get("perf_baseline")
    deltas={key:round(snap[key]-baseline[key],3) for key in ("objects","scripts","parts","ui","memoryMb")
        if baseline and snap.get(key) is not None and baseline.get(key) is not None}
    alerts=[k for k,v in deltas.items() if v>float(c.get("max_delta",50))]
    return {"sample":snap,"deltaFromBaseline":deltas,"alerts":alerts,
        "note":"Studio object/memory snapshot; does not measure FPS, GPU load or live server latency."}

def network(c,studio):
    settings=c.get("profiles",[{"name":"wifi","latency_ms":60,"jitter_ms":15,"loss_percent":1},
               {"name":"mobile","latency_ms":180,"jitter_ms":50,"loss_percent":4}])
    if not isinstance(settings,list) or len(settings)>12:raise ValueError("Too many profiles")
    validated=[]
    for item in settings:
        if any(float(item.get(key,0))<0 or float(item.get(key,0))>bound
          for key,bound in (("latency_ms",2500),("jitter_ms",1000),("loss_percent",100))):
            raise ValueError("Network profile out of range")
        validated.append({"name":core.ident(item["name"]),
          "latency_ms":item["latency_ms"],"jitter_ms":item["jitter_ms"],
          "loss_percent":item["loss_percent"]})
    if c.get("apply"):raise ValueError("Network impairment injection is not exposed by this suite; use Studio Network Simulator.")
    return {"profiles":validated,"automatically_applied":False,
        "note":"Plan-only. Multiplayer playtest uses suite_scenario; latency/jitter/loss need the Studio simulator."}

def visual(c,studio):
    from PIL import Image, ImageChops, ImageStat
    allowed=[Path.home()/n for n in ("Documents","Pictures","Desktop")]
    def safe(path):
        item=Path(path).resolve(strict=True)
        if not item.is_file() or item.suffix.lower() not in {".png",".jpg",".jpeg",".webp"}:
            raise ValueError("Local image required")
        if not any(item.is_relative_to(root.resolve()) for root in allowed):
            raise ValueError("Image must be in Documents, Pictures, or Desktop")
        if item.stat().st_size>16*1024*1024:raise ValueError("Image too large")
        return item
    a=safe(c["before"]);b=safe(c["after"])
    with Image.open(a) as original, Image.open(b) as updated:
        if original.width*original.height>12_000_000 or updated.width*updated.height>12_000_000:
            raise ValueError("Resolution too large")
        if original.size!=updated.size:
            return {"passed":False,"reason":"resolution_mismatch",
                    "before":original.size,"after":updated.size}
        l=original.convert("RGB");r=updated.convert("RGB")
        delta=ImageChops.difference(l,r)
        threshold=max(0,min(255,int(c.get("pixel_threshold",10))))
        mask=delta.convert("L").point(lambda v:255 if v>threshold else 0)
        hist=mask.histogram()
        changed=hist[255]
        count=original.width*original.height
        ratio=changed/count
        output={"passed":ratio<=float(c.get("max_changed_ratio",0.01)),
                "changed_ratio":round(ratio,6),"pixels_changed":changed,
                "total_pixels":count,"bbox":mask.getbbox(),"size":original.size,
                "note":"Pixel-difference comparison; no screenshot capture is performed by this operation."}
        core.put("visual_last",output|{"at":core.utc()})
        return output

def dashboard(c,studio):
    manifest=core.get("inventory",{})
    report=core.get("scenario_last",{})
    plan=core.get("tasks",{})
    mem=core.get("memory",{})
    title=escape(str(c.get("title","Open Studio Developer Dashboard"))[:100])
    rows=[("Capture",str(manifest.get("captured","not captured"))),
          ("Objects",str(manifest.get("total","-"))),("Scripts",str(manifest.get("scripts","-"))),
          ("Remote endpoints",str(manifest.get("remotes","-"))),
          ("GUI objects",str(manifest.get("ui","-"))),
          ("Last scenario",str(report.get("passed","not run"))),
          ("Tasks",str(len(plan))),("Project notes",str(len(mem)))]
    html="<html lang='en'><meta charset='utf-8'><meta name='viewport' content='width=device-width, initial-scale=1'><title>"+title+"</title>"
    html+="<style>body{font-family:system-ui;background:#fafafa;color:#222;margin:3rem auto;max-width:760px;padding:0 1rem}table{border-collapse:collapse;width:100%}td,th{border-bottom:1px solid #ccc;text-align:left;padding:.8rem}caption{text-align:left;margin-bottom:1rem}a:focus{outline:3px solid #166dbc}</style>"
    html+="<h1>"+title+"</h1><p>Local snapshot. Refresh by running suite_dashboard again.</p><table><caption>Project metrics</caption><tr><th scope='col'>Metric</th><th scope='col'>Value</th></tr>"
    for label,value in rows:html+="<tr><th scope='row'>"+escape(label)+"</th><td>"+escape(value)+"</td></tr>"
    html+="</table><p>Generated "+escape(core.utc())+"</p></html>"
    output=core.filename("dashboard",".html")
    output.write_text(html,encoding="utf-8")
    return {"path":str(output),"rows":dict(rows),"note":"Static local HTML, not an internet-facing dashboard."}

def export(c,studio):
    manifest=core.inventory(c,studio,refresh=bool(c.get("refresh")))
    folder=core.STATE/"source_export"
    folder.mkdir(exist_ok=True)
    exported=[]
    for n in manifest["nodes"]:
        if n["class"] not in core.SCRIPTS or not isinstance(n.get("source"),str) or n.get("sourceTruncated"):
            continue
        path=n["path"]
        slug="_".join("".join(ch if ch.isalnum() or ch in "-_" else "_" for ch in part)[:35] for part in path.split("/"))
        suffix=core.sha256(path.encode("utf-8")).hexdigest()[:10]
        file=folder/(slug[:130]+"_"+suffix+".luau")
        content=n["source"]
        if file.exists() and file.read_text(encoding="utf-8")!=content and not c.get("overwrite"):
            exported.append({"source":path,"status":"conflict","file":str(file)});continue
        file.write_text(content,encoding="utf-8")
        exported.append({"source":path,"status":"exported","file":str(file)})
    return {"files":exported[:140],"count":len(exported),
       "note":"Local source export, not automatic Rojo sync. Existing differing files need overwrite:true."}

def release(c,studio):
    scenario_last=core.get("scenario_last")
    inventory=core.get("inventory")
    check={"inventory_scanned":bool(inventory and not inventory.get("truncated")),
      "scenario_passed":bool(scenario_last and scenario_last.get("passed")),
      "explicit_release_approval":bool(c.get("approve_release") is True)}
    return {"ready":all(check.values()),"checks":check,"published":False,
       "note":"Release gate only. This tool never publishes or updates Roblox Cloud without a separately authorized operation."}

def help(c,studio):
    return {"operations":sorted(set(HANDLERS)|set(core.HANDLERS)),
      "note":"Use apply:true and confirm_persistent_changes:true for Studio modifications. All sandbox records stay on the paired PC."}

HANDLERS={"suite_gui":gui,"suite_theme":theme,"suite_world":world,"suite_scenario":scenario,
 "suite_perf":performance,"suite_network":network,"suite_visual":visual,
 "suite_dashboard":dashboard,"suite_export":export,"suite_release":release,"suite_help":help}
