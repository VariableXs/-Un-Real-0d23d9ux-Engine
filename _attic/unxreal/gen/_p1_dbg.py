import re
p=r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 300项新功能增补册（B01–B15 · F60001–F60300）.md"
t=open(p,encoding="utf-8").read()
rows=re.findall(r"^\| (UNX-F\d{5}) \| (.+?) \| (\d+) \| 增补 \|",t,re.M)
for i in range(0,300,20):
    print(i//20+1, [int(r[2]) for r in rows[i:i+20]])
