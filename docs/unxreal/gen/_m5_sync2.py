#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""合并 origin/main（幂等续跑）：冲突文件 union 策略，完成后 push 并恢复 stash。"""
import subprocess, os

os.chdir(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main")

def run(*args):
    return subprocess.run(list(args), capture_output=True, text=True, encoding="utf-8", errors="replace")

if not os.path.exists(".git/MERGE_HEAD"):
    print("MERGE 不在进行中，需先发起 merge——中止")
    sys = None
    raise SystemExit(1)

r2 = run("git", "-c", "core.quotepath=false", "diff", "--name-only", "--diff-filter=U")
conflicted = [l.strip() for l in (r2.stdout or "").splitlines() if l.strip()]
print("conflicted:", conflicted)

for f in conflicted:
    s1 = run("git", "show", f":1:{f}")
    s2 = run("git", "show", f":2:{f}")
    s3 = run("git", "show", f":3:{f}")
    base = s1.stdout if s1.returncode == 0 else None
    ours, theirs = s2.stdout, s3.stdout
    if base is None:
        merged = ours.rstrip("\n") + "\n\n---\n\n" + theirs
    else:
        for n, c in (("_mb", base), ("_mo", ours), ("_mt", theirs)):
            with open(n, "w", encoding="utf-8", newline="\n") as fh: fh.write(c)
        m = run("git", "merge-file", "--union", "_mo", "_mb", "_mt")
        with open("_mo", "r", encoding="utf-8") as fh: merged = fh.read()
        os.remove("_mb"); os.remove("_mo"); os.remove("_mt")
    with open(f, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(merged)
    a = run("git", "add", "--", f)
    print("resolved+added:", f, "add rc=", a.returncode)

r3 = run("git", "-c", "core.quotepath=false", "diff", "--name-only", "--diff-filter=U")
assert not (r3.stdout or "").strip(), "仍有未解决冲突"

c = run("git", "commit", "--no-edit")
print("commit rc=", c.returncode, (c.stdout or c.stderr or "").strip()[:150])
p = run("git", "push", "origin", "main")
print("push rc=", p.returncode, (p.stdout or p.stderr or "").strip()[-150:])
rp = run("git", "stash", "pop")
print("stash pop rc=", rp.returncode, (rp.stderr or "").strip()[:150])
st = run("git", "status", "-sb")
print((st.stdout or "").splitlines()[0])
