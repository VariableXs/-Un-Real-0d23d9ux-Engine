import re
p=r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 尾段增补册（B16–B40 · 500项新功能 · F60301–F60800）.md"
t=open(p,encoding="utf-8").read()
def rep(m):
    return m.group(0)+"（判据锚 "+m.group(1)+"-J1）"
t2=re.sub(r"^(\| UNX-F\d{5}) \| .+ \| 300 \| 增补 \| .+$",rep,t,flags=re.M)
open(p,"w",encoding="utf-8",newline="\n").write(t2)
print("patched rows:",len(re.findall(r"-J1）",t2)))
