"""领单产线塔（claim tower）· VTaskBoard 版。

与 dispatch_tower.py（PLAN.md 派工模式）的分工：
  dispatch_tower = 塔从 PLAN.md 切块派 WP，工人只做派给自己的那一包；
  claim_tower    = 工人 AI 自己去任务板（VTaskBoard.exe，端口 8767）领单，
                   塔只负责「保持 18 路工人永远有活干」。
                   工人编号 W001、W002…W018（三位数，新建任务时自动递增，
                   每个会话唯一不复用；领单号/状态文件/重链标记全部跟随编号）。

塔的四件事：
  1. 发车（--start N）：点「新建任务」建 N 个全新对话，先跑引导三件套
     （选工作空间 + 拖拽挂 4 文件 + 「/」面板挂 10 技能），再把
     「Variable 指令原文 + 操作要点」（CLAIM_WORKER_PROMPT.md 模板，
     首条即塔主原话）发给每个新对话（AI 零上下文也能开工）。
  2. 守护（--watch）：每 --interval 秒（默认 10s）轮询工人状态文件总线
     （dispatch/workers/Wxx.state）：
       READY            → 切到该会话 → 发续跑指令 → 工人去领下一单；
       BUSY             → 施工中不动它；超 --dead-min 分钟（默认 10）无动静 →
                          **归档重建**（写 dispatch/archive/ + 释放名下已领单 +
                          新建同编号工人会话继续）；--nudge-min 补发检查仍在
                          （dead_min=0 时作为唯一自愈手段）；
       会话 id 丢失      → 用首条提示词里的 VARIX-Wxx 标记在侧栏标题里重链。
  3. 积分监控与自动切号（--no-account-pool 关闭）：单轮发送失败 ≥3 或
     每 10 分钟预防性巡检 → 查 TreeCode 账号池（127.0.0.1:8792）；发送连败
     且池里有 ≥门槛余量的号 → 自动切号（TreeCode 重启 WorkBuddy，塔等待
     CDP 恢复并兜底带参重启）→ 释放全部在途任务 → 重建 18 路；池里没有
     可用号 → 判定「无可用积分账号」→ 优雅停机。
  4. 收口：任务板「待领/已领/阻塞」全为 0 且塔内无 BUSY 工人 → 全部完成退出；
     否则永远运行，直到 ① 50000+ 任务全部完成 ② 无可用积分账号 ③ 人工 Ctrl+C。

为什么工人状态走文件总线而不是读 UI：
  UI 忙闲只反映「正在生成」，不反映「任务做完没有」；文件是工人与塔共享的
  天然总线，工人写 READY 的几秒内塔就能感知——这是「AI 完成的一瞬间
  继续运行」的落地方式（dispatch_tower 同款机制，实测有效）。

安全铁律（继承本项目 D1~D15 全部实测教训）：
  1. 忙时一个字不写进输入框：空闲判定用 D15 修订版——「可见的停止按钮」
     是唯一忙票（按钮不存在=空闲；生成中按钮必以停止态存在）。
  2. 填充走 CDP Input.insertText（trusted 输入管线），execCommand 不进
     Slate 状态（D11 根因）；发送证据三选一（停止态/消息入流/编辑器清空）。
  3. 切会话必须确认选中态（D2/D3/D9/D14 同源教训），确认失败放弃填充。
  4. 发送失败有 45s 冷却（限流静默吞发送时绝不轰炸，D7/D10）。
  5. Ctrl+C 优雅停止：工人不受影响（它们靠领单 API 自循环），塔随时可重启接管。

用法（在 VarixAutoPilot2/ 下）：
  python tools/claim_tower.py --probe                # 只读体检（CDP/任务板/DOM）
  python tools/claim_tower.py --dry-run              # 演练：渲染协议/看忙闲，零 UI 动作
  python tools/claim_tower.py --start 18 --watch     # ★ 正式发车：建 18 对话+守护到收口
  python tools/claim_tower.py --watch                # 只守护（会话已建好/塔重启接管）
  可选：--workers N --interval S --nudge-min M --board-port P --board-exe 路径
  引导三件套（默认开，2026-10-06 上线）：新会话自动选工作空间
  -Un-Real-0d23d9ux-Engine-main、技能经「/」面板按显示名挂载为技能提及节点
  （挂载失败的回退为 @skill: 内联进首条提示词）、拖拽挂 4 份文件
  （VTaskBoard.exe + 3 份总纲 MD）；--no-bootstrap 关闭，
  --workspace/--files/--skills 可自定义；自检：python tools/bootstrap_selftest.py
"""

import argparse
import json
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import importlib.util

_ROOT = Path(__file__).resolve().parent.parent
_SPEC = importlib.util.spec_from_file_location("st", str(Path(__file__).parent / "send_selftest.py"))
st = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(st)
_BSPEC = importlib.util.spec_from_file_location("vb", str(Path(__file__).parent / "bootstrap.py"))
vb = importlib.util.module_from_spec(_BSPEC)
_BSPEC.loader.exec_module(vb)
_ASPEC = importlib.util.spec_from_file_location("apool", str(Path(__file__).parent / "account_pool.py"))
apool = importlib.util.module_from_spec(_ASPEC)
_ASPEC.loader.exec_module(apool)

# 引导三件套配置（--no-bootstrap 可整体关闭；main() 里按 CLI 参数重填）
BOOT = {"on": True, "ws": vb.WS_DEFAULT, "files": list(vb.FILES_DEFAULT)}

DISPATCH = _ROOT / "dispatch"
WORKERS_DIR = DISPATCH / "workers"
LEDGER = DISPATCH / "LEDGER.md"
PROTOCOL_MD = DISPATCH / "CLAIM_WORKER_PROMPT.md"
TOWER_LOG = DISPATCH / "claim_tower.log"
ARCHIVE_ROOT = DISPATCH / "archive"

BOARD_PORT = 8767
BOARD_EXE = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixTaskOps\VTaskBoard\dist\VTaskBoard.exe")
SKILLS_ARG = ",".join(vb.SKILLS_DEFAULT)
REMAIN_MIN = 30            # 切号目标账号的最低余量门槛（积分）
QUOTA_CHECK_EVERY = 600.0  # 预防性积分巡检间隔（秒）


def log(msg: str):
    line = f"[{time.strftime('%m-%d %H:%M:%S')}] {msg}"
    print(line, flush=True)
    try:
        with open(TOWER_LOG, "a", encoding="utf-8") as f:
            f.write(line + "\n")
    except OSError:
        pass


