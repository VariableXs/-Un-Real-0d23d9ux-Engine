# -*- coding: utf-8 -*-
"""AI-80 推送收敛 v4：worktree 内三方 union 合并 master/台账等追加型大文件 → 提交 → push，直到远端含 b78aed6e。"""
import os, shutil, subprocess, sys, time, urllib.request, json

ROOT = r'D:\2\14\-Un-Real-0d23d9ux-Engine-main'
WT = os.path.join(ROOT, '_attic', 'ai80_push_worktree')
MY = 'b78aed6ebff5d4e9be9e3661cbd0b40858e397f8'
BIG = ['docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md',
       'CoRun Varix STAR II · Unxreal · 统一协作总台账.md']

def sh(*a, cwd=ROOT, timeout=1800):
    return subprocess.run(a, capture_output=True, text=True, cwd=cwd, timeout=timeout, encoding='utf-8', errors='replace')

def api_remote_head():
    for _ in range(3):
        try:
            T = subprocess.run(['git', 'credential', 'fill'], input='protocol=https\nhost=github.com\n\n',
                               capture_output=True, text=True, timeout=60).stdout
            tok = [l.split('=', 1)[1] for l in T.splitlines() if l.startswith('password=')][0]
            r = urllib.request.Request('https://api.github.com/repos/VariableXs/-Un-Real-0d23d9ux-Engine/commits/main',
                                       headers={'Authorization': 'Bearer ' + tok, 'User-Agent': 'ai80-p5'})
            return json.load(urllib.request.urlopen(r, timeout=120))['sha']
        except Exception as e:
            print('api err', e, flush=True)
            time.sleep(8)
    return None

def cleanup():
    if os.path.isdir(WT):
        sh('git', 'merge', '--abort', cwd=WT, timeout=120)
    sh('git', 'worktree', 'remove', '--force', WT, timeout=300)
    if os.path.exists(WT):
        shutil.rmtree(WT, ignore_errors=True)
    sh('git', 'worktree', 'prune', timeout=120)

cleanup()
w = sh('git', 'worktree', 'add', '--detach', WT, 'HEAD', timeout=900)
print('worktree add rc', w.returncode, flush=True)
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
        # fetch（worktree 内共享对象库）
        f = sh('git', 'fetch', 'origin', 'main', cwd=WT, timeout=1800)
        print('fetch rc', f.returncode, (f.stderr or '').strip()[-100:], flush=True)
        if f.returncode != 0:
            time.sleep(20); continue
        # 快进检查：远端是否已是本地祖先 → 直接 push
        p = sh('git', 'push', 'origin', 'HEAD:main', cwd=WT, timeout=3600)
        out = ((p.stdout or '') + (p.stderr or '')).strip().splitlines()
        print('push rc', p.returncode, out[-1][:120] if out else '', flush=True)
        if p.returncode == 0:
            time.sleep(5); continue
        # 三方 union 合并冲突文件
        base = sh('git', 'merge-base', 'HEAD', 'FETCH_HEAD', cwd=WT, timeout=120).stdout.strip()
        print('merge-base', base[:10] if base else 'NONE', flush=True)
        if not base:
            time.sleep(20); continue
        m = sh('git', 'merge', '--no-commit', 'FETCH_HEAD', cwd=WT, timeout=900)
        print('merge rc', m.returncode, (m.stdout + m.stderr).strip()[-150:], flush=True)
        if m.returncode == 0:
            c = sh('git', 'commit', '--no-edit', cwd=WT, timeout=300)
            print('commit rc', c.returncode, flush=True)
            continue
        # 冲突路径清单
        st = sh('git', '-c', 'core.quotepath=false', 'status', '--porcelain', cwd=WT, timeout=120).stdout
        conflicted = [l[3:].strip().strip('"') for l in st.splitlines() if l.startswith(('UU ', 'AA ', 'DU ', 'UD '))]
        print('conflicted:', conflicted, flush=True)
        for fp in conflicted:
            if fp not in BIG:
                print('NON-UNION conflict at', fp, flush=True)
                sh('git', 'merge', '--abort', cwd=WT, timeout=120)
                sys.exit(3)
        for fp in BIG:
            if fp not in conflicted:
                continue
            ours = sh('git', 'show', f'HEAD:{fp}', cwd=WT, timeout=300).stdout
            theirs = sh('git', 'show', f'FETCH_HEAD:{fp}', cwd=WT, timeout=300).stdout
            baset = sh('git', 'show', f'{base}:{fp}', cwd=WT, timeout=300).stdout
            p_ours, p_base, p_theirs = [os.path.join(WT, f'_merge_{n}.md') for n in ('ours', 'base', 'theirs')]
            for pp, tt in ((p_ours, ours), (p_base, baset), (p_theirs, theirs)):
                open(pp, 'w', encoding='utf-8', newline='').write(tt)
            mf = sh('git', 'merge-file', '--union', '--marker-size=1', p_ours, p_base, p_theirs, cwd=WT, timeout=300)
            print('merge-file rc(=冲突hunk数)', mf.returncode, flush=True)
            merged = open(p_ours, encoding='utf-8').read()
            full = os.path.join(WT, fp)
            open(full, 'w', encoding='utf-8', newline='').write(merged)
            for pp in (p_ours, p_base, p_theirs):
                os.remove(pp)
        ad = sh('git', 'add', '--', *conflicted, cwd=WT, timeout=300)
        print('add rc', ad.returncode, (ad.stderr or '').strip()[-150:], flush=True)
        u = sh('git', '-c', 'core.quotepath=false', 'diff', '--name-only', '--diff-filter=U', cwd=WT, timeout=120)
        print('still-unmerged:', (u.stdout or '').strip()[:200], flush=True)
        if (u.stdout or '').strip():
            sh('git', 'merge', '--abort', cwd=WT, timeout=120)
            sys.exit(4)
        c = sh('git', 'commit', '--no-edit', cwd=WT, timeout=600)
        print('merge commit rc', c.returncode, (c.stdout + c.stderr).strip()[-150:], flush=True)
        if c.returncode != 0:
            sh('git', 'merge', '--abort', cwd=WT, timeout=120)
            sys.exit(4)
        # 合并后查重：AI-80 段唯一
        mm = open(os.path.join(WT, BIG[0]), encoding='utf-8').read()
        n80 = mm.count('# 增补卷 · AI-80 · UNX-P5 里程碑发布与年度镜像 · 首产段')
        print('AI-80 section count in merged master:', n80, flush=True)
        if n80 != 1:
            print('DUPLICATE AI-80 section — needs dedup, aborting', flush=True)
            sh('git', 'reset', '--hard', 'HEAD', cwd=WT, timeout=300)
            sh('git', 'merge', '--abort', cwd=WT, timeout=120)
            sys.exit(5)
        p2 = sh('git', 'push', 'origin', 'HEAD:main', cwd=WT, timeout=3600)
        out = ((p2.stdout or '') + (p2.stderr or '')).strip().splitlines()
        print('push2 rc', p2.returncode, out[-1][:120] if out else '', flush=True)
        if p2.returncode != 0:
            time.sleep(15)
    print('GAVE UP retries', flush=True)
    sys.exit(1)
finally:
    cleanup()
