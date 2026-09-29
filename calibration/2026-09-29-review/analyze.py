#!/usr/bin/env python3
"""Summarize the raw experiments with binomial intervals and combat measures."""
import collections
import csv
import math
from pathlib import Path
import statistics as st

ROOT=Path(__file__).resolve().parent
lines=[]
def emit(s=''): lines.append(s)
def wilson(h,n):
    z=1.96;p=h/n;d=1+z*z/n
    c=(p+z*z/(2*n))/d
    r=z*math.sqrt(p*(1-p)/n+z*z/(4*n*n))/d
    return f'{100*p:.1f}% [{100*(c-r):.1f}, {100*(c+r):.1f}]'
def rows(name):return list(csv.DictReader((ROOT/(name+'.csv')).open()))
def mean(rs,key):return st.mean(float(r[key]) for r in rs)
emit('# Simulation tables')
emit('\nIntervals are 95% Wilson intervals for independent seed outcomes within each cell.\nThey do not cover model or fixture uncertainty. Damage is hull plus armour HP.\n')
for name,keys in [('envelope',['payload','range_au','closure_kms','evade','active']),('edge',['payload','range_au','closure_kms','evade','active']),('maneuver',['payload','fraction','closure_kms','target_g']),('class-evasion',['class','payload','evade'])]:
    emit('## '+name+'\n')
    emit('| '+' | '.join(keys)+' | n | Hit rate [95% CI] | Mean flight min |')
    emit('| '+ ' | '.join(['---']*(len(keys)+3))+' |')
    groups=collections.defaultdict(list)
    for r in rows(name):groups[tuple(r[k] for k in keys)].append(r)
    for key,rs in groups.items():
        labels=[f'{float(x):g}' if k in ('range_au','fraction') else x for k,x in zip(keys,key)]
        emit('| '+' | '.join(labels)+f' | {len(rs)} | {wilson(sum(r["hit"]=="true" for r in rs),len(rs))} | {mean(rs,"time")/60:.1f} |')
    emit()
emit('## Duels\n')
emit('| Case | Class A | n | A/B wins | Timeouts | Beam finishes | Median min | Mean SRM/LRM hits | Mean SRM/LRM/beam damage |')
emit('| --- | --- | ---: | --- | ---: | ---: | ---: | --- | --- |')
for path in sorted(ROOT.glob('*.csv')):
    if path.stem in ('envelope','edge','maneuver','class-evasion','missions'):continue
    groups=collections.defaultdict(list)
    for r in rows(path.stem):groups[(r['class'],r['depth'])].append(r)
    for (cls,depth),rs in groups.items():
        n=len(rs);a=sum(r['winner']=='0' for r in rs);b=sum(r['winner']=='1' for r in rs);t=sum(r['winner']=='-1' for r in rs);beam=sum(r['beam_finish']=='true' for r in rs)
        emit(f'| {path.stem} (depth {depth}) | {cls} | {n} | {a}/{b} | {t} | {beam} | {st.median(float(r["time"]) for r in rs)/60:.1f} | {mean(rs,"srm_hits"):.1f}/{mean(rs,"lrm_hits"):.1f} | {mean(rs,"srm_hp"):.0f}/{mean(rs,"lrm_hp"):.0f}/{mean(rs,"beam_hp"):.0f} |')
emit('\n## Timeout diagnostics\n')
emit('| Case | Seed | Final AU | Drive A disabled | Drive B disabled |')
emit('| --- | --- | ---: | --- | --- |')
for path in sorted(ROOT.glob('*.csv')):
    for r in rows(path.stem):
        if r.get('winner')=='-1' and 'final_range_au' in r:
            emit(f'| {path.stem} | {r["seed"]} | {float(r["final_range_au"]):.3f} | {r["drive_disabled_a"]} | {r["drive_disabled_b"]} |')
(ROOT/'tables.md').write_text('\n'.join(lines)+'\n')
print('Wrote tables.md')
