# -*- coding: utf-8 -*-
"""AI-80 推送收敛循环 v2：fetch → 隔离未跟踪碰撞件 → merge → 处理备份 → push，直到远端包含 b78aed6e。"""
import os, shutil, subprocess, sys, time

MY = 'b78aed6ebff5d4e9be9e3661cbd0b40858e397f8'
QUAR = '_attic/ai80_merge_quarantine'

def sh(*a, **kw):
    return subprocess.run(a, capture_output=True, text=True, timeout=kw.pop('timeout', 1800), **kw)

def remote_head_via_api():
    try:
        T = subprocess.run(['git', 'credential', 'fill'], input='protocol=https\nhost=github.com\n\n',
                           capture_output=True, text=True, timeout=60).stdout
        tok = [l.split('=', 1)[1] for l in T.splitlines() if l.startswith('password=')][0]
        import urllib.request, json
        r = urllib.request.Request('https://api.github.com/repos/VariableXs/-Un-Real-0d23d9ux-Engine/commits/main',
                                   headers={'Authorization': 'Bearer ' + tok, 'User-Agent': 'ai80-p5'})
        return json.load(urllib.request.urlopen(r, timeout=120))['sha']
    except Exception as e:
        print('api err', e, flush=True)
        return None

def colliding_untracked():
    st = sh('git', '-c', 'core.quotepath=false', 'status', '--porcelain', '--untracked-files=all').stdout.splitlines()
    untracked = [l[3:] for l in st if l.startswith('?? ')]
    if not untracked:
        return []
    tree = sh('git', '-c', 'core.quotepath=false', 'ls-tree', '-r', '--name-only', 'FETCH_HEAD').stdout.splitlines()
    ts = set(tree)
    return [u for u in untracked if u in ts]

def quarantine(files):
    os.makedirs(QUAR, exist_ok=True)
    moved = []
    for f in files:
        f = f.strip('"')
        if not os.path.exists(f):
            continue
        dst = os.path.join(QUAR, f.replace('/', '__'))
        shutil.move(f, dst)
        moved.append((f, dst))
        print('quarantined', f, flush=True)
    return moved

def restore(moved):
    for f, dst in moved:
        same = False
        if os.path.exists(f):
            a = open(f, 'rb').read(); b = open(dst, 'rb').read()
            same = (a == b)
        if not same and not os.path.exists(f):
            shutil.move(dst, f)  # merge 未带来该文件，还原
            print('restored (merge lacked it)', f, flush=True)
        elif same:
            os.remove(dst)
            print('drop backup (identical)', f, flush=True)
        else:
            print('KEEP BACKUP (differs):', dst, '-> remote version at', f, flush=True)

for attempt in range(40):
    rh = remote_head_via_api()
    print(f'[{attempt}] remote={rh[:10] if rh else "?"}', flush=True)
    if rh:
        if sh('git', 'merge-base', '--is-ancestor', MY, rh).returncode == 0:
            print('DONE: my commit on remote', flush=True)
            sys.exit(0)
        if sh('git', 'cat-file', '-t', rh).returncode != 0:
            f = sh('git', 'fetch', 'origin', 'main', timeout=1200)
            print('fetch rc', f.returncode, (f.stderr or '').strip()[-100:], flush=True)
            if f.returncode != 0:
                time.sleep(20); continue
        p = sh('git', 'push', 'origin', 'HEAD:main', timeout=3600)
        out = ((p.stdout or '') + (p.stderr or '')).strip().splitlines()
        print('push rc', p.returncode, out[-1][:140] if out else '', flush=True)
        if p.returncode == 0:
            time.sleep(5); continue
        f = sh('git', 'fetch', 'origin', 'main', timeout=1200)
        if f.returncode != 0:
            print('fetch fail', (f.stderr or '').strip()[-100:], flush=True)
            time.sleep(20); continue
        cols = colliding_untracked()
        moved = quarantine(cols) if cols else []
        m = sh('git', 'merge', '--no-edit', 'FETCH_HEAD', timeout=600)
        print('merge rc', m.returncode, (m.stdout + m.stderr).strip()[-150:], flush=True)
        if m.returncode != 0:
            sh('git', 'merge', '--abort')
            restore(moved)
            print('merge conflict aborted', flush=True)
            time.sleep(15); continue
        restore(moved)
    time.sleep(20)
print('GAVE UP', flush=True)
sys.exit(1)
