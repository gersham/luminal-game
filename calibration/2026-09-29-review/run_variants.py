#!/usr/bin/env python3
"""Run paired review counterfactuals; no production balance settings are changed."""
import concurrent.futures
import csv
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
CLI = ROOT / 'target/release/luminal-cli'
cases = []
def case(name, cls='Cruiser', depth='stock', range_au='.35', **env):
    cases.append(dict(name=name, argv=[str(CLI), '--class-balance', '8', cls, depth, range_au, '24000'],
                      env={'LUMINAL_DUEL_FIRE_RANGE':'envelope'} | {'LUMINAL_DUEL_'+k:v for k,v in env.items()}))
case('paired-stock')
case('legacy-paired', FIRE_RANGE='legacy')
case('depleted', depth='30')
case('depth-80', depth='80')
case('depth-120', depth='120')
case('no-interceptors', depth='0')
case('no-evade', EVADE='off')
case('no-ping', PING='off')
case('natural-tracks', INITIAL_TRACK='off')
case('natural-tracks-no-ping', INITIAL_TRACK='off', PING='off')
case('rush', TACTIC='rush')
case('kite', TACTIC='kite')
case('retreat', TACTIC='retreat')
case('opening-lrm', range_au='1.4')
case('opening-srm', range_au='.1')
case('frigate-destroyer', cls='Frigate', OPPONENT='Destroyer')
case('destroyer-cruiser', cls='Destroyer', OPPONENT='Cruiser')
case('cruiser-battleship', OPPONENT='Battleship')
case('battleship-cruiser', cls='Battleship', OPPONENT='Cruiser')
(OUT / 'variant-manifest.json').write_text(json.dumps(cases, indent=2)+'\n')
def run(c):
    path=OUT / (c['name']+'.csv')
    existing=list(csv.DictReader(path.open())) if path.exists() else []
    count=len(existing)
    assert [int(r['seed']) for r in existing]==list(range(24000,24000+count))
    assert count<=8
    if count<8:
        argv=c['argv'].copy();argv[2]=str(8-count);argv[-1]=str(24000+count)
        result=subprocess.run(argv, cwd=ROOT, env=os.environ | c['env'], capture_output=True, text=True, check=True)
        with path.open('a' if count else 'w') as out:
            out.write('\n'.join(result.stdout.splitlines()[1 if count else 0:])+'\n')
    print('Finished '+c['name'], flush=True)
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    list(pool.map(run, cases))
