import re,sys
p1=r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 300项新功能增补册（B01–B15 · F60001–F60300）.md"
p2=r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 尾段增补册（B16–B40 · 500项新功能 · F60301–F60800）.md"
t1=open(p1,encoding="utf-8").read(); t2=open(p2,encoding="utf-8").read()
r1=re.findall(r"^\| (UNX-F\d{5}) \| (.+?) \| (\d+) \| 增补 \|",t1,re.M)
r2=re.findall(r"^\| (UNX-F\d{5}) \| (.+?) \| (\d+) \| 增补 \|",t2,re.M)
ids=[int(x[0][5:]) for x in r1+r2]
names=[x[1] for x in r1+r2]
sums=[sum(int(r[2]) for r in (r1+r2)[i*20:(i+1)*20]) for i in range(40)]
a1=ids==list(range(60001,60801))
a2=all(s==6000 for s in sums) and len(ids)==800
a3=len(set(names))==800
js=re.findall(r"UNX-F(\d{5})-J1",t2)
a4=sorted(set(int(j) for j in js))==list(range(60301,60801))
print("A1_ID连续零跳号:",a1,"| A2_40批批守恒240000:",a2,"| A3_800主题零重复:",a3,"| A4_尾段判据500唯一:",a4)
sys.exit(0 if all([a1,a2,a3,a4]) else 1)
