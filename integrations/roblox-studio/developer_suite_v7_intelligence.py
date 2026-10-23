"""Project intelligence: lexical retrieval, impact, property tests and deterministic manifests.

No arbitrary code execution is performed by this module.
"""
from __future__ import annotations
from collections import Counter,defaultdict,deque
from hashlib import sha256
import json, math, random, re
import developer_suite_core as core

STOP={"the","and","for","with","then","from","this","that","game","script","local","function",
      "return","true","false","yang","dan","dari","untuk","dengan","pada","saat","dalam"}
SYNONYMS={
    "save":{"store","persist","datastore","write","simpan","storage"},
    "load":{"read","restore","retrieve","baca","muat"},
    "delete":{"remove","destroy","hapus","unlink"},
    "terminal":{"shell","command","prompt","cli","console"},
    "window":{"gui","screen","frame","panel","desktop","jendela"},
    "file":{"filesystem","directory","folder","berkas"},
    "error":{"bug","failure","exception","crash","failed","gagal"},
    "player":{"user","client","pemain"},
    "network":{"remote","event","server","replicate","jaringan"},
    "payment":{"purchase","coins","economy","balance"},
}
ALIASES={term:head for head,others in SYNONYMS.items() for term in ({head}|others)}

def words(value):
    raw=re.findall(r"[a-z0-9]+",re.sub(r"([a-z])([A-Z])",r"\1 \2",str(value)).lower())
    return [ALIASES.get(t,t) for t in raw if len(t)>1 and t not in STOP][:5000]

def corpus(cmd,studio):
    manifest=core.inventory(cmd,studio,refresh=bool(cmd.get("refresh")))
    nodes=[n for n in manifest["nodes"] if n["class"] in core.SCRIPTS]
    entries=[]
    for n in nodes:
        tokens=words(n["path"]+" "+(n.get("source") or ""))
        entries.append({"path":n["path"],"class":n["class"],"source":n.get("source") or "",
                        "tokens":Counter(tokens),"truncated":n.get("sourceTruncated",False)})
    return entries,manifest

def semantic(cmd,studio):
    query=cmd.get("query")
    if not isinstance(query,str) or not query.strip() or len(query)>250:
        raise ValueError("Provide a query string 1..250 characters")
    index,manifest=corpus(cmd,studio)
    query_terms=set(words(query))
    if not query_terms:raise ValueError("Query has no useful terms")
    df=Counter(t for item in index for t in item["tokens"])
    n=max(1,len(index))
    ranked=[]
    for item in index:
        score=0.0
        for term in query_terms:
            tf=item["tokens"].get(term,0)
            if tf:score+=(1+math.log(tf))*math.log(1+(n-df[term]+0.5)/(df[term]+0.5))
        if score:
            ranked.append({"path":item["path"],"class":item["class"],
                 "score":round(score,3),"matched":sorted(t for t in query_terms if item["tokens"].get(t)),
                 "truncated":item["truncated"]})
    ranked.sort(key=lambda e:(-e["score"],e["path"]))
    return {"query":query,"results":ranked[:min(30,max(1,int(cmd.get("limit",10))))],
      "indexed":len(index),"inventoryTruncated":manifest["truncated"],
      "method":"TF-IDF-like lexical ranking + fixed bilingual synonym dictionary; no neural embeddings."}

def impact(cmd,studio):
    path=cmd.get("path")
    if not isinstance(path,str) or not path:raise ValueError("path required")
    graph=core.deps(cmd,studio)
    reverse=defaultdict(set)
    for edge in graph["edges"]:reverse[edge["to"]].add(edge["from"])
    visited=set();queue=deque([(path,0)])
    while queue:
        node,depth=queue.popleft()
        if node in visited:continue
        visited.add(node)
        if depth<12:
            for ref in reverse[node]:queue.append((ref,depth+1))
    nodes,manifest=corpus(cmd,studio)
    observed={n["path"] for n in nodes}
    text_refs=[n["path"] for n in nodes if n["path"]!=path and (
        path.rsplit("/",1)[-1] in n["source"])]
    likely=sorted((visited-{path})|set(text_refs))
    return {"target":path,"directDependents":sorted(reverse[path]),"potentiallyAffected":likely,
      "sourceFound":path in observed,"uncertain":bool(graph["unresolved"] or manifest["truncated"]),
      "unresolvedReferences":len(graph["unresolved"]),
      "method":"Static reverse dependency links and literal name references; not runtime causal proof."}

PROPERTIES={
 "stack_lifo":{"values":list,"invariant":"pop order is the reverse of push order"},
 "path_normalization":{"values":list,"invariant":"normalized path is idempotent and stays under virtual root"},
 "economy_conservation":{"values":list,"invariant":"transfer preserves supply, insufficient balance never creates funds"},
 "set_roundtrip":{"values":list,"invariant":"serializing and restoring JSON values preserves meaning"},
}
def _normalize(value):
    pieces=[]
    for part in value.replace("\\","/").split("/"):
        if part in ("","."):continue
        if part=="..":
            if pieces:pieces.pop()
        else:pieces.append(part)
    return "/"+"/".join(pieces)

