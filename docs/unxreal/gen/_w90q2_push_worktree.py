# -*- coding: utf-8 -*-
"""AI-102 推送收敛：worktree 内三方合并（追加型大文件 union）→ 提交 → push，直到远端含 ab7939d3。
体例承 docs/unxreal/gen/_p5_push_worktree.py（AI-80 判例 v4）。他会话在途工作树产物零触碰。
"""
import os, shutil, subprocess, sys, time, urllib.request, json

ROOT = r'D:\2\14\-Un-Real-0d23d9ux-Engine-main'
WT = os.path.join(ROOT, '_attic', 'ai102_push_worktree')
MY = 'ab7939d3'
BIG = ['docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md',
       'CoRun Varix STAR II · Unxreal · 统一协作总台账.md']
TMP = os.path.join(ROOT, '_attic', 'ai102_merge_tmp')

def sh(*a, cwd=ROOT, timeout=1800):
    return subprocess.run(a, capture_output=True, text=True, cwd=cwd, timeout=timeout,
                          encoding='utf-8', errors='replace')

def api_remote_head():
    for _ in range(3):
        try:
            T = subprocess.run(['git', 'credential', 'fill'],
                               input='protocol=https\nhost=github.com\n\n',
                               capture_output=True, text=True, timeout=60).stdout
            tok = [l.split('=', 1)[1] for l in T.splitlines() if l.startswith('password=')][0]
            r = urllib.request.Request(
                'https://api.github.com/repos/VariableXs/-Un-Real-0d23d9ux-Engine/commits/main',
                headers={'Authorization': 'Bearer ' + tok, 'User-Agent': 'ai102-q2'})
            return json.load(urllib.request.urlopen(r, timeout=120))['sha']
        except Exception as e:
            print('api err', e, flush=True)
            time.sleep(8)
    return None

def union_resolve(fp):
    base = os.path.join(TMP, 'base'); ours = os.path.join(TMP, 'ours'); theirs = os.path.join(TMP, 'theirs')
    with open(base, 'w', encoding='utf-8', newline='') as f:
        f.write(sh('git', 'show', ':1:' + fp, cwd=WT).stdout if sh('git', 'cat-file', '-e', ':1:' + fp, cwd=WT).returncode == 0 else '')
    with open(ours, 'w', encoding='utf-8', newline='') as f:
        f.write(sh('git', 'show', ':2:' + fp, cwd=WT).stdout)
    with open(theirs, 'w', encoding='utf-8', newline='') as f:
        f.write(sh('git', 'show', ':3:' + fp, cwd=WT).stdout)
    r = subprocess.run(['git', 'merge-file', '--union', '-p', ours, base, theirs],
                       capture_output=True, text=True, timeout=600, encoding='utf-8', errors='replace')
    if r.returncode not in (0,):
        return False
    dst = os.path.join(WT, fp)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    with open(dst, 'w', encoding='utf-8', newline='') as f:
        f.write(r.stdout)
    a = sh('git', 'add', '--', fp, cwd=WT, timeout=600)
    return a.returncode == 0

def cleanup():
    if os.path.isdir(WT):
        sh('git', 'merge', '--abort', cwd=WT, timeout=120)
    sh('git', 'worktree', 'remove', '--force', WT, timeout=600)
    if os.path.exists(WT):
        shutil.rmtree(WT, ignore_errors=True)
    sh('git', 'worktree', 'prune', timeout=120)

cleanup()
os.makedirs(TMP, exist_ok=True)
w = sh('git', 'worktree', 'add', '--detach', WT, MY, timeout=900)
print('worktree add rc', w.returncode, (w.stderr or '').strip()[-200:], flush=True)
if w.returncode != 0:
    sys.exit(2)

try:
    for attempt in range(30):
        rh = api_remote_head()
        print(f'[{attempt}] remote={rh[:10] if rh else "?"}', flush=True)
        if not rh:
            time.sleep(25); continue
        if sh('git', 'merge-base', '--is-ancestor', MY, rh, timeout=120).returncode == 0:
            print('DONE: my commit on remote', flush=True)
            sys.exit(0)
        f = sh('git', 'fetch', 'origin', 'main', cwd=WT, timeout=3600)
        print('fetch rc', f.returncode, (f.stderr or '').strip()[-120:], flush=True)
        if f.returncode != 0:
            time.sleep(20); continue
        p = sh('git', 'push', 'origin', 'HEAD:main', cwd=WT, timeout=3600)
        out = ((p.stdout or '') + (p.stderr or '')).strip().splitlines()
        print('push rc', p.returncode, out[-1][:160] if out else '', flush=True)
        if p.returncode == 0:
            time.sleep(5); continue
        base = sh('git', 'merge-base', 'HEAD', 'FETCH_HEAD', cwd=WT, timeout=120).stdout.strip()
        print('merge-base', base[:10] if base else 'NONE', flush=True)
        if not base:
            time.sleep(20); continue
        m = sh('git', 'merge', '--no-commit', 'FETCH_HEAD', cwd=WT, timeout=1200)
        print('merge rc', m.returncode, (m.stdout + m.stderr).strip()[-200:], flush=True)
        if m.returncode == 0:
            c = sh('git', 'commit', '--no-edit', cwd=WT, timeout=600)
            print('commit rc', c.returncode, flush=True)
            continue
        st = sh('git', '-c', 'core.quotepath=false', 'status', '--porcelain', cwd=WT, timeout=120).stdout
        conflicted = [l[3:].strip().strip('"') for l in st.splitlines() if l.startswith(('UU ', 'AA ', 'DU ', 'UD '))]
        print('conflicted:', conflicted, flush=True)
        ok = True
        for fp in conflicted:
            if fp in BIG:
                r = union_resolve(fp)
                print('union', fp, 'rc', r, flush=True)
                if not r:
                    ok = False
            else:
                print('NON-UNION conflict at', fp, flush=True)
                ok = False
        if not ok:
            sh('git', 'merge', '--abort', cwd=WT, timeout=120)
            print('ABORT: non-union conflict, need manual reconcile', flush=True)
            sys.exit(3)
        c = sh('git', 'commit', '--no-edit', cwd=WT, timeout=600)
        print('merge commit rc', c.returncode, (c.stderr or '').strip()[-160:], flush=True)
    print('EXHAUSTED: 30 attempts', flush=True)
    sys.exit(4)
finally:
    pass
