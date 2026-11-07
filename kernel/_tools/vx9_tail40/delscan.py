import sys

def scan(path):
    bstack=[]; pstack=[]; sstack=[]
    in_str=False; in_lc=False
    with open(path, encoding='utf-8') as fh:
        for ln, line in enumerate(fh, 1):
            in_lc=False
            i=0
            while i < len(line):
                ch=line[i]
                if in_str:
                    if ch=='\\':
                        i+=1
                    elif ch=='"':
                        in_str=False
                elif in_lc:
                    break
                elif ch=='"':
                    in_str=True
                elif ch=='/' and i+1<len(line) and line[i+1]=='/':
                    in_lc=True
                elif ch=='{':
                    bstack.append(ln)
                elif ch=='}':
                    if bstack: bstack.pop()
                elif ch=='(':
                    pstack.append(ln)
                elif ch==')':
                    if pstack: pstack.pop()
                elif ch=='[':
                    sstack.append(ln)
                elif ch==']':
                    if sstack: sstack.pop()
                i+=1
    return bstack, pstack, sstack

for p in sys.argv[1:]:
    b,pa,s = scan(p)
    print(p, 'braces', len(b), b[:3], 'parens', len(pa), pa[:3], 'brackets', len(s), s[:3])