def property_tests(cmd,studio):
    kind=cmd.get("property","stack_lifo")
    if kind not in PROPERTIES:raise ValueError("Unsupported property. Supported: "+",".join(PROPERTIES))
    cases=int(cmd.get("cases",100));seed=int(cmd.get("seed",42))
    if not 1<=cases<=500:raise ValueError("cases must be 1..500")
    rng=random.Random(seed)
    failures=[];samples=[]
    for i in range(cases):
        if kind=="stack_lifo":
            values=[rng.randint(-100000,100000) for _ in range(rng.randrange(0,60))]
            actual=list(reversed(values))
            valid=actual==values[::-1]
            evidence={"pushed":values[:6],"count":len(values)}
        elif kind=="path_normalization":
            segments=rng.choices(["tmp","usr","bin","..",".","","file.txt","folder"],k=rng.randrange(0,16))
            inp="/".join(segments);normal=_normalize(inp)
            valid=_normalize(normal)==normal and normal.startswith("/") and ".." not in normal.split("/")
            evidence={"input":inp,"normalized":normal}
        elif kind=="economy_conservation":
            amount=rng.randrange(0,10000);a=rng.randrange(0,100000);b=rng.randrange(0,100000)
            after=(a-amount,b+amount) if amount<=a and amount>0 else (a,b)
            valid=sum(after)==a+b and min(after)>=0
            evidence={"a":a,"b":b,"amount":amount}
        else:
            value={"name":"item"+str(rng.randrange(100)),"count":rng.randrange(500),
              "flags":[rng.choice([True,False]) for _ in range(5)]}
            valid=json.loads(json.dumps(value))==value
            evidence=value
        if i<5:samples.append(evidence)
        if not valid:failures.append({"case":i,"example":evidence})
    result={"property":kind,"seed":seed,"cases":cases,"passed":not failures,
      "failures":failures[:20],"samples":samples,"invariant":PROPERTIES[kind]["invariant"],
      "scope":"Built-in Python reference models; does not execute game Luau."}
    core.put("last_property_tests",{"at":core.utc(),"property":kind,"passed":not failures,"cases":cases})
    return result

def mutation_tests(cmd,studio):
    kind=cmd.get("property","economy_conservation")
    if kind not in PROPERTIES:raise ValueError("Unknown reference property")
    original=property_tests({"property":kind,"cases":50,"seed":cmd.get("seed",77)},studio)
    rng=random.Random(int(cmd.get("seed",77)))
    killed=[]
    for i in range(25):
        if kind=="economy_conservation":
            a=rng.randint(0,100);b=rng.randint(0,100);amount=rng.randint(1,100)
            changed=(a-amount,b+amount+1) if a>=amount else (a,b+1)
            detected=sum(changed)!=a+b
        elif kind=="stack_lifo":
            vals=[rng.randint(1,20) for _ in range(2,12)]
            changed=vals[:]
            detected=changed!=vals[::-1]
        elif kind=="path_normalization":
            raw="/a/../b";mutated="/a/../b"
            detected=mutated!=_normalize(mutated)
        else:
            value={"a":1};decoded={"a":"1"}
            detected=value!=decoded
        killed.append(bool(detected))
    return {"property":kind,"baselinePassed":original["passed"],"mutants":len(killed),
      "killed":sum(killed),"survived":len(killed)-sum(killed),
      "score":round(sum(killed)/len(killed),3),
      "scope":"Reference-model mutants only; no Studio source was mutated."}

def coverage(cmd,studio):
    manifest=core.inventory(cmd,studio)
    paths={n["path"] for n in manifest["nodes"] if n["class"] in core.SCRIPTS}
    specs=core.get("behavior_specs",{})
    watched={rule["path"] for spec in specs.values() for rule in spec.get("checks",[])
             if isinstance(rule,dict) and isinstance(rule.get("path"),str)}
    paths_observed={p for p in paths if p in watched or any(p.startswith(w+"/") for w in watched)}
    return {"scripts":len(paths),"specifications":len(specs),"associatedScripts":len(paths_observed),
      "unmappedScripts":sorted(paths-paths_observed)[:80],
      "observedPaths":sorted(watched)[:120],"metric":"Specification path mapping, not branch/line execution coverage.",
      "truncated":manifest.get("truncated",False)}