def ledger(event: str, wp: str, worker: str, note: str):
    row = f"| {time.strftime('%m-%d %H:%M')} | {event} | {wp} | {worker} | {note} |\n"
    try:
        text = LEDGER.read_text(encoding="utf-8")
        anchor = "| 时间 | 事件 | WP | 工人 | 结果/原因 |\n|---|---|---|---|---|\n"
        if anchor in text:
            text = text.replace(anchor, anchor + row, 1)
        else:
            text += row
        LEDGER.write_text(text, encoding="utf-8")
    except OSError as e:
        log(f"[WARN] LEDGER 写入失败：{e}")


# ══════════════════════ 任务板客户端 ══════════════════════

def board_api(path: str, payload: dict | None = None, timeout: float = 6.0):
    """调任务板 HTTP API。payload=None 走 GET，否则 POST JSON。"""
    url = f"http://127.0.0.1:{BOARD_PORT}{path}"
    data = json.dumps(payload).encode("utf-8") if payload is not None else None
    req = urllib.request.Request(
        url, data=data,
        method="POST" if data is not None else "GET",
        headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))


_board_last_try = 0.0


def ensure_board() -> bool:
    """健康检查；不通则拉起 VTaskBoard.exe（每 60s 至多重试一次）。"""
    global _board_last_try
    try:
        board_api("/api/health", timeout=3)
        return True
    except Exception:
        pass
    if time.time() - _board_last_try < 60:
        return False
    _board_last_try = time.time()
    if not BOARD_EXE.exists():
        log(f"[ERR] 任务板不通且 exe 不存在：{BOARD_EXE}")
        return False
    log(f"任务板不通 → 拉起 {BOARD_EXE.name} …")
    try:
        subprocess.Popen([str(BOARD_EXE)], cwd=str(BOARD_EXE.parent))
    except OSError as e:
        log(f"[ERR] 拉起任务板失败：{e}")
        return False
    for _ in range(25):
        time.sleep(1.2)
        try:
            board_api("/api/health", timeout=2)
            log("任务板已就绪")
            return True
        except Exception:
            continue
    log("[ERR] 任务板拉起后 30s 仍不通")
    return False


def board_counts() -> dict:
    """返回 {total, 待领, 已领, 阻塞, 已完成}；异常抛出由调用方处理。"""
    s = board_api("/api/summary", timeout=5)
    bs = s.get("by_status", {})
    return {"total": s.get("total", 0),
            "待领": bs.get("待领", 0),
            "已领": bs.get("已领", 0),
            "阻塞": bs.get("阻塞", 0),
            "已完成": bs.get("已完成", 0)}


# ══════════════════════ CDP 界面操作（继承实测教训） ══════════════════════

# ── D15 修订版空闲判定：唯一忙票 =「可见的停止按钮」──
# ① 空编辑器时发送键条件渲染整个消失 ⇒ 按钮不存在=空闲（最标准空闲态）；
# ② 停止元素限定 button 标签 + rect 可见性过滤（旧版 [title*="停止"] 会命中
#    隐藏残留元素恒投忙票）；
# ③ 生成动画元素（streaming/typing/loading）只做日志字段，不参与判忙
#    （569 次「动画忙」假阳性实证：消息区存在未知常驻 loading 类元素）。
JS_IDLE = r"""(() => {
  const vis = (e) => { const r = e.getBoundingClientRect();
    return r.width > 4 && r.height > 4; };
  const btn = document.querySelector('button.cr-send-button');
  const cls = btn ? String(btn.className) : '';
  const stopEls = Array.from(document.querySelectorAll(
      'button[class*="stop"],button[title*="停止"],button[aria-label*="停止"]'))
    .filter(vis);
  const animN = document.querySelectorAll(
      '[class*="streaming"],[class*="generating"],[class*="typing"],[class*="loading-"]').length;
  return { idle: stopEls.length === 0,
           btnGone: !btn, btnSending: /--sending|--stop/.test(cls),
           stopN: stopEls.length, animN: animN };
})()"""

# 切会话：对内层 _card_ 派发完整 MouseEvent 序列（裸 click React 不吃，D3/D14）
JS_CLICK_CONV = r"""((cid) => {
  const el = document.querySelector(
    'div.conversation-item[data-conversation-id="' + cid + '"]');
  if (!el) return { found: false };
  const node = el.querySelector('[class*="_card_"]') || el.firstElementChild || el;
  const fire = (n) => {
    const r = n.getBoundingClientRect();
    const o = { bubbles: true, cancelable: true, view: window,
                clientX: r.left + 8, clientY: r.top + 8, button: 0, detail: 1 };
    n.dispatchEvent(new MouseEvent('mouseover', o));
    n.dispatchEvent(new MouseEvent('mousemove', o));
    n.dispatchEvent(new MouseEvent('mousedown', o));
    n.dispatchEvent(new MouseEvent('mouseup', o));
    n.dispatchEvent(new MouseEvent('click', o));
  };
  fire(node);
  return { found: true };
})("%s")"""

# 新建对话：完整 MouseEvent 序列（裸 click 时灵时不灵，D8）
JS_CLICK_NEW = r"""(() => {
  const b = Array.from(document.querySelectorAll('button')).find(
    x => (x.innerText || '').trim() === '新建任务' && x.offsetWidth > 0);
  if (!b) return { ok: false, why: '页面上没有可见的「新建任务」按钮' };
  const r = b.getBoundingClientRect();
  const o = { bubbles: true, cancelable: true, view: window,
              clientX: r.left + 8, clientY: r.top + 8, button: 0, detail: 1 };
  b.dispatchEvent(new MouseEvent('mouseover', o));
  b.dispatchEvent(new MouseEvent('mousemove', o));
  b.dispatchEvent(new MouseEvent('mousedown', o));
  b.dispatchEvent(new MouseEvent('mouseup', o));
  b.dispatchEvent(new MouseEvent('click', o));
  return { ok: true };
})()"""

# 活跃会话判据：选中态在侧栏项后代的 _selected_ CSS-module 类上（D2）
JS_ACTIVE_CONV = r"""(() => {
  const items = Array.from(document.querySelectorAll('div.conversation-item'));
  for (const e of items) {
    const kids = Array.from(e.querySelectorAll('*'));
    if (kids.some(k => /_selected_/.test(String(k.className)))) {
      return e.getAttribute('data-conversation-id') || '';
    }
  }
  return '';
})()"""

