import json,re,os
d=json.load(open('/tmp/proto58/maps.json'))['data']['maps']
en=json.load(open('/tmp/proto58/maps_en.json'))['data']
ron=open('assets/maps.ron').read()
apps={}
for blk in re.split(r'\n  Map\(\n', ron)[1:]:
    nn=re.search(r'normalizedName: "([^"]*)"',blk).group(1)
    name=re.search(r'\n    name: "([^"]*)"',blk).group(1)
    mm=re.search(r'altMaps: Some\(\[([^\]]*)\]',blk)
    alts=re.findall(r'"([^"]*)"', mm.group(1)) if mm else []
    apps[nn]={'name':name,'alts':alts}
fold={nn:nn for nn in apps}
for nn,a in apps.items():
    for al in a['alts']: fold[al]=nn
byid={m['id']:m['normalizedName'] for m in d.values()}
def R(v): return round(v,2)
out={}
for m in d.values():
    key=fold.get(m['normalizedName'])
    if not key: print('skip',m['normalizedName']); continue
    o=out.setdefault(key,{'boss_spawns':{}, 'sniper_zones':[], 'minefields':[], 'transits':[], 'switches':[], 'btr_stops':[]})
    for b in m['bosses']:
        name=en.get(b['mob'],b['mob']); ch=b['spawnChance']
        for loc in b['spawnLocations']:
            for p in loc['positions']:
                k=(R(p['x']),R(p['z']))
                mobs=o['boss_spawns'].setdefault(k,{})
                mobs[name]=max(mobs.get(name,0),ch)
    for h in m['hazards']:
        ol=[[R(p['x']),R(p['z'])] for p in h['outline']]
        if not ol: continue
        (o['sniper_zones'] if h['hazardType']=='sniper' else o['minefields']).append(ol)
    for t in m['transits']:
        if not t['position']: continue
        tgt=fold.get(byid.get(t['map']))
        o['transits'].append({'position':[R(t['position']['x']),R(t['position']['z'])],'target':apps[tgt]['name'] if tgt else '?'})
    for s in m['switches']:
        if not s['position']: continue
        o['switches'].append({'position':[R(s['position']['x']),R(s['position']['z'])],'name':en.get(s['name'],s['name'])})
    for s in m['btrStops']:
        o['btr_stops'].append({'position':[R(s['x']),R(s['z'])],'name':en.get(s['name'],s['name'])})
for k,o in out.items():
    o['boss_spawns']=[{'position':list(p),'mobs':sorted([{'name':n,'chance':c} for n,c in mobs.items()],key=lambda x:-x['chance'])} for p,mobs in o['boss_spawns'].items()]
    print(k, {f:len(v) for f,v in o.items()})
json.dump(out,open('/tmp/proto58/overlays.json','w'),separators=(',',':'))
print(os.path.getsize('/tmp/proto58/overlays.json'))
