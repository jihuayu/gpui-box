"""Retain every sample from alternating release controls."""
import argparse
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

parser=argparse.ArgumentParser()
parser.add_argument("build",type=Path)
parser.add_argument("--out",type=Path,required=True)
parser.add_argument("--samples",type=int,default=100)
parser.add_argument("--rounds",type=int,default=4)
args=parser.parse_args()
assert args.samples >= 100 and args.rounds >= 2
binaries=json.loads(args.build.read_text(encoding="utf-8"))
for record in binaries.values():
    assert hashlib.sha256(Path(record["path"]).read_bytes()).hexdigest()==record["sha256"]
args.out.mkdir(parents=True,exist_ok=True)
metadata={"utc":time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime()),"platform":platform.platform(),"processor":platform.processor(),"samples":args.samples,"rounds":args.rounds,"executables":binaries}
rows=[]
for round_index in range(args.rounds):
    for variant in (["baseline","candidate"] if round_index%2==0 else ["candidate","baseline"]):
        result=subprocess.run([binaries[variant]["path"]],env=dict(os.environ,GPUI_PLAN_SAMPLES=str(args.samples)),text=True,encoding="utf-8",capture_output=True,check=True)
        assert not result.stderr,result.stderr
        (args.out/f"{round_index+1}-{variant}.csv").write_text(result.stdout,encoding="utf-8")
        values=list(csv.DictReader(io.StringIO(result.stdout)))
        assert len(values)==args.samples*4
        rows.extend({"round":round_index+1,"variant":variant,**row} for row in values)
        print(f"completed round {round_index+1}: {variant}",flush=True)
summary=[]
for tree in ["flex64","flex512"]:
    for operation in ["unchanged_plan","leaf_color"]:
        entry={"tree":tree,"operation":operation}
        for variant in binaries:
            processes=[]
            for round_index in range(args.rounds):
                values=sorted(int(row["nanoseconds"])/1000 for row in rows if row["tree"]==tree and row["operation"]==operation and row["variant"]==variant and row["round"]==round_index+1)
                assert len(values)==args.samples
                percentile=lambda p:values[math.ceil(len(values)*p)-1]
                processes.append({"round":round_index+1,"p50_us":percentile(.5),"p95_us":percentile(.95),"p99_us":percentile(.99),"max_us":values[-1]})
            entry[variant]={"processes":processes,**{f"median_{key}":statistics.median(process[key] for process in processes) for key in ["p50_us","p95_us","p99_us","max_us"]}}
        entry["p50_reduction_percent"]=100*(1-entry["candidate"]["median_p50_us"]/entry["baseline"]["median_p50_us"])
        summary.append(entry)
(args.out/"metadata.json").write_text(json.dumps(metadata,indent=2),encoding="utf-8")
(args.out/"summary.json").write_text(json.dumps(summary,indent=2),encoding="utf-8")
print(json.dumps(summary,indent=2))
