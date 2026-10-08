# -*- coding: utf-8 -*-
"""AI-101 外科推送：worktree 挂 origin/main，只落本会话 6 件产物（5 内容件 checkout 自本地 HEAD + 台账条目幂等追加）。
他会话在途提交零代办（AI-04 2026-10-01 判例；工具形态承 AI-102 _w90q2_push_cherry.py 判例）。
台账不走 cherry-pick（远端尾部有他会话条目，改从本地台账裁取本会话条目块按字节同源追加）。
幂等：增补册已在远端 / 台账条目已在远端 → 跳过对应步骤，终验后 exit 0。
"""
import os, shutil, subprocess, sys, time, urllib.request, json

ROOT = r'D:\2\14\-Un-Real-0d23d9ux-Engine-main'
WT = os.path.join(ROOT, '_attic', 'ai101_push_worktree')
BOOK_PATH = 'docs/Varix/CoRun Varix STAR II · Unxreal/AI-101 · W90-Q1 · 300项新功能增补册（B01–B15 · W90-Q1-001–300）.md'
CONTENT_PATHS = [
    'docs/unxreal/gen/_w90q1_data1.py',
    'docs/unxreal/gen/_w90q1_data2.py',
    'docs/unxreal/gen/_w90q1_data3.py',
    'docs/unxreal/gen/_w90q1_firstprod.py',
    BOOK_PATH,
]
LEDGER_PATH = 'CoRun Varix STAR II · Unxreal · 统一协作总台账.md'
LEDGER_MARKER = '## 会话条目 · AI-101 · W90-Q1 域开卷立账（2026-10-01）'
MSG_FILE = os.path.join(ROOT, '_attic', 'ai101_commit_msg.txt')

with open(os.path.join(ROOT, LEDGER_PATH), 'r', encoding='utf-8') as f:
    local_ledger = f.read()
mi = local_ledger.index(LEDGER_MARKER)
sep = local_ledger.rindex('\n---\n', 0, mi)
MY_LEDGER_ENTRY = local_ledger[sep:]  # 含前导 \n---\n 分隔与条目全块，字节同源

COMMIT_MSG = open(MSG_FILE, 'r', encoding='utf-8').read().strip()

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
                headers={'Authorization': 'Bearer ' + tok, 'User-Agent': 'ai101-q1'})
            return json.load(urllib.request.urlopen(r, timeout=120))['sha']
        except Exception as e:
            print('api err', e, flush=True)
            time.sleep(8)
    return None

def cleanup():
    sh('git', 'worktree', 'remove', '--force', WT, timeout=600)
    if os.path.exists(WT):
        shutil.rmtree(WT, ignore_errors=True)
    sh('git', 'worktree', 'prune', timeout=120)

def remote_state():
    """远端基线上我的内容在位状态：(book_ok, ledger_ok)"""
    book_ok = sh('git', 'cat-file', '-e', 'HEAD:' + BOOK_PATH, cwd=WT).returncode == 0
    led = sh('git', 'show', 'HEAD:' + LEDGER_PATH, cwd=WT, timeout=600).stdout
    led_ok = LEDGER_MARKER in led
    return book_ok, led_ok

cleanup()
w = sh('git', 'worktree', 'add', '--detach', WT, 'HEAD', timeout=900)
print('worktree add rc', w.returncode, flush=True)
if w.returncode != 0:
    sys.exit(2)

try:
    for attempt in range(20):
        rh = api_remote_head()
        print(f'[{attempt}] remote={rh[:10] if rh else "?"}', flush=True)
        if not rh:
            time.sleep(20); continue
        f_ = sh('git', 'fetch', 'origin', 'main', cwd=WT, timeout=3600)
        if f_.returncode != 0:
            print('fetch fail', (f_.stderr or '')[-150:], flush=True); time.sleep(15); continue
        r = sh('git', 'reset', '--hard', 'FETCH_HEAD', cwd=WT, timeout=1200)
        if r.returncode != 0:
            print('reset fail', flush=True); time.sleep(15); continue

        book_ok, led_ok = remote_state()
        print(f'state book={book_ok} ledger={led_ok}', flush=True)
        if book_ok and led_ok:
            print('DONE: my content already fully on remote baseline', flush=True)
            sys.exit(0)

        # 5 件内容件：从本地 HEAD checkout（新文件零冲突）
        local_head = subprocess.run(['git', '-C', ROOT, 'rev-parse', 'HEAD'],
                                    capture_output=True, text=True, timeout=60).stdout.strip()
        for p in CONTENT_PATHS:
            c = sh('git', 'checkout', local_head, '--', p, cwd=WT, timeout=600)
            if c.returncode != 0:
                print('checkout fail', p, (c.stderr or '')[-200:], flush=True); sys.exit(3)
        # 台账：幂等追加本会话条目块
        if not led_ok:
            lp = os.path.join(WT, LEDGER_PATH)
            with open(lp, 'r', encoding='utf-8') as fh:
                cur = fh.read()
            if LEDGER_MARKER not in cur:
                if not cur.endswith('\n'):
                    cur += '\n'
                cur += MY_LEDGER_ENTRY if MY_LEDGER_ENTRY.startswith('\n') else '\n' + MY_LEDGER_ENTRY
                with open(lp, 'w', encoding='utf-8', newline='\n') as fh:
                    fh.write(cur)
            sh('git', 'add', '--', LEDGER_PATH, cwd=WT, timeout=600)

        c = sh('git', 'commit', '-m', COMMIT_MSG, cwd=WT, timeout=600)
        print('commit rc', c.returncode, flush=True)
        if c.returncode != 0:
            st = sh('git', '-c', 'core.quotepath=false', 'status', '--porcelain', cwd=WT).stdout
            print('commit status:', st[:400], flush=True)
            sys.exit(4)

        p = sh('git', 'push', 'origin', 'HEAD:main', cwd=WT, timeout=3600)
        out = ((p.stdout or '') + (p.stderr or '')).strip().splitlines()
        print('push rc', p.returncode, out[-1][:200] if out else '', flush=True)
        if p.returncode == 0:
            for _ in range(5):
                rh2 = api_remote_head()
                if rh2:
                    ls = sh('git', 'ls-tree', '-r', '--name-only', rh2, '--', BOOK_PATH, cwd=WT, timeout=300)
                    if BOOK_PATH in ls.stdout:
                        led2 = sh('git', 'show', rh2 + ':' + LEDGER_PATH, cwd=WT, timeout=600).stdout
                        print('DONE: book on remote; ledger entry on remote =', LEDGER_MARKER in led2, flush=True)
                        sys.exit(0)
                time.sleep(5)
            print('pushed but final verify inconclusive', flush=True)
            sys.exit(5)
        time.sleep(10)
    print('EXHAUSTED 20 attempts', flush=True)
    sys.exit(6)
finally:
    cleanup()
