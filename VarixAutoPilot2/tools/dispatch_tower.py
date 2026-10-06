"""18 路产线调度塔（dispatch tower）。

架构：文件总线 + CDP 发送。
  - 18 个独立 WorkBuddy 会话 = 18 路工人；每个工人的会话 id 记在 dispatch/workers/Wxx.conv。
  - 工人开工写 dispatch/workers/Wxx.state = BUSY <WP>；收工写 READY <WP>（文件总线，秒级、零 UI 误判）。
  - 塔轮询 state 文件：READY 且 PLAN 有待派 → CDP 切到该会话 → 发续单指令。
  - 新工人不足 18 → CDP 点「新建任务」→ 发首条统一提示词 → 捕获新会话 id。
  - 全部动作记入 dispatch/LEDGER.md；塔日志 dispatch/tower.log。

为什么 worker 状态走文件而不是读 UI：
  UI 忙闲只反映「正在生成」，不反映「任务做完没有」；而且 CDP 读消息流
  只能看到当前打开的会话。文件是 worker 与塔共享的天然总线，完成即 READY，
  塔秒级感知——这就是「AI 完成的一瞬间继续运行」的落地方式。

安全铁律（继承项目实测教训）：
  1. 忙时一个字不写进输入框（三判据任一说忙就算忙）。
  2. 发送动作自带双重证据（消息入流 或 编辑器清空），失败记台账并重试。
  3. Ctrl+C 优雅停止：跑完当前原子动作再退，台账记「人工停止」。
  4. 塔不改 PLAN 的规格原文，只改「状态:」一行；不碰工人输出目录。

用法：
  python tools/dispatch_tower.py --probe                 # 只读体检（转 check_tower_dom）
  python tools/dispatch_tower.py --dry-run               # 演练：解析 PLAN/渲染提示词/体检，零 UI 动作
  python tools/dispatch_tower.py --start 18 --watch      # 正式发车：建 18 会话+派首批+守护补位
  python tools/dispatch_tower.py --watch                 # 只守护（会话已建好时用）
  可选：--workers N --interval S --stale-min M --spec 路径 --skills 技能1,技能2 --out 目录
  引导三件套（默认开，2026-10-06 上线）：新会话自动选工作空间、拖拽挂 4 份文件、
  技能以 @skill: 内联；--no-bootstrap 关闭；--workspace/--files 可自定义；
  自检：python tools/bootstrap_selftest.py
"""

import argparse
import json
import re
import sys
import time
from pathlib import Path

import importlib.util

_ROOT = Path(__file__).resolve().parent.parent
_SPEC = importlib.util.spec_from_file_location("st", str(Path(__file__).parent / "send_selftest.py"))
st = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(st)
_BSPEC = importlib.util.spec_from_file_location("vb", str(Path(__file__).parent / "bootstrap.py"))
vb = importlib.util.module_from_spec(_BSPEC)
_BSPEC.loader.exec_module(vb)

# 引导三件套配置（--no-bootstrap 可整体关闭；main() 里按 CLI 参数重填）
BOOT = {"on": True, "ws": vb.WS_DEFAULT, "files": list(vb.FILES_DEFAULT)}

DISPATCH = _ROOT / "dispatch"
WORKERS_DIR = DISPATCH / "workers"
REPORTS_DIR = DISPATCH / "reports"
LEDGER = DISPATCH / "LEDGER.md"
PLAN = DISPATCH / "PLAN.md"
PROMPT_TPL = DISPATCH / "WORKER_PROMPT.md"
TOWER_LOG = DISPATCH / "tower.log"

# ── 三判据空闲探测（idle_probe.py 实测版）──
JS_IDLE = r"""(() => {
  const btn = document.querySelector('button.cr-send-button');
  const cls = btn ? String(btn.className) : '';
  const stopN = document.querySelectorAll(
    'button[class*="stop"],button[aria-label*="停止"],[title*="停止"]').length;
  const animN = document.querySelectorAll(
    '[class*="streaming"],[class*="generating"],[class*="typing"],[class*="loading-"]').length;
  return { idle1: btn ? !/--sending|--stop/.test(cls) : false,
           stopN: stopN, animN: animN };
})()"""

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

