"""领单产线塔（claim tower）· VTaskBoard 版。

与 dispatch_tower.py（PLAN.md 派工模式）的分工：
  dispatch_tower = 塔从 PLAN.md 切块派 WP，工人只做派给自己的那一包；
  claim_tower    = 工人 AI 自己去任务板（VTaskBoard.exe，端口 8767）领单，
                   塔只负责「保持 18 路工人永远有活干」。

塔的三件事：
  1. 发车（--start N）：点「新建任务」建 N 个全新对话，把「领单工人协议」
     以【正文全文 + MD 文件路径】双通道发给每个新对话（AI 零上下文也能开工）。
  2. 守护（--watch）：每 --interval 秒（默认 10s）轮询工人状态文件总线
     （dispatch/workers/Wxx.state）：
       READY            → 切到该会话 → 发续跑指令 → 工人去领下一单；
       BUSY             → 施工中不动它；超 --nudge-min 分钟无动静 →
                          切过去确认空闲后补发「续跑检查」（自愈假死）；
       会话 id 丢失      → 用首条提示词里的 VARIX-Wxx 标记在侧栏标题里重链。
  3. 收口：任务板「待领/已领/阻塞」全为 0 且塔内无 BUSY 工人 → 全部完成退出；
     否则永远运行，直到人工 Ctrl+C。

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

DISPATCH = _ROOT / "dispatch"
WORKERS_DIR = DISPATCH / "workers"
LEDGER = DISPATCH / "LEDGER.md"
PROTOCOL_MD = DISPATCH / "CLAIM_WORKER_PROMPT.md"
TOWER_LOG = DISPATCH / "claim_tower.log"

BOARD_PORT = 8767
BOARD_EXE = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixTaskOps\VTaskBoard\dist\VTaskBoard.exe")


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

def render_protocol(wid: str) -> str:
    """首条提示词：协议全文内联（新对话零上下文也能开工）+ VARIX 标记。"""
    tpl = PROTOCOL_MD.read_text(encoding="utf-8")
    body = tpl.replace("{WORKER_ID}", wid)
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


def new_conversation(first_prompt: str, known_ids: set,
                     tag: str) -> tuple[str, str]:
    """点「新建任务」→ 发首条 → 差集/标记捕获新会话 id。返回 (conv_id, evidence)。

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
    ok, ev = send_text(first_prompt, tag)
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


# ══════════════════════ 主流程 ══════════════════════

def main() -> int:
    global BOARD_PORT, BOARD_EXE
    ap = argparse.ArgumentParser(description="领单产线塔（VTaskBoard 版）")
    ap.add_argument("--probe", action="store_true", help="只读体检（CDP/任务板/DOM）")
    ap.add_argument("--dry-run", action="store_true", help="演练：渲染协议+看忙闲，零 UI 动作")
    ap.add_argument("--start", type=int, metavar="N", help="发车：建 N 个新会话并派首批协议")
    ap.add_argument("--watch", action="store_true", help="守护：READY 补发/超时自愈/直到收口")
    ap.add_argument("--workers", type=int, default=18, help="工人数（默认 18）")
    ap.add_argument("--interval", type=int, default=10, help="守护轮询间隔秒（默认 10）")
    ap.add_argument("--nudge-min", type=int, default=25,
                    help="BUSY 超时多少分钟自动补发检查（默认 25，0=关）")
    ap.add_argument("--stale-min", type=int, default=90,
                    help="BUSY 超时多少分钟记 STALE 警告（默认 90）")
    ap.add_argument("--board-port", type=int, default=BOARD_PORT)
    ap.add_argument("--board-exe", default=str(BOARD_EXE))
    a = ap.parse_args()

    BOARD_PORT = a.board_port
    BOARD_EXE = Path(a.board_exe)

    WORKERS_DIR.mkdir(parents=True, exist_ok=True)

    # ── 体检模式 ──
    if a.probe:
        print("── 1/3 CDP ──")
        if not st.devtools_alive():
            print("[ERR] 9222 不通。先跑 开端口.bat 彻底重启 WorkBuddy")
            return 1
        print(f"OK，侧栏会话数 {len(snapshot_conv_ids())}")
        ok, why = idle3()
        print(f"当前忙闲：idle={ok}（{why}）")
        print("── 2/3 任务板 ──")
        try:
            c = board_counts()
            print(f"OK：总 {c['total']}，待领 {c['待领']}，已领 {c['已领']}，"
                  f"阻塞 {c['阻塞']}，已完成 {c['已完成']}")
        except Exception as e:
            print(f"[WARN] 任务板不通（{e}）；发车时会自动拉起 {BOARD_EXE.name}")
        print("── 3/3 DOM 详检 ──")
        sys.path.insert(0, str(Path(__file__).parent))
        return __import__("check_tower_dom").main()

    if not st.devtools_alive():
        print("[ERR] 9222 不通。先跑 开端口.bat（必须彻底重启 WorkBuddy 才有端口）")
        return 1

    # ── 任务板就绪（不通自动拉起）──
    if not ensure_board():
        print("[ERR] 任务板无法就绪，退出。工人提示词里的自启逻辑同样依赖它。")
        return 1
    try:
        c = board_counts()
        log(f"任务板概览：总 {c['total']}，待领 {c['待领']}，已领 {c['已领']}，"
            f"阻塞 {c['阻塞']}，已完成 {c['已完成']}")
    except Exception as e:
        log(f"[WARN] 概览读取失败：{e}")

    # ── 演练模式 ──
    if a.dry_run:
        wid = "W01"
        print("\n── DRY-RUN 演练 ──")
        print(f"首条提示词（{wid}）渲染预览：\n")
        print(render_protocol(wid)[:1000])
        print("……")
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
            wid = f"W{i:02d}"
            if read_conv(wid):
                log(f"{wid} 已有会话 {read_conv(wid)[:12]}…，跳过新建")
                continue
            prompt = render_protocol(wid)
            log(f"{wid} 新建会话并派协议 …")
            conv, ev = new_conversation(prompt, known, f"VARIX-{wid}")
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

    # ── 守护：READY 补发 / 超时自愈 / 收口判定 ──
    if a.watch:
        log(f"守护开始：{a.workers} 路，轮询 {a.interval}s，"
            f"自愈补发 {a.nudge_min}min，STALE 线 {a.stale_min}min")
        fail_until: dict[str, float] = {}   # wid → 该时间前不再尝试派发（失败冷却）
        last_hb = 0.0
        last_board_ok = True
        try:
            while True:
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
                    wid = f"W{i:02d}"
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
                                log(f"[FAIL] {wid} 补发协议失败：{why}")
                            time.sleep(1.5)

                # 3) 心跳（节流：≥60s 一条）
                if time.time() - last_hb >= 60:
                    last_hb = time.time()
                    board_txt = (f"待领 {pend}/已领 {claimed}/阻塞 {blocked}/"
                                 f"完成 {done}/{c['total']}" if board_ok else "任务板失联")
                    log(f"心跳：忙 {busy_cnt} / 待命 {ready_cnt} / 未链会话 {missing_conv}"
                        f" / 任务板[{board_txt}]")

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
