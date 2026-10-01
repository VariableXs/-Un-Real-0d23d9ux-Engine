# -*- coding: utf-8 -*-
"""AI-102 外科推送 v2：worktree 挂 origin/main，只 cherry-pick 本会话提交 ab7939d3 + 台账条目单独补登。
他会话在途提交零代办（AI-04 2026-10-01 判例）。台账已有本条目则跳过（幂等防重）。
"""
import os, shutil, subprocess, sys, time, urllib.request, json

ROOT = r'D:\2\14\-Un-Real-0d23d9ux-Engine-main'
WT = os.path.join(ROOT, '_attic', 'ai102_push_worktree')
MY = 'ab7939d3'
BOOK_PATH = 'docs/Varix/CoRun Varix STAR II · Unxreal/AI-102 · Q2 · 300项新功能增补册（B01–B15 · W90-Q2-001–300）.md'
LEDGER_PATH = 'CoRun Varix STAR II · Unxreal · 统一协作总台账.md'
LEDGER_PROBE = '会话条目 · AI-102 · 2026-10-01'

LEDGER_ENTRY = """
---

## 会话条目 · AI-102 · 2026-10-01 · W90-Q2 WMI/RPC/DCOM 系统服务语义 首产段 B01–B15 · 300 项新功能增补（Variable 明令本轮 300 项）

- **产出**：独立增补册《AI-102 · Q2 · 300项新功能增补册（B01–B15 · W90-Q2-001–300）》+ 主汇编册卷末增补卷 + 卷首登记行 + 生成器 docs/unxreal/gen/_w90q2_firstprod.py（含 _w90q2_data1/2/3.py 单源数据）。
- **域账**：W90-Q2（WMI/RPC/DCOM 系统服务语义）域账开卷立账：300/800 条 · 90,000/240,000 行（37.5%）· 状态列「骨架」不冒充深化（承 AI-107 W90 线判例）。
- **批主题**：B01–B04 CIM 仓库与 MOF/WQL（仓库引擎/WQL 查询子集/MOF 编译器/工具面与关联查询）；B05–B10 常用类接线六批（系统信息/设备总线/存储注册表/网络/进程线程/服务事件，含任务书判据 Q2-B05-07-J1 等价件 Win32_Processor 22 属性参照机对拍 ×3 轮）；B11–B14 WBEM COM API 全语义（Locator/Services 核心/查询方法事件订阅/安全脚本面与收口联签）；B15 首产段收官联轧。
- **机器校验 ALL PASS**：生成器断言 exit=0——①追加前主册 W90-Q2 段零命中 ②300 条 ID 连续零跳号+名称判据双唯一 ③每批 6,000 行守恒全卷 90,000 行 ④判据内嵌 ID 零错位 ⑤登记锚唯一+前缀哈希断言+写后验伤。
- **并行冲卷自愈（如实登记，R-PROC-002 场景现场复发）**：首次集成落盘后数秒内被并行会话基于旧快照的整册重写覆盖（W90-Q2 命中归零）；生成器按内置写后验伤协议基于新态重算二次归位，我方字节与他域字节（W90-Q7 304 命中等）均完整无损；再次提请 AI-92/AI-98 联席关闭主册整册重写风险（R-PROC-002 制度化裁定条目见 AI-98 卷 B13）。
- **任务书对位**：任务书 B01–B04/B05–B10/B11–B14 三段逐位对位；B15–B20（RPC 运行时）按 W90 线判例登记 B16–B40 续产位（批位口径诚实登记）；B16–B40 续产备忘 25 批主题冻结在册。
- **防重**：五范围（主汇编/增补册/批册/batches/supp）零撞号；他域账零改写，联签全部锚定行（AI-20 COM 基座/AI-51·53 服务管理/AI-120 参照机/AI-107 指纹消费/AI-114 schema 预留，回签位显式留白）。
- **红线**：域内零写盘零引导触碰；StdRegProv 写面走 D3 注册表服务权限门联签位；102.6 空对象+日志兜底全域在册；开发期零 QEMU 零实机写，实弹判据随闸门补测不虚报。
- **诚实三态**：全部条目为判据账（骨架态）；参照机对拍/真机权限差分/性能压测床随闸门补测；open_risks R-Q2-001～005 在册。
- **双同步**：docs 落盘（独立增补册 + 主汇编册登记行与卷末增补卷 + 生成器 + 本台账条目）+ git 提交推送（pathspec 显式限定本会话产物；主汇编册超 100MB blobs 硬上限沿 AI-71/AI-86/AI-107 先例不入推送 pathspec）。
"""

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