JS_CLICK_NEW = r"""(() => {
  const b = Array.from(document.querySelectorAll('button')).find(
    x => (x.innerText || '').trim() === '新建任务' && x.offsetWidth > 0);
  if (!b) return { ok: false, why: '页面上没有可见的「新建任务」按钮' };
  // 实测：原生 b.click() 时灵时不灵（React 合成事件不吃裸 click），
  // 与 switch_conv 同源问题。统一派发完整 MouseEvent 序列。
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

# ── 活跃会话判据（2026-10-06 实测修正版）──
# 侧栏项本身 class 恒定是 "conversation-item"，没有 active/selected/aria 标记；
# 真正的选中态在它的**后代**元素上，是带哈希的 CSS-module 类 `_selected_xxx`。
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

# ── 消息流证据：直接看消息列表文本里有没有本次发送的唯一 tag ──
# 实测本版 DOM 里 [data-message-author-role] / .cr-user-message 全部匹配不到（0 个），
# 旧版 JS_LAST_USER_MSG 恒返回空串，靠它做证据必然假阴性。
JS_MSG_TAIL_TMPL = r"""(() => {
  const ml = document.querySelector('.cr-message-list');
  if (!ml) return '';
  return (ml.innerText || '').trim().slice(-600);
})()"""

# ── 编辑器真实字数（剔除 placeholder）──
# 空编辑器 innerText 是 "今天帮你做些什么？@ 添加上下文，/调用技能与指令"（25 字），
# 旧判据 chars==0 永远不成立，"编辑器清空"这条证据因此是死的。
JS_REAL_CHARS = r"""(() => {
  const ed = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!ed) return -1;
  const t = (ed.innerText || '').trim();
  if (/今天帮你做些什么|有什么我可以帮|How can I help/i.test(t)) return 0;
  let sum = 0;
  ed.querySelectorAll('[data-slate-string]').forEach(s => { sum += (s.textContent || '').length; });
  return sum || t.length;
})()"""

# JS_LAST_USER_MSG 已删除（D1：[data-message-author-role] 等选择器在本版
# DOM 恒 0 个，该判据恒返回空串；消息入流证据改用 body 全文查唯一 tag）。


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
        else:  # 表头被改动过：退化为追加到文件尾
            text += row
        LEDGER.write_text(text, encoding="utf-8")
    except OSError as e:
        log(f"[WARN] LEDGER 写入失败：{e}")


# ── PLAN 解析：只认「### WP-xxxxx」块里的「状态: 待派 / 进行中(Wxx) / DONE」──
WP_RE = re.compile(r"^### (WP-\d+)\s*$", re.M)
STATUS_RE = re.compile(r"^- 状态:\s*(.+)$", re.M)


def parse_plan():
    text = PLAN.read_text(encoding="utf-8")
    blocks = {}
    matches = list(WP_RE.finditer(text))
    for i, m in enumerate(matches):
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        body = text[m.start():end]
        sm = STATUS_RE.search(body)
        blocks[m.group(1)] = sm.group(1).strip() if sm else "待派"
    return text, blocks


def mark_plan(plan_text: str, wp: str, status: str) -> str:
    """把某 WP 块里的「状态:」行替换，其余原文不动。"""
    pat = re.compile(r"(### %s\s*[\s\S]*?^- 状态:\s*)[^\n]*" % re.escape(wp), re.M)
    new_text, n = pat.subn(r"\g<1>%s" % status, plan_text, count=1)
    if n != 1:
        log(f"[WARN] PLAN 中未找到 {wp} 的状态行，未标记")
    return new_text


def idle3():
    """三判据：任一说忙就算忙。读不到也当忙。

    注意：这是**当前显示会话**的忙闲，不是全局的。实测在别的会话生成时，
    切到另一个会话后这里的判据会立刻变空闲——塔正是靠这一点串行派单。
    """
    v = st.call_js(JS_IDLE)
    if not v:
        return False, "读不到忙闲"
    if not v.get("idle1"):
        return False, "发送键处于停止/生成态"
    if v.get("stopN", 1) > 0:
        return False, "停止按钮可见"
    if v.get("animN", 1) > 0:
        return False, "生成动画元素存在"
    return True, "三判据一致=空闲"


def active_conv() -> str:
    """当前视图打开的是哪个会话（实测判据：侧栏项后代的 _selected_ 类）。"""
    return st.call_js(JS_ACTIVE_CONV) or ""


def real_chars() -> int:
    """输入框真实字数（剔除 placeholder 后的值；-1=找不到编辑器）。"""
    v = st.call_js(JS_REAL_CHARS)
    return v if isinstance(v, int) else -1


def msg_tail() -> str:
    return st.call_js(JS_MSG_TAIL_TMPL) or ""


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

    证据（任一成立即算后端受理，2026-10-06 D11 实弹修订）：
      E1 最强：发送键进入「停止」态 = AI 已开始生成 = 必然受理
      E2 强：唯一 tag 出现在页面文本里（消息真入流）
      E3 弱：编辑器真实字数归零 且 发送键非 stop
    废弃证据：会话数+1（实测侧栏渲染延迟 30s+，新会话已建但列表不刷新）；
              [data-message-author-role] 入流（本版 DOM 恒 0 个）。
    填充：fill_cdp（CDP Input.insertText，trusted 输入管线）。
    旧 execCommand 版只写 DOM 不进 Slate 状态——「编辑器有字但发送键
    黑色发不出」的根因（D11），点击被 React 空内容逻辑静默吞掉。
    """
    ok, ev = st.fill_cdp(text)
    if not ok:
        return False, f"填充失败：{ev}"
    time.sleep(0.6)
    r = st.call_js(st.click_send_js())
    if not (isinstance(r, dict) and r.get("ok")):
        return False, f"发送键点击失败：{r}"
    tag_js = json.dumps(tag)
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
                return True, "E3 编辑器清空且发送键非 stop（弱证据）"
    return False, "30s 内无受理证据（后端拒绝或网络异常）"


