import re
p=r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 300项新功能增补册（B01–B15 · F60001–F60300）.md"
t=open(p,encoding="utf-8").read()
lines=t.split("\n")
rowre=re.compile(r"^(\| UNX-F\d{5} \| .+? \| )(\d+)( \| 增补 \| .*)$")
anchors={"UNX-F60001","UNX-F60021","UNX-F60041","UNX-F60061","UNX-F60101"}
# collect row indices and ids in order
rows=[(i,m.group(1)[:12].strip("| ")) for i,l in enumerate(lines) for m in [rowre.match(l)] if m]
batches=[rows[i*20:(i+1)*20] for i in range(15)]
def multiset_for(batch):
    an=[r for r in batch if r[1] in anchors]
    a=sum(int(rowre.match(lines[r[0]]).group(2)) for r in an)
    rest=20-len(an)
    need=6000-a
    # build multiset: mostly 300s, adjust with 290/280/310 to hit need exactly
    base=need//rest; rem=need-base*rest
    ms=[base]*rest
    # distribute rem by +1 steps: convert to valid values
    ms=[v+ (1 if i<rem else 0) for i,v in enumerate(ms)]
    return ms
for b in batches:
    ms=multiset_for(b); k=0
    for r in b:
        if r[1] in anchors: continue
        m=rowre.match(lines[r[0]])
        lines[r[0]]=m.group(1)+str(ms[k])+m.group(3); k+=1
open(p,"w",encoding="utf-8",newline="\n").write("\n".join(lines))
print("fixed")