# 编辑器真实字数（剔除 placeholder 25 字，D5）
JS_REAL_CHARS = r"""(() => {
  const ed = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!ed) return -1;
  const t = (ed.innerText || '').trim();
  if (/今天帮你做些什么|有什么我可以帮|How can I help/i.test(t)) return 0;
  let sum = 0;
  ed.querySelectorAll('[data-slate-string]').forEach(s => { sum += (s.textContent || '').length; });
  return sum || t.length;
})()"""

# 侧栏清单：id + 标题文本（用于按 VARIX-Wxx 标记重链丢失的会话）
JS_LIST_CONVS = r"""(() => Array.from(document.querySelectorAll(
      'div.conversation-item[data-conversation-id]')).map(e => ({
        id: e.getAttribute('data-conversation-id'),
        title: (e.innerText || '').trim()
      })))()"""


def idle3() -> tuple[bool, str]:
    """D15 修订版：唯一忙票=可见停止按钮。读不到=忙（保守）。"""
    v = st.call_js(JS_IDLE)
    if not isinstance(v, dict):
        return False, "读不到忙闲（CDP/页面异常）"
    if v.get("stopN", 1) > 0:
        return False, f"停止按钮可见（生成中，dbg anim={v.get('animN')}）"
    dbg = f"btn={'消失' if v.get('btnGone') else '在'}"
    return True, f"空闲（{dbg}）"


def active_conv() -> str:
    return st.call_js(JS_ACTIVE_CONV) or ""


def real_chars() -> int:
    v = st.call_js(JS_REAL_CHARS)
    return v if isinstance(v, int) else -1


def wait_idle(timeout_s: int = 90) -> bool:
    t0 = time.time()
    while time.time() - t0 < timeout_s:
        ok, _ = idle3()
        if ok:
            return True
        time.sleep(2.0)
    return False


def send_text(text: str, tag: str) -> tuple[bool, str]:
    """填入 + 点发送 + 受理证据。调用方保证目标会话已空闲。

    证据（任一成立即算后端受理，D11 实弹修订版）：
      E1 最强：发送键进入停止态 = AI 已开始生成
      E2 强：唯一 tag 出现在页面文本里（消息真入流）
      E3 弱：编辑器真实字数归零 且 当前空闲
    填充走 st.fill_cdp（CDP Input.insertText，trusted 输入管线，D11 正解）。
    """
    ok, ev = st.fill_cdp(text)
    if not ok:
        return False, f"填充失败：{ev}"
    time.sleep(0.6)
    r = st.call_js(st.click_send_js())
    if not (isinstance(r, dict) and r.get("ok")):
        return False, f"发送键点击失败：{r}"
    tag_js = json.dumps(tag, ensure_ascii=False)
    for _ in range(30):  # ≤30s
        time.sleep(1.0)
        s = st.probe_state() or {}
        if s.get("sending"):
            return True, "E1 发送键进停止态（AI 生成中=已受理）"
        if tag and st.call_js(
                f"(document.body.innerText||'').includes({tag_js})"):
            return True, f"E2 消息入流（tag={tag}）"
        if real_chars() == 0:
            idle_now, _ = idle3()
            if idle_now:
                return True, "E3 编辑器清空且空闲（弱证据）"
    return False, "30s 内无受理证据（后端拒绝/限流静默吞，D10）"


def switch_conv(conv_id: str, settle_s: float = 2.0) -> bool:
    """切到指定会话并确认真的切过去了（D3/D9/D14 同源教训全吸收）。"""
    for attempt in range(2):
        r = st.call_js(JS_CLICK_CONV % conv_id)
        if not (isinstance(r, dict) and r.get("found")):
            return False
        for _ in range(6):
            time.sleep(settle_s / 6 + 0.6)
            if active_conv() == conv_id:
                return True
    return False


def snapshot_convs() -> list[dict]:
    v = st.call_js(JS_LIST_CONVS)
    return v if isinstance(v, list) else []


def snapshot_conv_ids() -> set:
    return {c.get("id", "") for c in snapshot_convs() if c.get("id")}


# ══════════════════════ 工人文件总线 ══════════════════════

def read_state(wid: str) -> str:
    try:
        return (WORKERS_DIR / f"{wid}.state").read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def write_state(wid: str, content: str):
    WORKERS_DIR.mkdir(parents=True, exist_ok=True)
    (WORKERS_DIR / f"{wid}.state").write_text(content, encoding="utf-8")


def read_conv(wid: str) -> str:
    try:
        return (WORKERS_DIR / f"{wid}.conv").read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def write_conv(wid: str, conv_id: str):
    WORKERS_DIR.mkdir(parents=True, exist_ok=True)
    (WORKERS_DIR / f"{wid}.conv").write_text(conv_id, encoding="utf-8")


def state_mtime(wid: str) -> float:
    try:
        return (WORKERS_DIR / f"{wid}.state").stat().st_mtime
    except OSError:
        return 0.0


# ══════════════════════ 提示词渲染 ══════════════════════