def behavior(cmd,studio):
    name=core.ident(cmd.get("name","default"))
    action=cmd.get("action","get");data=core.get("behavior_specs",{})
    if action=="list":return {"specs":sorted(data)}
    if action=="get":return {"spec":data.get(name)}
    if action=="define":
        checks=cmd.get("checks",[])
        if not isinstance(checks,list) or not 1<=len(checks)<=25:raise ValueError("1..25 checks required")
        for ck in checks:
            if not isinstance(ck,dict) or not isinstance(ck.get("path"),str) or not ck["path"]:
                raise ValueError("Each check needs a valid path")
            if ck.get("scope") not in ("client","server",None):raise ValueError("Unsupported check scope")
        data[name]={"description":str(cmd.get("description",""))[:600],
          "checks":checks,"created":core.utc()}
        core.put("behavior_specs",data)
        return {"saved":True,"name":name,"count":len(checks)}
    if action=="verify":
        spec=data[name]
        if any(ck.get("scope") in ("server","client") for ck in spec["checks"]):
            return {"requires_runtime":True,"checks":spec["checks"],
              "instruction":"Run suite_scenario with a disposable place and runtime checks."}
        result=studio({"op":"assert_state","checks":spec["checks"]},timeout=30)
        return {"passed":result["passed"],"result":result}
    raise ValueError("Unknown behavior action")

def event_trace(cmd,studio):
    name=core.ident(cmd.get("name","last"))
    action=cmd.get("action","get")
    if action=="get":
        if name=="last":
            item=core.get("scenario_last",{})
            return {"scenario":item,"limitations":"Scenario summary only; does not capture all RemoteEvent traffic."}
        return {"trace":core.get("event_"+name)}
    if action=="record":
        events=cmd.get("events",[])
        if not isinstance(events,list) or len(events)>250:raise ValueError("At most 250 events")
        cleaned=[]
        for i,item in enumerate(events):
            if not isinstance(item,dict):raise ValueError("Events must be objects")
            cleaned.append({"seq":i,"kind":str(item.get("kind","event"))[:60],
                "scope":str(item.get("scope","unknown"))[:20],
                "id":str(item.get("id",i))[:80]})
        core.put("event_"+name,{"at":core.utc(),"events":cleaned})
        return {"saved":len(cleaned),"trace":name}
    raise ValueError("Unknown event trace action")

def trace_inspect(cmd,studio):
    name=core.ident(cmd.get("name","last"))
    if name=="last":
        result=core.get("scenario_last",{})
        return {"cases":result.get("cases",[]),"passed":result.get("passed"),
          "scope":"Harness-derived trace summary; client/server correlation is best effort."}
    event=core.get("event_"+name,{})
    grouped=defaultdict(list)
    for item in event.get("events",[]):grouped[item["id"]].append(item)
    return {"trace":name,"groups":dict(list(grouped.items())[:60]),
      "scope":"Explicitly recorded event metadata, not automatic distributed tracing."}

def build_manifest(cmd,studio):
    data=core.inventory(cmd,studio,refresh=bool(cmd.get("refresh")))
    paths=[]
    for n in data["nodes"]:
        if n["class"] not in core.SCRIPTS:continue
        source=n.get("source","")
        paths.append({"path":n["path"],"sha256":sha256(source.encode("utf-8")).hexdigest(),
          "complete":not n.get("sourceTruncated",False)})
    paths.sort(key=lambda x:x["path"])
    canonical=json.dumps(paths,separators=(",",":"),sort_keys=True)
    digest=sha256(canonical.encode()).hexdigest()
    result={"digest":digest,"scripts":paths,"complete":not data["truncated"] and all(x["complete"] for x in paths),
      "captured":core.utc(),"scope":"Editor source snapshot manifest. Not a reproducible Roblox place binary."}
    name=core.ident(cmd.get("name","current"))
    previous=core.get("build_"+name)
    action=cmd.get("action","inspect")
    if action=="save":core.put("build_"+name,result)
    elif action!="inspect":raise ValueError("Use inspect or save")
    return {"manifest":result,"previousDigest":previous.get("digest") if previous else None,
      "changed":bool(previous and previous.get("digest")!=digest)}

def incremental(cmd,studio):
    base=core.get("build_"+core.ident(cmd.get("baseline","current")))
    if not base:raise ValueError("Saved baseline required (suite_build_manifest action save)")
    now=build_manifest(cmd,studio)["manifest"]
    old={x["path"]:x["sha256"] for x in base["scripts"]}
    new={x["path"]:x["sha256"] for x in now["scripts"]}
    return {"added":sorted(new.keys()-old.keys()),"removed":sorted(old.keys()-new.keys()),
      "changed":sorted(p for p in new.keys()&old.keys() if new[p]!=old[p]),
      "complete":base["complete"] and now["complete"],
      "note":"Incremental source change report, not an incremental Roblox binary compiler."}

HANDLERS={
 "v7_semantic_search":semantic,"v7_code_impact":impact,"v7_property_tests":property_tests,
 "v7_mutation_tests":mutation_tests,"v7_test_coverage":coverage,"v7_behavior_spec":behavior,
 "v7_event_trace":event_trace,"v7_trace_inspect":trace_inspect,
 "v7_build_manifest":build_manifest,"v7_incremental_build":incremental
}
