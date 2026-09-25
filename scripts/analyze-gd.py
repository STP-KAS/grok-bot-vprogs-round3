#!/usr/bin/env python3
"""Analyze a grok-desk-runner log: submitted vs executed moves, latency submit->executed per kind and per
load phase (base 1k TPS / burst :10-:15,:40-:45 / post-burst 5 min), failures by stage/cause.
usage: analyze-gd.py LOG [out.json]"""
import re, sys, json, datetime, collections
TS = re.compile(r'^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+)([+-]\d\d:\d\d)')
def ts(line):
    m = TS.match(line)
    if not m: return None
    return datetime.datetime.fromisoformat(m.group(1) + m.group(2)).timestamp()
def phase(t):
    m = datetime.datetime.fromtimestamp(t).minute
    if 10 <= m < 15 or 40 <= m < 45: return "burst"
    if 15 <= m < 20 or 45 <= m < 50: return "post-burst"
    return "base"
def pct(a, p):
    if not a: return None
    a = sorted(a); return round(a[min(len(a) - 1, int(len(a) * p))], 2)
sub, exe, fails, stats = {}, {}, collections.Counter(), []
run = {}
for line in open(sys.argv[1], errors="replace"):
    t = ts(line)
    if t is None: continue
    if " MOVE tag=" in line:
        d = dict(kv.split("=", 1) for kv in line.split(" MOVE ", 1)[1].split() if "=" in kv)
        sub[int(d["tag"])] = (t, d["kind"], int(d["submit_ms"]), int(d["build_ms"]))
    elif "MOVEFAIL" in line:
        d = line.split("MOVEFAIL", 1)[1]
        st = re.search(r"stage=(\S+)", d).group(1); err = re.search(r"err=(.*)", d)
        e = (err.group(1)[:90] if err else "")
        e = re.sub(r"[0-9a-f]{64}", "<h>", e); e = re.sub(r"\d+", "N", e)
        fails[(st, e)] += 1
    elif "executed: resource_index=" in line:
        h = re.search(r"data=([0-9a-f]+)", line)
        if h and len(h.group(1)) >= 64:
            tag = int.from_bytes(bytes.fromhex(h.group(1)[48:64]), "little")
            exe.setdefault(tag, t)
    elif "RUN started" in line:
        run["started"] = line.strip()[-160:]; run["t0"] = t
    elif "RUN END" in line: run["end"] = line.strip()[-160:]
out = {"run": run, "submitted": len(sub), "executed_tags": len([k for k in sub if k in exe]),
       "fails": {f"{a}|{b}": n for (a, b), n in fails.most_common(20)}, "by_kind": {}, "by_phase": {}}
if sub:
    ts_all = [v[0] for v in sub.values()]; dur = max(ts_all) - min(ts_all)
    out["submit_window_s"] = round(dur, 1); out["submitted_per_s"] = round(len(sub) / max(dur, 1), 2)
    ex_t = sorted(exe[k] for k in sub if k in exe)
    if ex_t: out["executed_per_s"] = round(len(ex_t) / max(ex_t[-1] - ex_t[0], 1), 2)
def summ(keys):
    lat = [exe[k] - sub[k][0] for k in keys if k in exe]
    return {"n": len(keys), "executed": len(lat), "not_executed": len(keys) - len(lat),
            "lat_p50_s": pct(lat, .5), "lat_p90_s": pct(lat, .9), "lat_p99_s": pct(lat, .99), "lat_max_s": pct(lat, 1),
            "submit_ms_p50": pct([sub[k][2] for k in keys], .5), "submit_ms_p99": pct([sub[k][2] for k in keys], .99),
            "build_ms_p50": pct([sub[k][3] for k in keys], .5), "build_ms_p99": pct([sub[k][3] for k in keys], .99)}
for kind in sorted({v[1] for v in sub.values()}):
    out["by_kind"][kind] = summ([k for k, v in sub.items() if v[1] == kind])
for ph in ("base", "burst", "post-burst"):
    ks = [k for k, v in sub.items() if phase(v[0]) == ph and v[1] != "bad"]
    if ks: out["by_phase"][ph] = summ(ks)
out["note"] = "kind=bad is an intentional impossible debit (must never execute). debit/floor/move may be legitimately rejected by the guest floor rule."
s = json.dumps(out, indent=1); print(s)
if len(sys.argv) > 2: open(sys.argv[2], "w").write(s)