# 首条提示词模板（Variable 2026-10-06 指定原话 + 操作要点，全文内联进脚本，
# 脚本自包含。启动时塔会把本模板自动写回 dispatch/CLAIM_WORKER_PROMPT.md，
# 供工人续跑时按路径重读——文件只是输出产物，唯一事实来源在这里）。
# 占位符：{WORKER_ID}=W001 式编号；{SKILLS_LINE}=兜底技能行。用 .replace 注入，
# 禁用 str.format（模板里的 curl JSON 含大量花括号）。
PROTOCOL_TEMPLATE = """hello 去在这个软件领一下任务并完成，然后在领任务的时候，注意标记一下，不要让其他AI 领到同样的任务呀还有，全面完整的分析要完成的所有内容，每一个要完成的功能和内容都要达到验收标准和深度打磨的情况下才算完成呀，不要完成其他AI的功能啦，直到全部任务56000个任务全部完成时才算结束呀，还有推送github仓库时，不要把任何非功能代码，截图，比如日志，测试代码还有脚本，提交，还有提交的介绍，只写纯功能的内容就可以了，然后先写完代码然后再进行修复呀，祝你好运呀，一定要深度打磨哦

---

↑ 以上是塔主 Variable 的原话指令，一字不改。以下是照做所需的操作要点。

## 0. 你的身份与"标记"机制

- 你是 **{WORKER_ID}**。领单时用 `AI-{WORKER_ID}`（一字不差）。
- "注意标记，不让其他 AI 领到同样的任务" = 领单 API 本身：
  **原子领单**，领到即被任务板标记为你的专属任务，其他 AI 再领同一单会
  收到 409 被拒。你不需要另做标记文件，报上编号领单就是标记。

## 1. 领单（每轮循环第一件事）

```bash
curl -s -m 15 -X POST http://127.0.0.1:8767/api/claim -d '{"worker":"AI-{WORKER_ID}"}'
```

- 成功返回 task：记住 `id`、`title`、`规格`、`书路径`、`验收`。
- **领到后立即**把状态文件写为 `BUSY <任务id>`（塔靠它感知你是忙是闲）：
  `D:/2/14/-Un-Real-0d23d9ux-Engine-main/VarixAutoPilot2/dispatch/workers/{WORKER_ID}.state`
- 板子没响应：先运行附带的 `VTaskBoard.exe`，等 3 秒重试；
  连续 3 次失败 → 状态文件写 `READY`，回复说明原因并停止。
- **心跳**：施工期间至少每 5 分钟刷新一次状态文件（如 `BUSY <任务id> 正在编译`）。
  超 10 分钟不刷新，塔判定你卡死：归档会话、释放名下任务、重建同编号新会话——
  长构建/长测试也必须按时写心跳。

## 2. 施工与验收（验收标准 + 深度打磨，一个都不能少）

- 全面完整分析要完成的所有内容，严格按任务单 `验收`/`规格`/`书路径` 原文施工。
- 附带的 3 份总纲 MD（CGPU / VE / CoRun Varix STAR II · 总纲与施工书）是总规格，
  动手前先读任务对应章节；任务单与总纲冲突时以任务单原文为准，并记进完成报告。
- 每个功能和内容都要达到验收标准并深度打磨才算完成；只做领到的任务，
  不做其他 AI 的功能。
- 纪律：动手前 git status；只 add 显式路径；源码只增不减不移动；异常零静默——
  卡住就说卡在哪/需要什么/建议，不许空转。

## 3. 收单与循环（直到 56000 个任务全部完成）

```bash
curl -s -m 15 -X POST http://127.0.0.1:8767/api/complete -d '{"id":"<任务id>","worker":"AI-{WORKER_ID}","result":"<一句话结果，含关键验证数据>"}'
```

- 收单后**立即回到第 1 步**领下一单，循环执行。
- 做不下去：`curl -s -m 15 -X POST http://127.0.0.1:8767/api/block -d '{"id":"<id>","reason":"<原因>"}'`
- 想放弃已领的单：`curl -s -m 15 -X POST http://127.0.0.1:8767/api/release -d '{"id":"<id>"}'`
  （单子回待领栏，他人可领）。禁止领了不做还占着不放。
- **无单可领**：状态文件写 `READY`，回复「产线无单，待命」——
  塔有新单会自动给你发续跑指令，收到后从第 1 步重新开始。
- 所有 API 必须真实执行并以真实响应为准，禁止编造领取/完成状态；
  只写自己的状态文件，不碰其他工人文件、不碰调度塔与工具源码。

## 4. GitHub 提交纪律（照 Variable 原话执行）

- 仓库：`github.com/VariableXs/-Un-Real-0d23d9ux-Engine.git`（main 分支）。
- **只提交功能代码**：日志、测试代码、测试脚本、截图、临时探针一律**不提交**
  （过程产物放 `_attic/`，保持被 .gitignore 排除，不入库）。
- commit 介绍只写纯功能的内容；先写完代码，然后再进行修复。

## 5. 必载技能

- {SKILLS_LINE}
- 挂载形态三者等价，消息里有哪种算哪种，全部就位才允许开工：
  ① 消息里的**技能提及节点**（塔经「/」面板挂载，显示为中文技能名，
     如「Rust 编程最佳实践」）；
  ② 消息里的 `@skill:xxx` 提及文本；
  ③ 以上都没有 → 用 Skill 工具按名逐一加载。

现在开始：执行第 1 步领单。
"""


def sync_protocol_file() -> None:
    """把脚本内联模板写回 dispatch/CLAIM_WORKER_PROMPT.md（工人续跑参考文件）。"""
    try:
        PROTOCOL_MD.write_text(PROTOCOL_TEMPLATE, encoding="utf-8")
    except Exception as e:
        log(f"[WARN] 协议文件同步失败（工人续跑参考仍可用旧文件）：{e}")


def render_protocol(wid: str, skills_fallback: list | None = None) -> str:
    """首条提示词：脚本内联模板（Variable 原话 + 操作要点）按编号渲染。

    每个会话编号唯一（W001、W002…由发车循环递增生成），绝不复用。
    skills_fallback=None          → 全部技能内联 @skill:（dry-run/补发场景）；
    skills_fallback=非空 list     → 仅把 / 面板没挂上的技能内联为兜底；
    skills_fallback=[]            → 技能已全部经 / 面板挂载为 mention 节点。
    """
    all_skills = [s.strip() for s in SKILLS_ARG.split(",") if s.strip()]
    if skills_fallback is None:
        line = vb.skills_inline(all_skills)
    elif skills_fallback:
        line = vb.skills_inline(skills_fallback)
    else:
        line = "（全部技能已由塔经「/」面板以技能提及节点挂载在本消息中）"
    body = (PROTOCOL_TEMPLATE
            .replace("{WORKER_ID}", wid)
            .replace("{SKILLS_LINE}", line))
    return f"【VARIX-{wid}·产线领单工人】\n\n{body}"


def continue_msg(wid: str) -> str:
    """续跑指令：短促、指向协议文件，工人自会回到领单循环。"""
    return (
        f"【VARIX-{wid}·塔·续跑】上一轮已结束（READY 已收到）。"
        f"重新执行领单循环，协议文件："
        f"D:/2/14/-Un-Real-0d23d9ux-Engine-main/VarixAutoPilot2/dispatch/CLAIM_WORKER_PROMPT.md"
        f"（你的编号仍是 {wid}）：claim → 施工 → complete → 循环；"
        f"领到先写状态文件 BUSY <任务id>，无单可领时写 READY 待命。现在开始领单。"
    )


def nudge_msg(wid: str) -> str:
    """超时自愈检查：对任何状态都安全的万能续跑指令。"""
    return (
        f"【VARIX-{wid}·塔·检查】你这条线超时无动静，做一次状态对账："
        f"① 手上有已领未完成的任务 → 继续施工；"
        f"② 已完成未收单 → 立即收单（complete）；"
        f"③ 什么都没有 → 按协议重新领单"
        f"（D:/2/14/-Un-Real-0d23d9ux-Engine-main/VarixAutoPilot2/dispatch/CLAIM_WORKER_PROMPT.md）；"
        f"④ 无单可领 → 写 READY 待命。状态文件："
        f"D:/2/14/-Un-Real-0d23d9ux-Engine-main/VarixAutoPilot2/dispatch/workers/{wid}.state"
    )


