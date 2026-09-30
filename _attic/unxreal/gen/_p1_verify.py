import re,sys
p=r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 300项新功能增补册（B01–B15 · F60001–F60300）.md"
t=open(p,encoding="utf-8").read()
rows=re.findall(r"^\| (UNX-F\d{5}) \| (.+?) \| (\d+) \| 增补 \|",t,re.M)
ids=[int(r[0][5:]) for r in rows]
a1 = ids==list(range(60001,60301))
batches={}
for _,_,ln in [(r[0],r[1],int(r[2])) for r in rows]:
    pass
rows2=re.findall(r"^\| (UNX-F\d{5}) \| (.+?) \| (\d+) \| 增补 \|",t,re.M)
# batch mapping by position: 20 per batch in order
sums=[sum(int(r[2]) for r in rows2[i*20:(i+1)*20]) for i in range(15)]
a2 = all(s==6000 for s in sums) and len(rows2)==300
names=[r[1] for r in rows2]
a3 = len(set(names))==300
js=re.findall(r"UNX-F(\d{5})-J1",t)
a4 = sorted(set(int(j) for j in js))==ids
anchors={"UNX-F60001":"380","UNX-F60021":"420","UNX-F60041":"300","UNX-F60061":"260","UNX-F60101":"340"}
a5 = all(any(r[0]==k and r[2]==v for r in rows2) for k,v in anchors.items())
print("A1_ID连续零跳号:",a1,"| A2_15批×20条批守恒6000:",a2,sums,"| A3_300主题零重复:",a3,"| A4_判据300唯一:",a4,"| A5_五锚保真:",a5)
sys.exit(0 if all([a1,a2,a3,a4,a5]) else 1)