def switch_conv(conv_id: str, settle_s: float = 2.0) -> bool:
    """切到指定会话并**确认真的切过去了**。

    实测：对外层 div 调 el.click() 不生效（React 不响应），必须对内层
    `_card_` 元素派发完整 MouseEvent 序列（over/move/down/up/click）。
    返回 True 当且仅当切完后 active_conv() == conv_id。
    """
    for attempt in range(2):  # 实测：从空白新会话切回时首次点击偶发被吞，重试一轮即可
        r = st.call_js(JS_CLICK_CONV % conv_id)
        if not (isinstance(r, dict) and r.get("found")):
            return False
        for _ in range(6):
            time.sleep(settle_s / 6 + 0.6)
            if active_conv() == conv_id:
                return True
    return False


def read_state(wid: str) -> str:
    p = WORKERS_DIR / f"{wid}.state"
    try:
        return p.read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def write_state(wid: str, content: str):
    (WORKERS_DIR / f"{wid}.state").write_text(content, encoding="utf-8")


def read_conv(wid: str) -> str:
    p = WORKERS_DIR / f"{wid}.conv"
    try:
        return p.read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def write_conv(wid: str, conv_id: str):
    (WORKERS_DIR / f"{wid}.conv").write_text(conv_id, encoding="utf-8")


def render_first_prompt(wid: str, wp: str, spec: str, skills: str, outdir: str) -> str:
    tpl = PROMPT_TPL.read_text(encoding="utf-8")
    skills_line = " ".join(
        f"@skill:{s.strip()}" for s in skills.split(",") if s.strip())
    out = (tpl
           .replace("{WORKER_ID}", wid)
           .replace("{WP_ID}", wp)
           .replace("{SPEC_MD_PATH}", spec)
           .replace("{SKILLS_LIST}", skills_line)
           .replace("{OUTPUT_DIR}", outdir))
    # 文件总线协议（开工/收工写 state）注入到提示词末尾
    bus = f"""

## 文件总线协议（调度塔依赖，必须执行）

1. **开工第一件事**：把 `dispatch/workers/{wid}.state` 写为 `BUSY {wp}`。
2. **收工前最后一步**：完整报告写入 `dispatch/reports/{wp}.md`（按上面的报告格式），
   然后把 `dispatch/workers/{wid}.state` 写为 `READY {wp}`。
   塔靠这个文件的 READY 在几秒内给你派下一单——漏写 = 这条产线停摆。
3. 阻塞时同样写 `READY {wp}` 并在报告里标 BLOCKED 与原因，塔会调度处理。
"""
    return out + bus