# ══════════════════════ 核心动作 ══════════════════════

def relink_conv(wid: str) -> str:
    """会话 id 丢失时按首条提示词头部的 VARIX-Wxx 标记在侧栏标题里重链。"""
    marker = f"VARIX-{wid}"
    try:
        for c in snapshot_convs():
            if marker in (c.get("title") or ""):
                write_conv(wid, c["id"])
                log(f"{wid} 会话重链成功 → {c['id'][:12]}…")
                ledger("会话重链", "-", wid, c["id"][:16])
                return c["id"]
    except Exception as e:
        log(f"[WARN] {wid} 重链失败：{e}")
    return ""


def new_conversation(first_prompt_fn, known_ids: set,
                     tag: str) -> tuple[str, str]:
    """点「新建任务」→ 引导三件套 → 发首条 → 差集/标记捕获新会话 id。

    first_prompt_fn(skills_fallback: list | None) -> str：引导完成后才知道
    哪些技能没挂上，首条提示词由本工厂按兜底清单渲染。
    返回 (conv_id, evidence)。

    顺序重要（实测）：先点新建，再判忙闲——忙闲是按会话的，
    新建出来的空白会话必然空闲，不能用全局 wait_idle 卡死自己。
    """
    r = st.call_js(JS_CLICK_NEW)
    if not (isinstance(r, dict) and r.get("ok")):
        return "", f"点「新建任务」失败：{r.get('why') if isinstance(r, dict) else r}"
    # 轮询确认真的切到空白会话（active='' 且消息列表消失），最多 8s
    blank = False
    for _ in range(11):
        time.sleep(0.7)
        if not active_conv() and not st.call_js(
                "!!document.querySelector('.cr-message-list')"):
            blank = True
            break
    if not blank:
        return "", "点「新建任务」后 8s 内未进入空白会话（视图仍在原会话上，放弃填充）"
    s = st.probe_state()
    if not s or not s.get("hasEditor"):
        return "", "点击后没有出现输入框"
    # 安全闸：确认视图确实离开了原会话（否则填充会污染用户的会话）
    if active_conv():
        return "", "视图仍在原会话上，放弃填充（防污染）"
    if not wait_idle(30):
        return "", "新建出来的会话 30s 内未进入可发送态"
    # ── 引导三件套（工作空间 + 技能 / 面板挂载 + 文件拖拽）──
    if BOOT["on"]:
        skills = [s.strip() for s in SKILLS_ARG.split(",") if s.strip()]
        ok_b, ev_b = vb.run(st, BOOT["ws"], BOOT["files"], skills=skills)
        if not ok_b:
            vb.cleanup_reset(st, JS_CLICK_NEW)
            return "", f"引导失败（已重置空白）：{ev_b}"
        mounted = ev_b.get("skills_ui", {}).get("mounted", [])
        fallback = ev_b.get("skills_fallback", [])
        log(f"引导：工作空间✓ 技能 / 面板挂载 {len(mounted)}/{len(skills)}"
            f"（内联兜底 {len(fallback)}：{','.join(fallback) or '无'}） 文件✓")
    else:
        fallback = None   # 未开引导 → 技能全走内联 @skill:
    ok, ev = send_text(first_prompt_fn(fallback), tag)
    if not ok:
        return "", f"首条发送失败：{ev}"
    # 侧栏渲染有 30s+ 延迟（实测），差集轮询 35s；再不行靠标记重链兜底
    for _ in range(35):
        time.sleep(1.0)
        fresh = snapshot_conv_ids() - known_ids
        if fresh:
            return fresh.pop(), ev
    return "", f"发送成功但侧栏 35s 未出现新会话（{ev}；标记 {tag} 待重链）"


def dispatch_continue(wid: str, conv_id: str, msg: str) -> tuple[bool, str]:
    """切到工人会话 → 确认空闲 → 发指令。"""
    if not switch_conv(conv_id):
        return False, f"切会话失败：{conv_id[:12]}"
    if not wait_idle(60):
        return False, "切过去后 60s 仍忙，本轮回头再试"
    return send_text(msg, f"VARIX-{wid}·塔")


# ══════════════ 卡死归档重建 & 积分切号（2026-10-06 Variable 指定）══════════════

def release_worker_tasks(wid: str = "") -> str:
    """把 AI-{wid}（wid="" = 全部工人）名下已领单退回待领（任务板 /api/release_worker）。"""
    try:
        r = board_api("/api/release_worker", {"worker": f"AI-{wid}" if wid else ""})
        if r.get("ok"):
            return f"释放 {r.get('n')} 单"
        return f"无在途单（{r.get('err')}）"
    except Exception as e:
        return f"释放失败：{e}（板上可能有孤儿已领单，需人工看板）"


def archive_dead_worker(wid: str, conv: str, state: str,
                        age_min: float, reason: str) -> Path:
    """卡死工人归档：写 dispatch/archive/日期/Wxx_HHMMSS.md + 释放名下任务 +
    作废会话映射与状态。旧会话留在 WorkBuddy 历史里（天然归档，不删）。"""
    d = ARCHIVE_ROOT / time.strftime("%Y-%m-%d")
    d.mkdir(parents=True, exist_ok=True)
    f = d / f"{wid}_{time.strftime('%H%M%S')}.md"
    freed = release_worker_tasks(wid)
    f.write_text(
        f"# {wid} 卡死归档 · {time.strftime('%Y-%m-%d %H:%M:%S')}\n\n"
        f"- 原因：{reason}\n"
        f"- 状态文件内容：`{state}`（{age_min:.0f} 分钟无 mtime 更新）\n"
        f"- 会话 id：`{conv}`（保留在 WorkBuddy 历史中）\n"
        f"- 任务板：{freed}\n"
        f"- 处置：会话映射作废，重建同编号工人继续领单\n",
        encoding="utf-8")
    write_conv(wid, "")
    write_state(wid, "")
    return f


def rebuild_worker(wid: str) -> tuple[str, str]:
    """重建同编号工人：新建会话（引导三件套 + 协议全文）。返回 (conv_id, evidence)。"""
    known = snapshot_conv_ids()
    prompt_fn = lambda missing: render_protocol(wid, missing)  # noqa: E731
    conv, ev = new_conversation(prompt_fn, known, f"VARIX-{wid}")
    if conv:
        write_conv(wid, conv)
        write_state(wid, "BUSY claim")
    elif "发送成功" in ev:
        write_state(wid, "BUSY claim")   # 侧栏没来得及刷新 → 标记重链兜底
    return conv, ev


