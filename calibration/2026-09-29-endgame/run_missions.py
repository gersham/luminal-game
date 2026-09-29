#!/usr/bin/env python3
"""Exercise real transport objectives and Doctrine, without the duel pilot."""
import concurrent.futures
import csv
from pathlib import Path
import re
import subprocess
ROOT=Path(__file__).resolve().parents[2]
OUT=Path(__file__).resolve().parent
(OUT/'missions').mkdir(exist_ok=True)
def run(case):
    cls,seed=case
    result=subprocess.run([str(ROOT/'target/release/luminal-cli'),'--doctrine','12','60',cls,str(seed)],cwd=ROOT,text=True,capture_output=True,check=True)
    (OUT/'missions'/f'{cls}-{seed}.txt').write_text(result.stdout)
    outcome=re.findall(r'OUTCOME (.*)',result.stdout)
    hours=re.findall(r'simulated ([0-9.]+) h',result.stdout)
    return [cls,seed,hours[-1],outcome[-1] if outcome else 'No outcome by 12 hours']
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    results=list(pool.map(run,[(cls,seed) for cls in ('Picket','Frigate','Destroyer','Cruiser','Battleship') for seed in (32000,32001,40000,40001)]))
with (OUT/'missions.csv').open('w') as f:
    w=csv.writer(f);w.writerow(['class','seed','hours','outcome']);w.writerows(results)
print('Finished 20 mission trials')
