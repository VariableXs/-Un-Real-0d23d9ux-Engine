# -*- coding: utf-8 -*-
# unxreal_e5_deepen_check.py — E5 深化轮六查（体例随 unxreal_e4_deepen_check.py）
import re,sys,io,hashlib,os
sys.stdout=io.TextIOWrapper(sys.stdout.buffer,encoding='utf-8')
BOOK=r'docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md'
ok=True
def chk(name,cond,detail=''):
    global ok
    print(('PASS' if cond else 'FAIL'),name,detail)
    if not cond: ok=False
t=open(BOOK,encoding='utf-8').read()
# 查1 E5 条目 800 与号段连续
rows=re.findall(r'^\| (UNX-F\d+) \| [^|]+ \| (\d+) \| (骨架|已深化) \|',t,re.M)
e5=[(i,int(r),s) for i,r,s in rows if 19201<=int(i[5:])<=20000]
chk('1 条目800',len(e5)==800,len(e5))
chk('1b 号段连续',sorted(int(x[0][5:]) for x in e5)==list(range(19201,20001)))
# 查2 深化翻牌 320
d=sum(1 for _,_,s in e5 if s=='已深化')
chk('2 已深化800（域满深化封账）',d==800,d)
# 查3 域账守恒 240,000
tot=sum(x[1] for x in e5)
chk('3 域账240,000',tot==240000,tot)
# 查4 deepen 册齐备 15 册且每册 20 条 J2
miss=[]
for b in range(2,41):
    p=f'docs/unxreal/deepen/E5-B{b:02d}.md'
    if not os.path.exists(p): miss.append(p); continue
    s=open(p,encoding='utf-8').read()
    n=len(re.findall(r'UNX-F\d+-J2 ',s))
    if n<20: miss.append(f'{p} J2={n}')
chk('4 deepen册39x20',not miss,str(miss[:3]))
# 查5 主册 J2 引用与册内 J2 编号一致
book_j2={x for x in re.findall(r'UNX-F(\d{5})-J2',t) if 19201<=int(x)<=20000}
册_j2=set()
for b in range(2,41):
    册_j2|=set(re.findall(r'UNX-F(\d{5})-J2',open(f'docs/unxreal/deepen/E5-B{b:02d}.md',encoding='utf-8').read()))
chk('5 J2对平',book_j2==册_j2 and len(册_j2)==780,f'book={len(book_j2)}册={len(册_j2)}')
# 查6 快照行与域档行翻牌一致
chk('6 快照E5行',('240,000 | 800 / 0 |' in t) and ('10640 / 9520' in t))
print('RESULT:','ALL PASS' if ok else 'HAS FAIL')
sys.exit(0 if ok else 1)