def switch_account_flow(reason: str, workers: int) -> str:
    """积分耗尽 → TreeCode 切号 → 等 WorkBuddy 回来 → 释放全部任务并重建。

    返回："ok" / "no_account"（无可用积分账号 → 停机信号）/"fail:<why>"。
    """
    log(f"[积分] 触发切号检查（{reason}）…")
    info = apool.probe_all(REMAIN_MIN)
    if not info.get("ok"):
        return f"fail:账号池不可读 {info.get('why')}"
    log(f"[积分] 池子 {info['accounts']} 号 / 总余 {info['pool_remain']} / "
        f"最高 {info['richest']}({info['richest_remain']}) / 可切 {info['switchable']} 号")
    target = apool.pick_richest(REMAIN_MIN)
    if not target:
        ledger("停机", "-", "领单塔",
               f"无可用积分账号（门槛 {REMAIN_MIN}）：池 {info}")
        return "no_account"
    label = target.get("label")
    log(f"[切号] → {label}（余 {target.get('remain')}）。TreeCode 将重启 WorkBuddy…")
    ledger("切号", "-", "领单塔",
           f"→ {label} 余 {target.get('remain')}（{reason}）")
    ok, msg = apool.switch(label)
    if not ok:
        return f"fail:切号失败 {msg}"
    ok_b, ev = apool.wait_workbuddy_back(log=log)
    if not ok_b:
        return f"fail:WorkBuddy 未恢复 {ev}"
    log(f"[切号] WorkBuddy 已恢复（{ev}）→ 释放全部在途任务并重建 {workers} 路")
    ledger("切号完成", "-", "领单塔", f"{label} 就绪，{ev}")
    release_worker_tasks("")            # 全部已领单退回待领
    for i in range(1, workers + 1):     # 会话映射/状态全部作废（旧账号的会话不可用）
        wid = f"W{i:03d}"
        write_conv(wid, "")
        write_state(wid, "")
    # 逐个重建（每路 = 新会话 + 引导三件套 + 协议全文，约 2-3 分钟/路）
    built = 0
    for i in range(1, workers + 1):
        wid = f"W{i:03d}"
        conv2, ev2 = rebuild_worker(wid)
        if conv2 or "发送成功" in ev2:
            built += 1
            log(f"[切号重建] {wid} ✓")
        else:
            ledger("切号重建失败", "-", wid, ev2)
            log(f"[FAIL] [切号重建] {wid}：{ev2}")
        time.sleep(2.5)
    log(f"[切号] 重建完成 {built}/{workers} 路")
    ledger("切号重建", "-", "领单塔", f"{built}/{workers} 路")
    return "ok"


# ══════════════════════ 主流程 ══════════════════════