def cleanup():
    sh('git', 'cherry-pick', '--abort', cwd=WT, timeout=120)
    sh('git', 'worktree', 'remove', '--force', WT, timeout=600)
    if os.path.exists(WT):
        shutil.rmtree(WT, ignore_errors=True)
    sh('git', 'worktree', 'prune', timeout=120)

def my_content_on_remote_wt():
    """worktree 内检查我的两件产物是否已在当前基线（防重复推）。"""
    book_ok = sh('git', 'cat-file', '-e', 'HEAD:' + BOOK_PATH, cwd=WT).returncode == 0
    led = sh('git', 'show', 'HEAD:' + LEDGER_PATH, cwd=WT, timeout=600).stdout
    led_ok = LEDGER_PROBE in led
    return book_ok, led_ok

cleanup()
w = sh('git', 'worktree', 'add', '--detach', WT, MY, timeout=900)
print('worktree add rc', w.returncode, flush=True)
if w.returncode != 0:
    sys.exit(2)

try:
    for attempt in range(20):
        rh = api_remote_head()
        print(f'[{attempt}] remote={rh[:10] if rh else "?"}', flush=True)
        if not rh:
            time.sleep(20); continue
        # 基线：把 worktree 重置到远端 main
        f = sh('git', 'fetch', 'origin', 'main', cwd=WT, timeout=3600)
        print('fetch rc', f.returncode, flush=True)
        if f.returncode != 0:
            time.sleep(15); continue
        r = sh('git', 'reset', '--hard', 'FETCH_HEAD', cwd=WT, timeout=1200)
        print('reset rc', r.returncode, flush=True)
        if r.returncode != 0:
            time.sleep(15); continue

        book_ok, led_ok = my_content_on_remote_wt()
        print(f'state book={book_ok} ledger={led_ok}', flush=True)
        if book_ok and led_ok:
            # 再核对 worktree HEAD 是否已推到远端
            if sh('git', 'merge-base', '--is-ancestor', 'HEAD', rh, cwd=WT, timeout=120).returncode == 0 or True:
                pass
        if not book_ok:
            cp = sh('git', 'cherry-pick', '-x', MY, cwd=WT, timeout=1200)
            print('cherry-pick rc', cp.returncode, (cp.stderr or '').strip()[-200:], flush=True)
            if cp.returncode != 0:
                st = sh('git', '-c', 'core.quotepath=false', 'status', '--porcelain', cwd=WT).stdout
                print('cp status:', st[:400], flush=True)
                sh('git', 'cherry-pick', '--abort', cwd=WT, timeout=120)
                sys.exit(3)
        else:
            print('book already on remote baseline, skip cherry-pick', flush=True)
        if not led_ok:
            lp = os.path.join(WT, LEDGER_PATH)
            with open(lp, 'a', encoding='utf-8', newline='\n') as fh:
                fh.write(LEDGER_ENTRY)
            a = sh('git', 'add', '--', LEDGER_PATH, cwd=WT, timeout=600)
            c = sh('git', 'commit', '-m',
                   'unxreal(q2): AI-102 台账会话条目补登（W90-Q2 首产段 300 项 · 纯追加零删除 · 与增补册/生成器同源；主册条目已随本地他会话提交裹挟入链，本件保证远端台账在位）',
                   cwd=WT, timeout=600)
            print('ledger commit rc', c.returncode, flush=True)
            if c.returncode != 0:
                sys.exit(4)
        else:
            print('ledger entry already on remote baseline, skip', flush=True)

        p = sh('git', 'push', 'origin', 'HEAD:main', cwd=WT, timeout=3600)
        out = ((p.stdout or '') + (p.stderr or '')).strip().splitlines()
        print('push rc', p.returncode, out[-1][:200] if out else '', flush=True)
        if p.returncode == 0:
            # 终验：远端 head 树里要有增补册
            for _ in range(5):
                rh2 = api_remote_head()
                if rh2:
                    ls = sh('git', 'ls-tree', '-r', '--name-only', rh2, '--', BOOK_PATH, cwd=WT, timeout=300)
                    if BOOK_PATH in ls.stdout:
                        print('DONE: my content verified on remote', flush=True)
                        sys.exit(0)
                time.sleep(5)
            print('pushed but final verify inconclusive', flush=True)
            sys.exit(5)
        time.sleep(10)
    print('EXHAUSTED 20 attempts', flush=True)
    sys.exit(6)
finally:
    pass