def new_conversation(first_prompt: str, known_ids: set,
                     tag: str = "") -> tuple[str, str]:
    """点「新建任务」→ 发首条 → 差集捕获新会话 id。返回 (conv_id, evidence)。

    顺序重要（2026-10-06 实测）：先点新建，再判忙闲。
    实测「AI 正在别的会话生成」时，新建出来的空白会话发送键是「发送」（空闲态），
    ——忙闲是**按会话**的，不是全局的。所以不能用全局 wait_idle 把自己卡死。
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
    # ── 引导三件套（工作空间 + 文件附件；技能走提示词内联 @skill:）──
    if BOOT["on"]:
        ok_b, ev_b = vb.run(st, BOOT["ws"], BOOT["files"])
        if not ok_b:
            vb.cleanup_reset(st, JS_CLICK_NEW)
            return "", f"引导失败（已重置空白）：{ev_b}"
    ok, ev = send_text(first_prompt, tag)
    if not ok:
        return "", f"首条发送失败：{ev}"
    for _ in range(10):  # 等侧栏出现新会话项
        time.sleep(1.0)
        ids = snapshot_conv_ids()
        fresh = ids - known_ids
        if fresh:
            return fresh.pop(), ev
    return "", "发送成功但没捕捉到新会话 id"


def snapshot_conv_ids() -> set:
    v = st.call_js(
        r"""(() => Array.from(document.querySelectorAll(
              'div.conversation-item[data-conversation-id]'))
              .map(e => e.getAttribute('data-conversation-id')))()"""
    )
    return set(v) if isinstance(v, list) else set()


def dispatch_next(wid: str, conv_id: str, wp: str) -> tuple[bool, str]:
    """给空闲 worker 发续单。"""
    if not switch_conv(conv_id):
        return False, f"切会话失败：{conv_id}"
    if not wait_idle(60):
        return False, "切过去后 60s 仍忙，本轮回头再试"
    msg = (
        f"续单：上一包的 READY 与报告已收到。现在按 `dispatch/PLAN.md` 领取新任务包 **{wp}**，"
        f"纪律与文件总线协议不变：开工先写 `dispatch/workers/{wid}.state` 为 `BUSY {wp}`，"
        f"完成后写报告到 `dispatch/reports/{wp}.md` 再置 `READY {wp}`。开始。"
    )
    ok, ev = send_text(msg, wid)
    return ok, ev


def claim_next_wp(blocks: dict) -> str:
    for wp, status in blocks.items():
        if status == "待派":
            return wp
    return ""


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--probe", action="store_true", help="只读体检")
    ap.add_argument("--dry-run", action="store_true", help="演练，零 UI 动作")
    ap.add_argument("--start", type=int, metavar="N", help="发车：建 N 个新会话并派首批")
    ap.add_argument("--watch", action="store_true", help="守护补位直到 PLAN 清空")
    ap.add_argument("--workers", type=int, default=18)
    ap.add_argument("--interval", type=int, default=20, help="守护轮询间隔秒")
    ap.add_argument("--stale-min", type=int, default=120, help="BUSY 超时分钟数（提醒线）")
    ap.add_argument("--spec", default="docs/kernel-spec.md", help="规格 MD 路径")
    ap.add_argument("--skills",
                    default=",".join(vb.SKILLS_DEFAULT),
                    help="逗号分隔技能清单（渲染为 @skill: 提及）")
    ap.add_argument("--no-bootstrap", action="store_true",
                    help="关闭引导三件套（不选工作空间/不挂文件）")
    ap.add_argument("--workspace", default=vb.WS_DEFAULT,
                    help="新会话工作空间名（默认 -Un-Real-0d23d9ux-Engine-main）")
    ap.add_argument("--files", default="default",
                    help='逗号分隔的附件绝对路径；"default"=内置4文件；""=不挂文件')
    ap.add_argument("--out", default="kernel-wp", help="工人产物根目录")
    a = ap.parse_args()

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
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)

    if a.probe:
        sys.path.insert(0, str(Path(__file__).parent))
        sys.exit(__import__("check_tower_dom").main())

    if not st.devtools_alive():
        print("[ERR] 9222 不通。先跑 开端口.bat")
        return 1

    plan_text, blocks = parse_plan()
    pending = [w for w, s in blocks.items() if s == "待派"]
    log(f"PLAN 概览：总 {len(blocks)} 包，待派 {len(pending)}，"
        f"进行中 {sum('进行中' in s for s in blocks.values())}，"
        f"DONE {sum(s == 'DONE' for s in blocks.values())}")

    if a.dry_run:
        wid = "W01"
        wp = claim_next_wp(blocks) or "(无待派)"
        print("\n── DRY-RUN 演练 ──")
        print(f"首条提示词（{wid} / {wp}）渲染结果预览：\n")
        print(render_first_prompt(wid, wp, a.spec, a.skills, a.out)[:1200])
        print("……")
        ok, why = idle3()
        print(f"\n当前忙闲：idle={ok}（{why}）")
        print(f"侧栏会话数：{len(snapshot_conv_ids())}")
        print("演练结束（零 UI 动作）。正式发车：python tools/dispatch_tower.py --start 18 --watch")
        return 0

    if not a.start and not a.watch:
        print(" nothing to do：加 --start N / --watch / --probe / --dry-run")
        return 0

    # ── 发车：建 N 个会话 + 派首批 ──
    if a.start:
        known = snapshot_conv_ids()
        created = 0
        for i in range(1, a.start + 1):
            wid = f"W{i:02d}"
            if read_conv(wid):
                log(f"{wid} 已有会话 {read_conv(wid)[:12]}，跳过新建")
                continue
            wp = claim_next_wp(parse_plan()[1])
            if not wp:
                log("PLAN 已无待派，停止新建")
                break
            prompt = render_first_prompt(wid, wp, a.spec, a.skills, a.out)
            log(f"{wid} 新建会话并派 {wp} …")
            conv, ev = new_conversation(prompt, known)
            if not conv:
                log(f"[FAIL] {wid} 新建失败：{ev}")
                ledger("发车失败", wp, wid, ev)
                continue
            known.add(conv)
            write_conv(wid, conv)
            write_state(wid, f"BUSY {wp}")
            plan_text = mark_plan(plan_text, wp, f"进行中({wid})")
            PLAN.write_text(plan_text, encoding="utf-8")
            ledger("派工(新会话)", wp, wid, ev)
            created += 1
            time.sleep(2.0)
        log(f"发车完成：新建 {created} 路")

    # ── 守护：READY → 续单，直到 PLAN 清空 ──
    if a.watch:
        log(f"守护开始：{a.workers} 路，轮询 {a.interval}s，BUSY 超时线 {a.stale_min}min")
        try:
            while True:
                _, blocks = parse_plan()
                pending = claim_next_wp(blocks)
                all_done = not pending and all(
                    s == "DONE" for s in blocks.values()) if blocks else False
                busy_cnt = ready_cnt = 0
                for i in range(1, a.workers + 1):
                    wid = f"W{i:02d}"
                    state = read_state(wid)
                    conv = read_conv(wid)
                    if state.startswith("BUSY"):
                        busy_cnt += 1
                        # 超时提醒（不动手，只记录——人工裁决是否换道）
                        mtime = (WORKERS_DIR / f"{wid}.state").stat().st_mtime
                        if (time.time() - mtime) > a.stale_min * 60:
                            log(f"[STALE] {wid} BUSY 超 {a.stale_min}min（{state}）——"
                                f"建议去该会话喊一声报进度")
                    elif state.startswith("READY"):
                        ready_cnt += 1
                        if not conv:
                            log(f"[WARN] {wid} READY 但没有 conv 记录，跳过")
                            continue
                        if not pending:
                            continue
                        # 只在当前页面空闲时动手（全局串行发送，天然互斥）
                        ok, why = idle3()
                        if not ok:
                            continue
                        ok2, ev = dispatch_next(wid, conv, pending)
                        if ok2:
                            plan_text = mark_plan(plan_text, pending,
                                                  f"进行中({wid})")
                            write_state(wid, f"BUSY {pending}")
                            PLAN.write_text(plan_text, encoding="utf-8")
                            ledger("补位派工", pending, wid, ev)
                            log(f"{wid} ← {pending}（{ev}）")
                            pending = ""  # 本轮已用掉，下轮重新解析
                        else:
                            ledger("补位失败", pending, wid, ev)
                            log(f"[FAIL] {wid} ← {pending} 失败：{ev}")
                            time.sleep(5)
                remaining = sum(1 for s in blocks.values() if s == "待派")
                log(f"心跳：忙 {busy_cnt} / 空 {ready_cnt} / 待派 {remaining}")
                if all_done and ready_cnt == a.workers:
                    log("★ PLAN 全部 DONE 且全部工人空闲 —— 50000 计划收口 ★")
                    ledger("收口", "-", "调度塔", "PLAN 清空，产线完成")
                    return 0
                time.sleep(a.interval)
        except KeyboardInterrupt:
            log("人工停止（Ctrl+C）——正在跑的工人不受影响，重新 --watch 即恢复")
            ledger("停止", "-", "调度塔", "人工 Ctrl+C，工人继续跑")
            return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
