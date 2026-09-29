#!/usr/bin/env python3
import csv
from pathlib import Path
import statistics as st
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
P=Path(__file__).resolve().parent
plt.rcParams.update({'font.size':10,'axes.spines.top':False,'axes.spines.right':False})
def rows(n):return list(csv.DictReader((P/(n+'.csv')).open()))
fig,axes=plt.subplots(1,2,figsize=(12,4.6),layout='constrained')
files=['no-interceptors','depleted','depth-80','depth-120','paired-stock']
labels=['0','30','80','120','200 (stock)']
bottom=[0.0]*5
for key,label,color in [('lrm_hp','LRM','#c99b31'),('srm_hp','SRM','#da5c57'),('beam_hp','Beam','#4385bd')]:
    values=[st.mean(float(r[key]) for r in rows(f)) for f in files]
    axes[0].bar(labels,values,bottom=bottom,label=label,color=color)
    bottom=[a+b for a,b in zip(bottom,values)]
axes[0].set(xlabel='Interceptors per Cruiser',ylabel='Mean combined hull + armour damage',title='What actually causes damage?\n8 duels per fit; both ships combined')
axes[0].legend(frameon=False)
env=rows('envelope');xs=range(4)
conditions=[('SRM',.014),('SRM',.14),('LRM',.14),('LRM',1.4)]
for evade,offset,color in [('false',-.18,'#4385bd'),('true',.18,'#da5c57')]:
    values=[]
    for payload,range_au in conditions:
        rs=[r for r in env if r['payload']==payload and abs(float(r['range_au'])-range_au)<1e-6 and r['evade']==evade and r['closure_kms']=='0' and r['active']=='false']
        values.append(100*sum(r['hit']=='true' for r in rs)/len(rs))
    bars=axes[1].bar([x+offset for x in xs],values,.36,color=color,label='Auto evade' if evade=='true' else 'No evade')
    axes[1].bar_label(bars,fmt='%.1f%%',padding=3,fontsize=9)
axes[1].set(xticks=list(xs),xticklabels=['SRM\n0.014 AU','SRM\n0.14 AU','LRM\n0.14 AU','LRM\n1.4 AU'],ylim=(0,112),ylabel='Hit rate (%)',title='Evasion depends strongly on weapon and range\n256 isolated shots per condition; Frigate target')
axes[1].legend(frameon=False,loc='lower left')
fig.savefig(P/'balance.png',dpi=160)
print('Wrote balance.png')
