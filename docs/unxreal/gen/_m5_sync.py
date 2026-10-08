#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""合并 origin/main 到本地 main：冲突文件用 union 策略（纯追加两侧都保留）。"""
import subprocess, os, sys

os.chdir(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main")

def run(*args, **kw):
    return subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", **kw)

r = run("git", "stash", "push", "--include-untracked", "-m", "ai65-sync-tmp")
print("stash rc=", r.returncode)
r = run("git", "merge", "--no-commit", "--no-ff", "origin/main")
out = (r.stdout or "") + (r.stderr or "")
print("merge rc=", r.returncode)

r2 = run("git", "diff", "--name-only", "--diff-filter=U")
conflicted = [l.strip() for l in (r2.stdout or "").splitlines() if l.strip()]
print("conflicted:", conflicted)

for f in conflicted:
    s1 = run("git", "show", f":1:{f}")
    s2 = run("git", "show", f":2:{f}")
    s3 = run("git", "show", f":3:{f}")
    if s1.returncode != 0:  # 无共同基（双侧独立添加）
        base = None
    else:
        base = s1.stdout
    ours, theirs = s2.stdout, s3.stdout
    assert ours is not None and theirs is not None, f
    if base is None:
        merged = ours.rstrip("\n") + "\n\n---\n\n" + theirs
    else:
        with open("_m3_base", "w", encoding="utf-8", newline="\n") as fh: fh.write(base)
        with open("_m3_ours", "w", encoding="utf-8", newline="\n") as fh: fh.write(ours)
        with open("_m3_theirs", "w", encoding="utf-8", newline="\n") as fh: fh.write(theirs)
        m = run("git", "merge-file", "--union", "_m3_ours", "_m3_base", "_m3_theirs")
        with open("_m3_ours", "r", encoding="utf-8") as fh: merged = fh.read()
        os.remove("_m3_base"); os.remove("_m3_ours"); os.remove("_m3_theirs")
    with open(f, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(merged)
    a = run("git", "add", "--", f)
    print("resolved+added:", f, "add rc=", a.returncode)

# 检查是否残留冲突标记（union 模式不应有）
r3 = run("git", "diff", "--name-only", "--diff-filter=U")
assert not (r3.stdout or "").strip(), "仍有未解决冲突"

c = run("git", "commit", "--no-edit")
print("commit rc=", c.returncode, (c.stdout or c.stderr or "").strip()[:200])
p = run("git", "push", "origin", "main")
print("push rc=", p.returncode, (p.stdout or p.stderr or "").strip()[-200:])
rp = run("git", "stash", "pop")
print("stash pop rc=", rp.returncode, (rp.stderr or "").strip()[:200])
st = run("git", "status", "-sb")
print((st.stdout or "").splitlines()[0])