def main() -> int:
    global BOARD_PORT, BOARD_EXE, SKILLS_ARG, REMAIN_MIN
    ap = argparse.ArgumentParser(description="领单产线塔（VTaskBoard 版）")
    ap.add_argument("--probe", action="store_true", help="只读体检（CDP/任务板/DOM/账号池）")
    ap.add_argument("--dry-run", action="store_true", help="演练：渲染协议+看忙闲，零 UI 动作")
    ap.add_argument("--start", type=int, metavar="N", help="发车：建 N 个新会话并派首批协议")
    ap.add_argument("--watch", action="store_true", help="守护：READY 补发/卡死归档重建/积分切号/直到收口")
    ap.add_argument("--workers", type=int, default=18, help="工人数（默认 18）")
    ap.add_argument("--interval", type=int, default=10, help="守护轮询间隔秒（默认 10）")
    ap.add_argument("--dead-min", type=int, default=10,
                    help="BUSY 无动静多少分钟归档重建（默认 10，0=关）")
    ap.add_argument("--nudge-min", type=int, default=25,
                    help="BUSY 超时多少分钟自动补发检查（默认 25，0=关；"
                         "dead-min 生效时先到先触发）")
    ap.add_argument("--stale-min", type=int, default=90,
                    help="BUSY 超时多少分钟记 STALE 警告（默认 90）")
    ap.add_argument("--remain-min", type=int, default=REMAIN_MIN,
                    help="切号目标账号最低余量门槛（默认 30 积分）")
    ap.add_argument("--no-account-pool", action="store_true",
                    help="关闭积分监控与自动切号（TreeCode 不在线时用）")
    ap.add_argument("--board-port", type=int, default=BOARD_PORT)
    ap.add_argument("--board-exe", default=str(BOARD_EXE))
    ap.add_argument("--no-bootstrap", action="store_true",
                    help="关闭引导三件套（不选工作空间/不挂文件）")
    ap.add_argument("--workspace", default=vb.WS_DEFAULT,
                    help="新会话工作空间名（默认 -Un-Real-0d23d9ux-Engine-main）")
    ap.add_argument("--files", default="default",
                    help='逗号分隔的附件绝对路径；"default"=内置4文件；""=不挂文件')
    ap.add_argument("--skills", default=SKILLS_ARG,
                    help="逗号分隔的技能清单（默认经 / 面板挂载为技能提及节点，"
                         "失败者回退为 @skill: 内联）")
    a = ap.parse_args()

    BOARD_PORT = a.board_port
    BOARD_EXE = Path(a.board_exe)
    SKILLS_ARG = a.skills
    REMAIN_MIN = a.remain_min
    USE_POOL = not a.no_account_pool
    if a.no_bootstrap:
        BOOT["on"] = False
    BOOT["ws"] = a.workspace
    if a.files == "default":
        BOOT["files"] = list(vb.FILES_DEFAULT)
    elif a.files.strip() == "":
        BOOT["files"] = []
    else:
        BOOT["files"] = [s.strip() for s in a.files.split(",") if s.strip()]

    WORKERS_DIR.mkdir(parents=True, exist_ok=True)

    # ── 体检模式 ──
    if a.probe:
        print("── 1/4 CDP ──")
        if not st.devtools_alive():
            print("[ERR] 9222 不通。先跑 开端口.bat 彻底重启 WorkBuddy")
            return 1
        print(f"OK，侧栏会话数 {len(snapshot_conv_ids())}")
        ok, why = idle3()
        print(f"当前忙闲：idle={ok}（{why}）")
        print("── 2/4 任务板 ──")
        try:
            c = board_counts()
            print(f"OK：总 {c['total']}，待领 {c['待领']}，已领 {c['已领']}，"
                  f"阻塞 {c['阻塞']}，已完成 {c['已完成']}")
        except Exception as e:
            print(f"[WARN] 任务板不通（{e}）；发车时会自动拉起 {BOARD_EXE.name}")
        print("── 3/4 TreeCode 账号池 ──")
        info = apool.probe_all(REMAIN_MIN)
        if info.get("ok"):
            print(f"OK：{info['accounts']} 号 / 池总余 {info['pool_remain']} / "
                  f"最高 {info['richest']}({info['richest_remain']}) / "
                  f"可切（≥{REMAIN_MIN}）{info['switchable']} 号")
        else:
            print(f"[WARN] 账号池不可读：{info.get('why')}"
                  f"（TreeCode 未开时用 --no-account-pool 关闭积分监控）")
        print("── 4/4 DOM 详检 ──")
        sys.path.insert(0, str(Path(__file__).parent))
        return __import__("check_tower_dom").main()

    if not st.devtools_alive():
        print("[ERR] 9222 不通。先跑 开端口.bat（必须彻底重启 WorkBuddy 才有端口）")
        return 1

    # ── 任务板就绪（不通自动拉起）──
    if not ensure_board():
        print("[ERR] 任务板无法就绪，退出。工人提示词里的自启逻辑同样依赖它。")
        return 1
    sync_protocol_file()   # 脚本内联模板 → 写回 CLAIM_WORKER_PROMPT.md（工人续跑参考）
    try:
        c = board_counts()
        log(f"任务板概览：总 {c['total']}，待领 {c['待领']}，已领 {c['已领']}，"
            f"阻塞 {c['阻塞']}，已完成 {c['已完成']}")
    except Exception as e:
        log(f"[WARN] 概览读取失败：{e}")

    # ── 演练模式 ──
    if a.dry_run:
        print("\n── DRY-RUN 演练 ──")
        print(f"引导三件套：{'开' if BOOT['on'] else '关'}，"
              f"工作空间={BOOT['ws']}，文件 {len(BOOT['files'])} 个，"
              f"技能行={vb.skills_inline([s.strip() for s in SKILLS_ARG.split(',') if s.strip()])[:120]}…")
        print("会话编号：新建任务时自动递增 W001、W002、W003…（每个会话唯一，绝不复用）")
        for wid in ("W001", "W002"):
            p = render_protocol(wid)
            print(f"\n首条提示词（{wid}）开头预览：")
            print("  " + p[:130].replace("\n", "\n  ") + " …")
            print(f"  [核验] 编号行={('【VARIX-' + wid + '·产线领单工人】') in p}"
                  f" 领单号={('AI-' + wid) in p}"
                  f" 状态文件={('.state') in p and wid in p}")
        ok, why = idle3()
        print(f"\n当前忙闲：idle={ok}（{why}）")
        print(f"侧栏会话数：{len(snapshot_conv_ids())}")
        try:
            c = board_counts()
            print(f"任务板：待领 {c['待领']} / 已领 {c['已领']} / 已完成 {c['已完成']}")
        except Exception as e:
            print(f"任务板：不可达（{e}）")
        print("\n演练结束（零 UI 动作）。正式发车：python tools/claim_tower.py --start 18 --watch")
        return 0

    if not a.start and not a.watch:
        print("nothing to do：加 --start N / --watch / --probe / --dry-run")
        return 0

    # ── 发车：建 N 个会话 + 派首批协议 ──
    if a.start:
        known = snapshot_conv_ids()
        created = 0
        for i in range(1, a.start + 1):
            wid = f"W{i:03d}"
            if read_conv(wid):
                log(f"{wid} 已有会话 {read_conv(wid)[:12]}…，跳过新建")
                continue
            prompt_fn = lambda missing: render_protocol(wid, missing)  # noqa: E731
            log(f"{wid} 新建会话并派协议 …")
            conv, ev = new_conversation(prompt_fn, known, f"VARIX-{wid}")
            if conv:
                known.add(conv)
                write_conv(wid, conv)
                write_state(wid, "BUSY claim")
                ledger("派协议(新会话)", "-", wid, ev)
                log(f"{wid} ✓ {ev}")
                created += 1
                time.sleep(2.5)
            elif "发送成功" in ev:
                # 发出去了但侧栏没来得及刷新：状态照记，标记重链兜底
                write_state(wid, "BUSY claim")
                ledger("派协议(待重链)", "-", wid, ev)
                log(f"[WARN] {wid} {ev}")
                created += 1
                time.sleep(2.5)
            else:
                ledger("发车失败", "-", wid, ev)
                log(f"[FAIL] {wid} 新建失败：{ev}")
                time.sleep(5)
        log(f"发车完成：新建 {created} 路")

    # ── 守护：READY 补发 / 卡死归档重建 / 积分切号 / 收口判定 ──
    if a.watch:
        log(f"守护开始：{a.workers} 路，轮询 {a.interval}s，"
            f"卡死线 {a.dead_min}min（归档重建），补发 {a.nudge_min}min，"
            f"STALE 线 {a.stale_min}min"
            + (f"，积分门槛 {REMAIN_MIN}" if USE_POOL else "，积分监控关"))
        fail_until: dict[str, float] = {}   # wid → 该时间前不再尝试派发（失败冷却）
        last_hb = 0.0
        last_board_ok = True
        last_quota_check = time.time()
        try:
            while True:
                round_fail = 0      # 本轮发送失败次数（积分巡检触发器）
                # 1) 任务板心跳（决定 stop 条件与派发许可）
                board_ok, pend, claimed, blocked, done = True, None, None, None, None
                try:
                    c = board_counts()
                    pend, claimed, blocked, done = (
                        c["待领"], c["已领"], c["阻塞"], c["已完成"])
                except Exception as e:
                    board_ok = False
                    if last_board_ok:
                        log(f"[WARN] 任务板失联（{e}）——工人不受影响，塔降级巡检")
                    ensure_board()
                last_board_ok = board_ok

                # 2) 逐工人巡检
                busy_cnt = ready_cnt = missing_conv = 0
                any_busy = False
                now = time.time()
                for i in range(1, a.workers + 1):
                    wid = f"W{i:03d}"
                    state = read_state(wid)
                    conv = read_conv(wid)
                    if state and not conv:
                        conv = relink_conv(wid)  # 标记重链（只读侧栏，零干扰）
                    if not conv:
                        if state:
                            missing_conv += 1
                        continue

                    if state.startswith("BUSY"):
                        busy_cnt += 1
                        any_busy = True
                        age_min = (now - state_mtime(wid)) / 60.0
                        # 卡死归档重建（Variable 2026-10-06 指定：10min 无动静
                        # → 归档 + 释放名下任务 + 重建同编号工人继续）
                        if (a.dead_min and age_min > a.dead_min
                                and now >= fail_until.get(wid, 0)):
                            arch = archive_dead_worker(
                                wid, conv, state, age_min,
                                f"BUSY {age_min:.0f}min 无动静")
                            ledger("卡死归档", "-", wid,
                                   f"{state} {age_min:.0f}min → {arch.name}，重建中")
                            log(f"[卡死] {wid} {age_min:.0f}min 无动静 → "
                                f"归档 {arch.name} → 重建")
                            fail_until[wid] = time.time() + 60
                            conv2, ev2 = rebuild_worker(wid)
                            if conv2 or "发送成功" in ev2:
                                ledger("卡死重建", "-", wid, ev2)
                                log(f"{wid} 重建 ✓ {ev2}")
                            else:
                                ledger("卡死重建失败", "-", wid, ev2)
                                log(f"[FAIL] {wid} 重建失败：{ev2}")
                            time.sleep(2.5)
                            continue
                        if a.stale_min and age_min > a.stale_min:
                            log(f"[STALE] {wid} BUSY 超 {age_min:.0f}min（{state}）")
                        if (a.nudge_min and age_min > a.nudge_min
                                and now >= fail_until.get(wid, 0)):
                            ok, why = dispatch_continue(wid, conv, nudge_msg(wid))
                            if ok:
                                write_state(wid, state)  # 原内容覆盖 = 刷新 mtime
                                ledger("超时自愈", "-", wid, f"BUSY {age_min:.0f}min → 已补发检查")
                                log(f"{wid} 超时 {age_min:.0f}min → 补发检查 ✓")
                            else:
                                fail_until[wid] = time.time() + 45
                                round_fail += 1
                                log(f"[FAIL] {wid} 补发检查失败：{why}")
                            time.sleep(1.5)

                    elif state.startswith("READY"):
                        ready_cnt += 1
                        if not board_ok or not pend:
                            continue  # 板上无单：让它待命，不发空指令
                        if now < fail_until.get(wid, 0):
                            continue
                        ok, why = dispatch_continue(wid, conv, continue_msg(wid))
                        if ok:
                            write_state(wid, "BUSY claim")
                            ledger("补位派工", "-", wid, f"待领 {pend} → 续跑指令已发（{why}）")
                            log(f"{wid} ← 续跑（{why}）")
                        else:
                            fail_until[wid] = time.time() + 45
                            round_fail += 1
                            ledger("补位失败", "-", wid, why)
                            log(f"[FAIL] {wid} 续跑失败：{why}")
                        time.sleep(1.5)

                    elif not state:
                        # 有会话没状态：首条协议丢失/塔中途重启 → 重发协议全文
                        if board_ok and pend and now >= fail_until.get(wid, 0):
                            ok, why = dispatch_continue(wid, conv, render_protocol(wid))
                            if ok:
                                write_state(wid, "BUSY claim")
                                ledger("补发协议", "-", wid, why)
                                log(f"{wid} ← 补发协议 ✓")
                            else:
                                fail_until[wid] = time.time() + 45
                                round_fail += 1
                                log(f"[FAIL] {wid} 补发协议失败：{why}")
                            time.sleep(1.5)

                # 3) 心跳（节流：≥60s 一条）
                if time.time() - last_hb >= 60:
                    last_hb = time.time()
                    board_txt = (f"待领 {pend}/已领 {claimed}/阻塞 {blocked}/"
                                 f"完成 {done}/{c['total']}" if board_ok else "任务板失联")
                    log(f"心跳：忙 {busy_cnt} / 待命 {ready_cnt} / 未链会话 {missing_conv}"
                        f" / 任务板[{board_txt}]")

                # 3.5) 积分巡检与自动切号（Variable 2026-10-06 指定）：
                #      单轮发送失败 ≥3（异常信号）或每 10 分钟预防性巡检。
                #      连败且池里有号 → 切号重建；池里没号 → 停机。
                quota_due = time.time() - last_quota_check >= QUOTA_CHECK_EVERY
                if USE_POOL and (round_fail >= 3 or quota_due):
                    last_quota_check = time.time()
                    info = apool.probe_all(REMAIN_MIN)
                    if not info.get("ok"):
                        log(f"[积分] 账号池不可读：{info.get('why')}（下轮再试）")
                    else:
                        log(f"[积分巡检] 池 {info['accounts']} 号 总余 "
                            f"{info['pool_remain']} 最高 {info['richest']}"
                            f"({info['richest_remain']}) 可切 {info['switchable']}"
                            f" / 本轮发送失败 {round_fail}")
                        if round_fail >= 3:
                            if info.get("switchable", 0) <= 0:
                                log("★ 停机 ★ 发送连败且账号池无可用积分账号"
                                    "——产线终止（50000 任务未完，需人工补充账号）")
                                ledger("停机", "-", "领单塔",
                                       f"发送连败 {round_fail} + 无可用积分账号"
                                       f"（门槛 {REMAIN_MIN}）：{info}")
                                return 2
                            r = switch_account_flow(
                                f"单轮发送失败 {round_fail} 次", a.workers)
                            if r == "no_account":
                                log("★ 停机 ★ 无可用积分账号——产线终止")
                                return 2
                            if r == "ok":
                                log("[切号] 全部工人已重建，产线继续")
                            else:
                                log(f"[FAIL] 切号失败：{r}（下轮重试）")

                # 4) 收口判定：板上无单可领、无在途、无阻塞，塔内也无人 BUSY
                if board_ok and pend == 0 and claimed == 0 and blocked == 0 \
                        and not any_busy:
                    log(f"★ 收口 ★ 任务板 {done}/{c['total']} 全部完成，"
                        f"塔内 {a.workers} 路无在途任务 —— 产线结束")
                    ledger("收口", "-", "领单塔",
                           f"任务板全部完成（{done}/{c.get('total', done)}），产线结束")
                    return 0

                time.sleep(a.interval)
        except KeyboardInterrupt:
            log("人工停止（Ctrl+C）——工人不受影响（它们靠领单 API 自循环），"
                "重新 --watch 即恢复接管")
            ledger("停止", "-", "领单塔", "人工 Ctrl+C，工人继续跑")
            return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
