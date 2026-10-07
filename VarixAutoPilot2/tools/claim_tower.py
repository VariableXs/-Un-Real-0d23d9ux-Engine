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
     发完静置 --settle-min 分钟（默认 5）再体检：状态文件仍是塔占位
     （工人没自己领单/报到）= 没正常运行 → 归档旧会话并新建对话重来
     （找回的既有对话则点回原对话补发协议，不开新的）。
  2. 守护（--watch）：每 --interval 秒（默认 10s）轮询工人状态文件总线
     （dispatch/workers/Wxx.state）：
       READY            → 切到该会话 → 发续跑指令 → 工人去领下一单；
       BUSY             → 施工中不动它；超 --dead-min 分钟（默认 10）无动静 →
                          **归档重建**（写 dispatch/archive/ + 释放名下已领单 +
                          新建同编号工人会话继续）；--nudge-min 补发检查仍在
                          （dead_min=0 时作为唯一自愈手段）；
       会话 id 丢失      → 用首条提示词里的 VARIX-Wxx 标记在侧栏标题里重链。
  3. 积分监控与自动切号（--no-account-pool 关闭）：三信号任一命中即切号——
     ① 单轮发送失败 ≥3；② 页面积分提示连续 ≥2 次（toast/dialog 扫描，
     排除聊天区）；③ 阻塞单原因含积分关键词 ≥2 例（积分耗尽时塔的发送
     不耗积分、round_fail 恒 0，2026-10-07 实测只靠连败会漏掉整个场景，
     任务板阻塞单堆积停摆）；另有每 10 分钟预防性巡检 → 查 TreeCode
     账号池（127.0.0.1:8792）；池里有 ≥门槛余量的号 → 自动切号（TreeCode
     重启 WorkBuddy，塔等待 CDP 恢复并兜底带参重启）。切号异常自动换号
     （2026-10-06 Variable 指定）：切号失败 / WorkBuddy 未恢复 / 切入后
     账号显示异常（疑似封号）→ 拉黑该号换下一个，直到切到可用号；
     全池耗尽 → 判定「无可用积分账号」→ 优雅停机。切号成功后自动把
     积分阻塞单逐个 /api/release 重排回待领（任务继续做下去）；切号后
     先看 18 路对话存活情况（Variable 指定）：≥2/3 会话还在且状态真实 →
     保全续用（逐路点回旧对话唤醒 + 静置 --settle-min 分钟体检补活，
     不释放任务不整体重建）；不足 → 释放全部在途任务 + 重建 18 路。
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
  6. 视图归属守卫（2026-10-06 Variable 指定）：您正在查看的对话绝不被写入——
     填充前后各校验一次活跃会话（续跑=必须等于目标 conv_id；新建=不得落进
     旧会话集合），漂移即拦截不发送；引导三件套每阶段之间同样校验；
     续跑/发车先在侧栏全列表（含滚动加载）里找回既有对话，新建只是最后手段。
  7. 异常护栏 = 24h 无人值守（2026-10-07 实测教训）：页面主线程卡顿>20s
     时 CDP recv 超时曾一路穿到 main 带崩塔。此后 call_js 传输异常不外抛
     （返回 None，读不到状态=判忙，保守安全）；塔内 CDP 重动作全部经
     guarded——单点异常记日志+账本后返回默认值继续跑；bat 对非 0/2 退出码
     60s 自动重启（0=优雅停止/收口，2=无账号停机，均不重启）。

用法（在 VarixAutoPilot2/ 下）：
  python tools/claim_tower.py --probe                # 只读体检（CDP/任务板/DOM）
  python tools/claim_tower.py --dry-run              # 演练：渲染协议/看忙闲，零 UI 动作
  python tools/claim_tower.py --start 18 --watch     # ★ 正式发车：建 18 对话+守护到收口
  python tools/claim_tower.py --watch                # 只守护（会话已建好/塔重启接管）
  可选：--workers N --interval S --nudge-min M --settle-min M --board-port P --board-exe 路径
  引导三件套（默认开，2026-10-06 上线）：新会话自动选工作空间
  -Un-Real-0d23d9ux-Engine-main、技能经「/」面板按显示名挂载为技能提及节点
  （挂载失败的回退为 @skill: 内联进首条提示词）、拖拽挂 4 份文件
  （VTaskBoard.exe + 3 份总纲 MD）；--no-bootstrap 关闭，
  --workspace/--files/--skills 可自定义；自检：python tools/bootstrap_selftest.py
"""

import argparse
import json
import re
import subprocess
import sys
import time
import urllib.parse
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
# 积分耗尽信号关键词（2026-10-07 实测教训：积分用完时工人 AI 失败会把任务标
# 「阻塞」并写原因、页面弹「积分不足」类提示；而塔往输入框发字不耗积分，
# round_fail 恒为 0——只靠发送连败触发切号会彻底漏掉这个场景，任务板
# 阻塞单堆积、产线停摆。三信号任一命中即切号，切号成功后自动重排阻塞单）
CREDIT_KW = ("积分不足", "积分不够", "积分用尽", "积分用完", "积分已用完",
             "积分耗尽", "余额不足", "额度不足", "配额不足")


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


def guarded(label: str, fn, *a, default=None, **kw):
    """单点动作装甲（2026-10-07，24h 无人值守铁律）：任何异常不许带崩塔。

    实测教训：WorkBuddy 页面主线程卡顿 >20s 时 call_js recv 超时
    TimeoutError 曾一路穿到 main 带崩整个塔。此后凡调 CDP/文件/网络
    的重动作一律经本护栏：KeyboardInterrupt 照常穿透（人工停止不受
    影响），其余 Exception 记日志+账本后返回 default，塔继续跑。
    """
    try:
        return fn(*a, **kw)
    except KeyboardInterrupt:
        raise
    except Exception as e:
        log(f"[EXC] {label}：{type(e).__name__}: {e}（塔继续运行，"
            f"下轮自动重试）")
        ledger("异常护栏", "-", label, f"{type(e).__name__}: {e}")
        return default


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

# 页面积分耗尽提示扫描（2026-10-07）：只扫 toast/dialog/modal 弹层容器且
# 排除 .cr-message-list 聊天区——工人对话里可能转述「积分不足」，正文区
# 一律不算，只有真实弹层命中才算信号（防误报触发无谓切号重启）。
JS_CREDIT_PROMPT = r"""(() => {
  const KW = /(积分不足|积分不够|积分用尽|积分用完|积分已用完|积分耗尽|余额不足|额度不足|配额不足)/;
  const vis = (e) => { const r = e.getBoundingClientRect();
    return r.width > 4 && r.height > 4; };
  const ml = document.querySelector('.cr-message-list');
  const sel = '[class*="toast"],[class*="Toast"],[class*="dialog"],[class*="Dialog"],' +
              '[class*="modal"],[class*="Modal"],[class*="popover"],[class*="Popover"],' +
              '[class*="message-box"],[class*="notice"],[class*="snackbar"]';
  for (const e of document.querySelectorAll(sel)) {
    if (!vis(e) || (ml && ml.contains(e))) continue;
    const t = (e.innerText || '').trim();
    if (t && t.length <= 80 && KW.test(t))
      return { hit: true, where: 'overlay', text: t.slice(0, 50) };
  }
  return { hit: false };
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

# 滚动查找后点击：会话多后旧项被虚拟化/滚出渲染区（不在 DOM），fast path
# 找不到时先定位侧栏可滚动容器，从顶部渐进 scrollTop（两遍扫描，吃滚动
# 加载的分页），目标项渲染出来后走同样的完整 MouseEvent 序列点击。
JS_SCROLL_CLICK_CONV = r"""((cid) => {
  const find = () => document.querySelector(
    'div.conversation-item[data-conversation-id="' + cid + '"]');
  let el = find();
  let scrolled = false;
  if (!el) {
    const any = document.querySelector('div.conversation-item');
    let sc = null;
    for (let p = any && any.parentElement; p; p = p.parentElement) {
      if (p.scrollHeight > p.clientHeight + 40 &&
          /auto|scroll/.test(getComputedStyle(p).overflowY)) { sc = p; break; }
    }
    if (!sc) return { found: false, why: '目标不在 DOM 且侧栏无可滚动容器' };
    const step = Math.max(240, sc.clientHeight - 80);
    for (let pass = 0; pass < 2 && !el; pass++) {
      sc.scrollTop = 0;
      let done = 0;
      while (!el) {
        sc.scrollTop += step; done += step; scrolled = true;
        el = find();
        if (done > sc.scrollHeight * 1.5 + 4000) break;
      }
    }
    if (!el) return { found: false, why: '滚动全列表仍未找到该会话' };
  }
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
  return { found: true, scrolled: scrolled };
})("%s")"""

# 按标记（VARIX-Wxx）全列表搜索（只读，不点击）：含滚动加载的两遍扫描，
# 搜完把侧栏滚动位置还原（不动用户侧栏）。用于会话映射丢失时的重链。
# 第二参 exclude = 已归档会话 id 列表（卡死归档的旧对话标题同样带标记，
# 滚动全列表会把它们翻出来，必须跳过——归档的会话绝不复活）。
JS_FIND_CONV_BY_MARKER = r"""((marker, exclude) => {
  const items = () => Array.from(document.querySelectorAll(
    'div.conversation-item[data-conversation-id]'));
  const scan = () => {
    for (const e of items()) {
      const id = e.getAttribute('data-conversation-id');
      if (!id || exclude.includes(id)) continue;
      if ((e.innerText || '').includes(marker)) {
        return { id: id,
                 title: (e.innerText || '').trim().replace(/\s+/g, ' ').slice(0, 60) };
      }
    }
    return null;
  };
  let hit = scan();
  let scrolled = false;
  if (!hit) {
    const any = items()[0];
    let sc = null;
    for (let p = any && any.parentElement; p; p = p.parentElement) {
      if (p.scrollHeight > p.clientHeight + 40 &&
          /auto|scroll/.test(getComputedStyle(p).overflowY)) { sc = p; break; }
    }
    if (sc) {
      const top0 = sc.scrollTop;
      const step = Math.max(240, sc.clientHeight - 80);
      for (let pass = 0; pass < 2 && !hit; pass++) {
        sc.scrollTop = 0;
        let done = 0;
        while (!hit) {
          sc.scrollTop += step; done += step; scrolled = true;
          hit = scan();
          if (done > sc.scrollHeight * 1.5 + 4000) break;
        }
      }
      sc.scrollTop = top0;
    }
  }
  return hit ? { found: true, scrolled: scrolled, id: hit.id, title: hit.title }
             : { found: false, why: '全列表（含滚动）未命中标记 ' + marker };
})("%s", %s)"""

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


def _target_ok(now_active: str, expect) -> tuple[bool, str]:
    """目标守卫判定。expect: str=精确相等 / set=不在其中（旧会话黑名单）。"""
    lbl_now = f"会话[{now_active[:12]}]" if now_active else "无选中视图（空白/新会话）"
    if isinstance(expect, str):
        if now_active == expect:
            return True, ""
        want = f"会话[{expect[:12]}]" if expect else "空白新建视图"
        return False, (f"当前活跃 {lbl_now} ≠ 目标 {want}"
                       f"（窗口可能正被您查看/操作）")
    if isinstance(expect, set):
        if now_active not in expect:
            return True, ""
        return False, (f"当前活跃 {lbl_now} 是旧会话（您可能正在查看它）——拦截")
    return True, ""


def send_text(text: str, tag: str, expect_active=None) -> tuple[bool, str]:
    """填入 + 点发送 + 受理证据。调用方保证目标会话已空闲。

    expect_active（目标守卫，2026-10-06 Variable 指定「提示词不许进到我
    正在看的对话」）：
      None → 不校验（accept_tower 等旧调用方兼容）；
      str  → 当前活跃会话必须等于它（续跑路径传 conv_id）；
      set  → 当前活跃会话不得属于该集合（新建路径传旧会话 id 黑名单：
             空白视图 active='' 或新会话自身 id 都放行）。
    填充前 + 点击发送前各校验一次；漂移即中止并显性记日志，绝不发送。
    续跑路径（str 且非空）填充前检查编辑器残留：含 VARIX-（上次失败填充
    的遗留）→ 先清空防双份（insertText 是追加不是替换）；是非塔内容
    （可能是您的草稿）→ 拒绝覆盖并显性报错。
    """
    now = active_conv()
    if expect_active is not None:
        ok_t, why_t = _target_ok(now, expect_active)
        if not ok_t:
            return False, f"填充前视图漂移拦截：{why_t}"
    if isinstance(expect_active, str) and expect_active:
        s0 = st.probe_state() or {}
        if (s0.get("charsReal") or 0) > 0:
            has_varix = st.call_js(
                '(() => { const e = document.querySelector('
                "'div[data-slate-editor=\"true\"][contenteditable=\"true\"]');"
                " return e ? (e.innerText||'').includes('VARIX-') : false; })()")
            if has_varix:
                st.clear_editor_cdp()
            else:
                return False, ("目标会话输入框已有非塔内容（可能是您的草稿）"
                               "——拒绝覆盖，本轮放弃；请手动处理后重试")
    ok, ev = st.fill_cdp(text)
    if not ok:
        return False, f"填充失败：{ev}"
    time.sleep(0.6)
    if expect_active is not None:
        ok_t, why_t = _target_ok(active_conv(), expect_active)
        if not ok_t:
            # 文本可能已落进漂移后的编辑器：仅当内容确属我们（含唯一 tag）
            # 时才清掉，绝不动用户自己的草稿
            tag_js = json.dumps(tag, ensure_ascii=False)
            has_ours = st.call_js(
                '(() => { const e = document.querySelector('
                "'div[data-slate-editor=\"true\"][contenteditable=\"true\"]');"
                " return e ? (e.innerText||'').includes(" + tag_js + ") : false; })()")
            extra = ""
            if has_ours:
                st.clear_editor_cdp()
                extra = "；检测到塔文本已落进漂移视图，已清空"
            return False, f"点击发送前视图漂移拦截：{why_t}{extra}"
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
    """切到指定会话并确认真的切过去了（D3/D9/D14 同源教训全吸收）。

    目标项不在 DOM（侧栏长列表虚拟化/滚出渲染区）时，先渐进滚动侧栏把
    它找出来再点——续跑必须点进既有对话，旧对话被滚没导致找不到就算
    失败重试，绝不静默改开新对话（2026-10-06 Variable 指定）。
    """
    for attempt in range(2):
        r = st.call_js(JS_CLICK_CONV % conv_id)
        if not (isinstance(r, dict) and r.get("found")):
            r = st.call_js(JS_SCROLL_CLICK_CONV % conv_id)
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
PROTOCOL_TEMPLATE = """hello 去在这个软件领一下任务并完成，然后在领任务的时候，注意标记一下，不要让其他AI 领到同样的任务呀还有，全面完整的分析要完成的所有内容，每一个要完成的功能和内容都要达到验收标准和深度打磨的情况下才算完成呀，不要完成其他AI的功能啦，直到全部任务56000个任务全部完成时才算结束呀，还有推送github仓库时，不要把任何非功能代码，截图，比如日志，测试代码还有脚本，提交，还有提交的介绍，只写纯功能的内容就可以了，然后先写完代码然后再进行修复呀，全部功能围绕这内核进行全部为Rust，祝你好运呀，一定要深度打磨哦

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
- **全部功能围绕内核进行、全部为 Rust**：功能实现一律围绕本引擎的 Rust
  内核展开——在 Rust 内核工程里扩展、与内核集成；功能代码全部用 Rust 写，
  不用其他语言另写功能实现，也不做与内核无关的旁支功能。
- **TypeScript 存量全面迁移（Variable 2026-10-07 指定）**：仓库既有的
  TypeScript（src/ 下约 1400 个 .ts/.tsx：system/features/lib/apps 等）不豁免。
  领到的任务落在 TS 区域时：**先把本任务涉及的既有 TS 模块用 Rust 重新实现**
  （迁进 Rust 内核工程；UI 层可用 Rust 前端方案，选型记进完成报告），
  再在 Rust 实现上完成功能；迁移与功能一并按验收标准交付，完成报告注明
  「迁移了哪些 TS 文件 → 哪些 Rust 模块」。只迁本任务涉及的模块，
  不做任务范围外的大迁移，不碰范围外的 TS 文件。
- 纪律：动手前 git status；只 add 显式路径；源码只增不减不移动——
  唯一例外：被本次迁移的 Rust 实现所替代的 TS 文件，Rust 版验收通过且
  确认无其他引用后可删除（仅限被替代文件）；异常零静默——
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

_relink_last: dict[str, float] = {}   # wid → 上次全量重链扫描时间（节流）
_archived_ids: dict[str, set] = {}    # wid → 已归档会话 id（重链时排除）


def archived_conv_ids(wid: str) -> set:
    """从 dispatch/archive/日期/Wxx_*.md 回收已归档会话 id。

    卡死归档的旧对话标题同样带 VARIX-Wxx 标记且永久留在侧栏历史里；
    重链滚动全列表时必须把它们排除——归档的会话绝不复活顶替新会话。
    结果按 wid 缓存（归档文件只增不改）。
    """
    if wid in _archived_ids:
        return _archived_ids[wid]
    out = set()
    try:
        if ARCHIVE_ROOT.is_dir():
            for f in ARCHIVE_ROOT.glob(f"*/{wid}_*.md"):
                m = re.search(r"会话 id：`([^`]+)`", f.read_text(encoding="utf-8"))
                if m:
                    out.add(m.group(1).strip())
    except OSError as e:
        log(f"[WARN] 归档 id 回收失败（{wid}）：{e}")
    _archived_ids[wid] = out
    return out


def relink_conv(wid: str, min_gap_s: float = 60.0) -> str:
    """会话 id 丢失时按首条提示词头部的 VARIX-Wxx 标记在侧栏标题里重链。

    2026-10-06 升级：侧栏长列表会虚拟化（旧会话滚出渲染区就不在 DOM），
    现在做全量滚动搜索（两遍扫描，吃滚动加载），搜完还原滚动位置——
    找回既有对话永远优先于新建（Variable 指定：续跑点进旧对话，不开新的）。
    已归档会话 id 一律排除（见 archived_conv_ids）。
    min_gap_s 节流：守护循环每轮都会来问，全量扫描同一工人 60s 内只跑一次。
    """
    now = time.time()
    if now - _relink_last.get(wid, 0.0) < min_gap_s:
        return ""
    _relink_last[wid] = now
    marker = f"VARIX-{wid}"
    exclude = sorted(archived_conv_ids(wid))
    try:
        r = st.call_js(JS_FIND_CONV_BY_MARKER
                       % (json.dumps(marker), json.dumps(exclude)))
        if isinstance(r, dict) and r.get("found"):
            write_conv(wid, r["id"])
            log(f"{wid} 会话重链成功 → {r['id'][:12]}…"
                f"（{'滚动全列表找回' if r.get('scrolled') else '当前列表命中'}，"
                f"标题≈{r.get('title', '')}）")
            ledger("会话重链", "-", wid, r["id"][:16])
            return r["id"]
        why = r.get("why") if isinstance(r, dict) else r
        if exclude:
            why += f"（已排除 {len(exclude)} 个归档会话）"
        log(f"[WARN] {wid} 重链未命中：{why}")
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

    防污染双保险（2026-10-06 Variable 指定「提示词不许进到我正在看的对话」）：
      A. 引导全程守卫：工作空间/逐技能/拖文件每个阶段之间都确认视图没有
         落进 known_ids（旧会话 = 您可能正在查看的那批），落进即中止引导，
         且不点「新建任务」抢视图，把窗口原样还给您；
      B. 发送守卫：send_text(expect_active=known_ids) 填充前后各校验一次，
         漂移即拦截。
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
    drift = {"hit": False, "why": ""}

    def guard():
        now = active_conv()
        if now and now in known_ids:
            drift["hit"] = True
            drift["why"] = (f"引导中视图被切到旧会话[{now[:12]}]"
                            f"（您可能正在查看）→ 立即中止，不动您的窗口")
            return False, drift["why"]
        return True, ""

    if BOOT["on"]:
        skills = [s.strip() for s in SKILLS_ARG.split(",") if s.strip()]
        ok_b, ev_b = vb.run(st, BOOT["ws"], BOOT["files"], skills=skills,
                            guard=guard)
        if not ok_b:
            if drift["hit"]:
                # 视图已被用户接管：绝不点「新建任务」抢窗口，原样让位
                log(f"[让位] {drift['why']}")
                return "", f"引导中止（让位用户视图）：{drift['why']}"
            vb.cleanup_reset(st, JS_CLICK_NEW)
            return "", f"引导失败（已重置空白）：{ev_b.get('why_guard') or ev_b}"
        mounted = ev_b.get("skills_ui", {}).get("mounted", [])
        fallback = ev_b.get("skills_fallback", [])
        log(f"引导：工作空间✓ 技能 / 面板挂载 {len(mounted)}/{len(skills)}"
            f"（内联兜底 {len(fallback)}：{','.join(fallback) or '无'}） 文件✓")
    else:
        fallback = None   # 未开引导 → 技能全走内联 @skill:
    ok, ev = send_text(first_prompt_fn(fallback), tag,
                       expect_active=set(known_ids))
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
    """重新点进该工人的既有会话 → 确认空闲 → 发指令。

    2026-10-06 Variable 指定：续跑永远「点回旧对话发送」，绝不开新对话。
    expect_active=conv_id：wait_idle 期间用户若点开别的对话，填充前会被
    拦截（绝不把续跑指令发进您正在看的对话）；目标项被侧栏虚拟化滚出
    渲染区时 switch_conv 会先滚动找回再点。
    """
    if not switch_conv(conv_id):
        return False, f"切会话失败（含滚动查找）：{conv_id[:12]}"
    if not wait_idle(60):
        return False, "切过去后 60s 仍忙，本轮回头再试"
    return send_text(msg, f"VARIX-{wid}·塔", expect_active=conv_id)


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


def blocked_credit_hits() -> dict:
    """阻塞单里的积分耗尽信号统计（只读 /api/tasks?status=阻塞，最多 2000 单）。

    返回 {"hits": 命中数, "blocked": 阻塞总数, "total": 板上总数, "samples": [...]}。
    任务板不通时抛异常，由调用方 guarded/try 处理。
    """
    ts = board_api(f"/api/tasks?status={urllib.parse.quote('阻塞')}&limit=2000",
                   timeout=8)
    tasks = ts.get("tasks") or []
    hits = [t for t in tasks
            if t.get("状态") == "阻塞"
            and any(k in (t.get("结果") or "") for k in CREDIT_KW)]
    return {"hits": len(hits),
            "blocked": sum(1 for t in tasks if t.get("状态") == "阻塞"),
            "total": ts.get("total", len(tasks)),
            "samples": [f"{t.get('id')}:{(t.get('结果') or '')[:24]}"
                        for t in hits[:3]]}


def requeue_credit_blocked(fuse: int = 500) -> str:
    """把「因积分耗尽被卡住」的阻塞单逐个退回待领（POST /api/release，单任务
    端点，服务端任意状态可退——2026-10-07 实读源码确认）。

    只退原因含积分关键词的单：其他阻塞（前置缺失/真实障碍）退了也会被再次
    阻塞，还污染验收口径。切号成功后调用（Variable 指定：任务要继续做下去，
    不重排的话这些单永远躺在阻塞栏没人再领）。fuse=单轮重排上限（保险丝）。
    返回 "重排 N 单"。
    """
    freed, off, page = [], 0, 2000
    while len(freed) < fuse:
        ts = board_api(f"/api/tasks?status={urllib.parse.quote('阻塞')}"
                       f"&limit={page}&offset={off}", timeout=8)
        tasks = ts.get("tasks") or []
        if not tasks:
            break
        for t in tasks:
            if len(freed) >= fuse:
                break
            if t.get("状态") == "阻塞" \
                    and any(k in (t.get("结果") or "") for k in CREDIT_KW):
                try:
                    r = board_api("/api/release", {"id": t.get("id")}, timeout=8)
                    if r.get("ok"):
                        freed.append(str(t.get("id")))
                except Exception:
                    pass   # 单个失败不放弃整批（下轮切号/巡检再试）
        off += len(tasks)
        if len(tasks) < page:
            break
    if freed:
        ledger("积分阻塞重排", "-", "领单塔", f"{len(freed)} 单退回待领")
    return f"重排 {len(freed)} 单"


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
    _archived_ids.pop(wid, None)   # 归档缓存失效：后续重链立即可见新归档 id
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


def post_start_check(started: int, settle_min: float, reused: set,
                     fresh_since: float | None = None) -> None:
    """发车后静置体检（2026-10-06 Variable 指定）：N 路开完先零干预跑满
    settle 分钟，再逐路核对有没有「没正常运行」的，有就新建对话完成。

    判据（只读状态文件总线，零视图侵入、不切会话不打扰您）：
      健康 = 状态文件被工人本人改写过（领到单写 BUSY <任务id>，或写 READY 报到）；
      异常 = 状态还是塔写的占位「BUSY claim」（协议发出 5 分钟工人没动）/
             状态为空 / 会话映射缺失（强制全列表重链一次仍找不回）。
    fresh_since（切号保全体检用，2026-10-06 Variable 指定）：给定时刻后还要求
      状态文件有工人本人的心跳（mtime ≥ fresh_since）——切号唤醒后静置期内
      没心跳 = 唤醒未生效，状态内容再真实也按异常处理（归档旧会话新建重来）。
    处置（分来源，两种都不违背「续跑必进旧对话」铁律）：
      找回的既有对话（reused）→ 点回原对话补发协议，绝不为它开新对话；
      本次新建的对话 → 归档旧会话（释放名下任务 + 断绝重链复活）后
                       新建同编号对话重发协议全文（Variable 指定：
                       没正常运行的「新建对话完成」）。
    """
    log(f"静置 {settle_min:g} 分钟开始（期间零干预，让工人自己起跑）…")
    deadline = time.time() + settle_min * 60.0
    next_ping = time.time() + 60.0
    while True:
        remain = deadline - time.time()
        if remain <= 0:
            break
        time.sleep(min(5.0, remain))
        if time.time() >= next_ping:
            next_ping = time.time() + 60.0
            log(f"静置中… 体检还剩 {max(0, int((deadline - time.time()) / 60))} 分钟")
    log(f"发车体检：逐路核对 {started} 路（健康=工人已自己领单/报到）")
    ok_cnt = fixed = 0
    for i in range(1, started + 1):
        wid = f"W{i:03d}"
        state = read_state(wid).strip()
        conv = read_conv(wid).strip()
        if not conv:
            conv = relink_conv(wid, min_gap_s=0.0)   # 体检期强制全列表搜一次
        fresh_ok = not (state and fresh_since is not None
                        and state_mtime(wid) < fresh_since)
        if state and state != "BUSY claim" and fresh_ok:
            ok_cnt += 1
            log(f"[体检] {wid} ✓（{state[:48]}）")
            continue
        if state == "BUSY claim":
            why = "状态仍是塔占位 BUSY claim（协议发出后工人没动）"
        elif not state:
            why = "状态为空"
        else:
            why = (f"状态无唤醒后心跳（{state[:32]}… 是静置开始前的旧状态，"
                   f"唤醒未生效）")
        if not conv:
            why += "，且会话映射缺失（全列表重链未找回）"
        log(f"[体检] {wid} ✗ {why} → 处置")
        if conv and wid in reused:
            # 找回的既有对话：点回原对话补发协议（绝不为它开新对话）
            ok, ev = dispatch_continue(wid, conv, render_protocol(wid))
            if ok:
                write_state(wid, "BUSY claim")
                fixed += 1
                ledger("体检补发", "-", wid, f"{why} → 已点回原对话补发（{ev}）")
                log(f"[体检] {wid} 已点回旧对话补发协议 ✓")
            else:
                ledger("体检补发失败", "-", wid, f"{why}；{ev}")
                log(f"[FAIL] [体检] {wid} 补发失败：{ev}（交守护循环继续跟进）")
            time.sleep(1.5)
            continue
        # 本次新建的对话没跑起来 → 归档旧会话 + 新建对话完成
        if conv:
            arch = archive_dead_worker(wid, conv, state, settle_min,
                                       f"发车体检：{why}")
            log(f"[体检] {wid} 旧会话已归档 → {arch.name}")
        else:
            release_worker_tasks(wid)   # 防看不见的孤儿已领单
            write_state(wid, "")
        conv2, ev2 = rebuild_worker(wid)
        if conv2 or "发送成功" in ev2:
            fixed += 1
            ledger("体检重建", "-", wid, f"{why} → 新建对话重发协议（{ev2}）")
            log(f"[体检] {wid} 新建对话完成 ✓ {ev2}")
        else:
            ledger("体检重建失败", "-", wid, f"{why}；{ev2}")
            log(f"[FAIL] [体检] {wid} 新建失败：{ev2}（交守护循环继续跟进）")
        time.sleep(2.5)
    log(f"发车体检完成：健康 {ok_cnt} / 修复 {fixed} / 共 {started} 路")
    ledger("发车体检", "-", "领单塔",
           f"{settle_min:g}min 静置后：健康 {ok_cnt}，修复 {fixed}，共 {started}")


def switch_account_flow(reason: str, workers: int,
                        settle_min: float = 5.0) -> str:
    """积分耗尽 → TreeCode 切号 → 等 WorkBuddy 回来 → 按存活情况续用或重建。

    返回："ok" / "no_account"（无可用积分账号 → 停机信号）/"fail:<why>"。

    切号循环（2026-10-06 Variable 指定「账号异常可能被封，继续切直到可用」）：
      候选号最富优先逐个试；切号失败 / WorkBuddy 未恢复 / 切入后账号显示
      异常（ok=false 或余量不达标，疑似封号）→ 拉黑该号换下一个，
      直到切到可用号；全池耗尽 → no_account 停机。

    WorkBuddy 恢复后的对话保全检查（2026-10-06 Variable 指定「先看 18 个
    对话还在不在、还跑不跑」）：切号会重启客户端、打断所有生成——
      存活 ≥2/3（会话还在侧栏且状态文件是工人本人写的真实状态）
        → 保全续用：不释放任务、不作废映射，逐路点回旧对话发唤醒
          （nudge 对任何状态安全），静置 settle_min 分钟后走发车体检
          （fresh_since=唤醒完成时刻：静置期内有工人本人心跳=真恢复；
          没心跳=唤醒未生效 → 归档旧会话新建对话重来）；
      存活不足 2/3 → 释放全部在途任务 + 作废全部映射 + 逐路新建（原路径）。
    """
    log(f"[积分] 触发切号检查（{reason}）…")
    banned: set[str] = set()
    label, ev, remain_txt = "", "", "?"
    while True:
        info = apool.probe_all(REMAIN_MIN)
        if not info.get("ok"):
            return f"fail:账号池不可读 {info.get('why')}"
        log(f"[积分] 池子 {info['accounts']} 号 / 总余 {info['pool_remain']} / "
            f"最高 {info['richest']}({info['richest_remain']}) / 可切 "
            f"{info['switchable']} 号"
            + (f"（已拉黑异常号 {len(banned)} 个）" if banned else ""))
        target = apool.pick_richest(REMAIN_MIN, exclude_labels=banned)
        if not target:
            ledger("停机", "-", "领单塔",
                   f"无可用积分账号（门槛 {REMAIN_MIN}）"
                   + (f"；{len(banned)} 号异常/疑似封号已跳过：{sorted(banned)}"
                      if banned else f"：池 {info}"))
            return "no_account"
        label = target.get("label")
        log(f"[切号] → {label}（余 {target.get('remain')}）。TreeCode 将重启 WorkBuddy…")
        ledger("切号", "-", "领单塔",
               f"→ {label} 余 {target.get('remain')}（{reason}）")
        ok, msg = apool.switch(label)
        if not ok:
            log(f"[FAIL] 切号 {label} 失败：{msg} → 疑似异常号，拉黑换下一个")
            banned.add(label)
            ledger("切号跳过", "-", "领单塔", f"{label} 切号失败：{msg}")
            continue
        ok_b, ev = apool.wait_workbuddy_back(log=log)
        if not ok_b:
            log(f"[FAIL] 切到 {label} 后 WorkBuddy 未恢复：{ev} → 疑似异常号，拉黑换下一个")
            banned.add(label)
            ledger("切号跳过", "-", "领单塔", f"{label} WorkBuddy 未恢复：{ev}")
            continue
        # 切入后体检该号：显示异常（可能封号）→ 拉黑换下一个
        remain_txt = str(target.get("remain"))
        try:
            time.sleep(3)   # TreeCode 登录落账要几秒
            cur = next((a for a in apool.accounts(apool.quota(fresh=True))
                        if a.get("label") == label), None)
            if cur is None or not cur.get("ok") \
                    or (cur.get("remain") or 0) < REMAIN_MIN:
                log(f"[FAIL] {label} 切入后显示异常"
                    f"（ok={cur.get('ok') if cur else '无记录'}，"
                    f"remain={cur.get('remain') if cur else '无记录'}）"
                    f"→ 疑似封号，拉黑换下一个")
                banned.add(label)
                ledger("切号跳过", "-", "领单塔", f"{label} 切入后异常/疑似封号")
                continue
            remain_txt = str(cur.get("remain"))
        except Exception as e:
            log(f"[WARN] {label} 切入后账号体检失败（{e}）→ 不拉黑，交给运行期判据")
        log(f"[切号] {label} 可用（余 {remain_txt}）✓")
        break

    # 积分耗尽卡死的阻塞单 → 新账号就绪后逐个退回待领（Variable 2026-10-07
    # 指定「任务要继续做下去」：不重排的话这些单永远躺在阻塞栏没人再领）
    try:
        rq = requeue_credit_blocked()
        if rq != "重排 0 单":
            log(f"[切号] 积分阻塞单已重排：{rq}")
    except Exception as e:
        log(f"[WARN] 积分阻塞单重排失败：{e}（不影响切号，下轮再试）")

    # ── WorkBuddy 已恢复：先看 18 路对话还在不在、还跑不跑 ──
    mapped = {read_conv(f"W{i:03d}") for i in range(1, workers + 1)}
    mapped.discard("")
    snapshot: set = set()
    deadline = time.time() + 60.0   # 侧栏渲染要时间；我们的对话出现才早停
    while time.time() < deadline:
        time.sleep(3)
        snapshot = snapshot_conv_ids()
        if snapshot and (snapshot & mapped):
            break
    alive: list[str] = []
    for i in range(1, workers + 1):
        wid = f"W{i:03d}"
        conv = read_conv(wid)
        state = read_state(wid).strip()
        # 存活判据：会话还在侧栏 + 状态文件是工人本人写的真实状态
        # （BUSY <任务id>/READY；塔占位 BUSY claim 或空 = 从没跑起来过）
        if conv and conv in snapshot and state and state != "BUSY claim":
            alive.append(wid)
    keep_min = max(1, (workers * 2) // 3)   # 「绝大部分」= ≥2/3
    if len(alive) >= keep_min:
        log(f"[切号] {len(alive)}/{workers} 路对话存活且状态真实（≥2/3）"
            f"→ 保全续用：不释放任务，逐路点回旧对话唤醒后静置体检")
        ledger("切号保全", "-", "领单塔",
               f"{label} 就绪（余 {remain_txt}）；{len(alive)}/{workers} 路存活"
               f"→ 唤醒+静置体检（不整体重建）")
        # 切号重启打断了所有生成：逐路点回旧对话发唤醒（nudge 对任何状态安全）
        for wid in alive:
            conv = read_conv(wid)
            state = read_state(wid)
            ok, ev2 = dispatch_continue(wid, conv, nudge_msg(wid))
            if ok:
                write_state(wid, state)   # 原内容覆盖 = 刷新 mtime（唤醒送达）
                log(f"[切号唤醒] {wid} ✓")
            else:
                log(f"[WARN] [切号唤醒] {wid} 失败：{ev2}（静置体检会处理）")
            time.sleep(1.5)
        if settle_min > 0:
            # fresh_since=唤醒完成之后：静置期内有工人本人心跳=真恢复；
            # 没心跳（唤醒未生效）→ 体检按异常处理：归档旧会话新建对话重来
            post_start_check(workers, settle_min, reused=set(),
                             fresh_since=time.time())
        return "ok"
    # 存活不足 → 全量重建（原路径）
    log(f"[切号] 仅 {len(alive)}/{workers} 路存活（不足 2/3）"
        f"→ 释放全部在途任务并重建 {workers} 路")
    ledger("切号完成", "-", "领单塔",
           f"{label} 就绪（余 {remain_txt}，跳过异常号 {len(banned)} 个），{ev}")
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
    ap.add_argument("--settle-min", type=float, default=5.0,
                    help="发车后静置多少分钟再体检：状态仍是塔占位/为空/会话"
                         "缺失（工人没自己领单或报到）的 → 新建对话完成"
                         "（默认 5，0=关；找回的既有对话只补发不重建）")
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

    # ── 发车：能找回既有对话就继续用，实在没有才建新会话 + 派首批协议 ──
    if a.start:
        known = snapshot_conv_ids()
        created = 0
        reused: set[str] = set()   # 找回的既有对话（体检异常时点回原对话补发）
        for i in range(1, a.start + 1):
            wid = f"W{i:03d}"
            conv = read_conv(wid)
            if not conv:
                conv = relink_conv(wid)   # 全列表滚动搜索：找回既有对话优先
                if conv:
                    ledger("发车找回", "-", wid, f"既有会话 {conv[:16]}，跳过新建")
            if conv:
                known.add(conv)
                reused.add(wid)
                log(f"{wid} 已有会话 {conv[:12]}…，继续用它（不另开新对话）")
                continue
            prompt_fn = lambda missing: render_protocol(wid, missing)  # noqa: E731
            log(f"{wid} 新建会话并派协议 …")
            conv, ev = guarded(f"{wid} 新建会话", new_conversation,
                               prompt_fn, known, f"VARIX-{wid}",
                               default=("", "异常（见塔日志，下轮自愈）"))
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
        # ── 发车后静置体检（Variable 2026-10-06 指定）：先零干预跑 5 分钟，
        #    再逐路核对有没有没正常运行的 → 新建对话完成（0 = 关闭）──
        if a.settle_min > 0:
            guarded("发车体检", post_start_check,
                    a.start, a.settle_min, reused)

    # ── 守护：READY 补发 / 卡死归档重建 / 积分切号 / 收口判定 ──
    if a.watch:
        log(f"守护开始：{a.workers} 路，轮询 {a.interval}s，"
            f"卡死线 {a.dead_min}min（归档重建），补发 {a.nudge_min}min，"
            f"STALE 线 {a.stale_min}min"
            + (f"，积分门槛 {REMAIN_MIN}" if USE_POOL else "，积分监控关"))
        fail_until: dict[str, float] = {}   # wid → 该时间前不再尝试派发（失败冷却）
        missing_since: dict[str, float] = {}  # wid → 首次观测到会话失链的时间
        last_hb = 0.0
        last_board_ok = True
        last_quota_check = time.time()
        _credit_streak = 0        # 页面积分提示连续命中次数（两轮≥30s间隔才算成立）
        _last_credit_scan = 0.0   # 页面扫描节流（30s）
        _last_blocked_scan = 0.0  # 阻塞单积分信号扫描节流（60s，列表拉取较重）
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
                    if not conv:
                        # 标记重链（全列表滚动搜索，60s 节流；只读侧栏标题）
                        conv = relink_conv(wid)
                    if not conv:
                        missing_conv += 1
                        # 失链兜底（2026-10-06 Variable 指定：续跑必点进旧
                        # 对话，新建只是最后手段）——READY/空状态且全列表
                        # （含滚动）彻底搜索 15min 仍找不回 → 该对话已不可达，
                        # 作废映射后重建；BUSY 失链绝不新建（工人可能还在
                        # 旧对话里干活，等它写 READY 再兜底）。
                        if state.startswith("BUSY"):
                            continue
                        first = missing_since.setdefault(wid, now)
                        if (now - first >= 900
                                and now >= fail_until.get(wid, 0)):
                            missing_since.pop(wid, None)
                            ledger("失链重建", "-", wid,
                                   "READY/空状态 15min 找不回会话"
                                   "（全列表搜索过）→ 作废映射重建（最后手段）")
                            log(f"[失链] {wid} 会话 15min 找不回 → "
                                f"作废映射重建（最后手段）")
                            write_conv(wid, "")
                            release_worker_tasks(wid)   # 防名下有看不见的已领单
                            fail_until[wid] = time.time() + 120
                            conv2, ev2 = guarded(f"{wid} 失链重建", rebuild_worker,
                                                 wid, default=("", "异常（见塔日志）"))
                            if conv2 or "发送成功" in ev2:
                                ledger("失链重建完成", "-", wid, ev2)
                                log(f"{wid} 失链重建 ✓ {ev2}")
                            else:
                                ledger("失链重建失败", "-", wid, ev2)
                                log(f"[FAIL] {wid} 失链重建失败：{ev2}")
                            time.sleep(2.5)
                        continue

                    missing_since.pop(wid, None)   # 会话已找回，清失链计时
                    if state.startswith("BUSY"):
                        busy_cnt += 1
                        any_busy = True
                        age_min = (now - state_mtime(wid)) / 60.0
                        # 卡死归档重建（Variable 2026-10-06 指定：10min 无动静
                        # → 归档 + 释放名下任务 + 重建同编号工人继续）
                        if (a.dead_min and age_min > a.dead_min
                                and now >= fail_until.get(wid, 0)):
                            arch = guarded(f"{wid} 卡死归档", archive_dead_worker,
                                           wid, conv, state, age_min,
                                           f"BUSY {age_min:.0f}min 无动静",
                                           default=None)
                            if arch is None:
                                fail_until[wid] = time.time() + 60
                                continue   # 归档没成功：下轮重试（状态依旧会超时命中）
                            ledger("卡死归档", "-", wid,
                                   f"{state} {age_min:.0f}min → {arch.name}，重建中")
                            log(f"[卡死] {wid} {age_min:.0f}min 无动静 → "
                                f"归档 {arch.name} → 重建")
                            fail_until[wid] = time.time() + 60
                            conv2, ev2 = guarded(f"{wid} 卡死重建", rebuild_worker,
                                                 wid, default=("", "异常（见塔日志）"))
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
                            ok, why = guarded(f"{wid} 补发检查", dispatch_continue,
                                              wid, conv, nudge_msg(wid),
                                              default=(False, "异常（见塔日志）"))
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
                        ok, why = guarded(f"{wid} 续跑派工", dispatch_continue,
                                          wid, conv, continue_msg(wid),
                                          default=(False, "异常（见塔日志）"))
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
                            ok, why = guarded(f"{wid} 补发协议", dispatch_continue,
                                              wid, conv, render_protocol(wid),
                                              default=(False, "异常（见塔日志）"))
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

                # 3.5) 积分巡检与自动切号（Variable 2026-10-06/07 指定）。
                #      触发信号（积分耗尽时塔的发送不耗积分，round_fail 恒 0，
                #      只靠发送连败会彻底漏掉该场景 → 任务板阻塞单堆积停摆）：
                #      ① 发送连败 ≥3（塔→工人指令发不出去）
                #      ② 页面积分提示连续 ≥2 次（toast/dialog 扫描，30s 节流，
                #        排除聊天区；连续两次防单次误报触发无谓重启）
                #      ③ 阻塞单原因含积分关键词 ≥2 例（工人 AI 失败把任务标阻塞）
                #      ④ 页面提示 1 次 + 阻塞积分单 ≥1 或阻塞堆积 ≥8（交叉印证）
                #      切号成功后自动把积分阻塞单重排回待领（任务继续做下去）。
                quota_due = time.time() - last_quota_check >= QUOTA_CHECK_EVERY
                page_credit = ""
                if time.time() - _last_credit_scan >= 30:
                    _last_credit_scan = time.time()
                    pr = guarded("积分提示扫描", st.call_js, JS_CREDIT_PROMPT,
                                 default=None)
                    if isinstance(pr, dict) and pr.get("hit"):
                        page_credit = str(pr.get("text") or "")[:40]
                        _credit_streak += 1
                        if _credit_streak == 1:
                            log(f"[积分] 页面出现积分提示：{page_credit}")
                    else:
                        _credit_streak = 0
                credit_hits, blocked_all = 0, 0
                if board_ok and blocked:
                    if time.time() - _last_blocked_scan >= 60:
                        _last_blocked_scan = time.time()
                        binfo = guarded("阻塞积分信号", blocked_credit_hits,
                                        default={"hits": 0, "blocked": 0})
                        credit_hits = binfo.get("hits") or 0
                        blocked_all = binfo.get("blocked") or 0
                        if credit_hits:
                            log(f"[积分] 阻塞单积分信号 {credit_hits} 例"
                                f"（{'；'.join(binfo.get('samples') or [])}）")
                if USE_POOL and (round_fail >= 3 or _credit_streak >= 2
                                 or credit_hits >= 2
                                 or (page_credit and credit_hits >= 1)
                                 or (page_credit and blocked_all >= 8)
                                 or quota_due):
                    last_quota_check = time.time()
                    info = guarded("积分巡检", apool.probe_all, REMAIN_MIN,
                                   default={"ok": False, "why": "异常（见塔日志）"})
                    if not info.get("ok"):
                        log(f"[积分] 账号池不可读：{info.get('why')}（下轮再试）")
                    else:
                        log(f"[积分巡检] 池 {info['accounts']} 号 总余 "
                            f"{info['pool_remain']} 最高 {info['richest']}"
                            f"({info['richest_remain']}) 可切 {info['switchable']}"
                            f" / 发送连败 {round_fail} / 页面提示×{_credit_streak}"
                            f" / 阻塞积分单 {credit_hits}")
                        need_switch = (round_fail >= 3 or _credit_streak >= 2
                                       or credit_hits >= 2
                                       or (page_credit and credit_hits >= 1)
                                       or (page_credit and blocked_all >= 8))
                        if need_switch:
                            why_sw = (f"单轮发送失败 {round_fail} 次"
                                      if round_fail >= 3 else
                                      f"页面积分提示×{_credit_streak}"
                                      if _credit_streak >= 2 else
                                      f"阻塞单积分信号 {credit_hits} 例")
                            if info.get("switchable", 0) <= 0:
                                log("★ 停机 ★ 积分耗尽信号成立但账号池无可用"
                                    "积分账号——产线终止（需人工补充账号）")
                                ledger("停机", "-", "领单塔",
                                       f"{why_sw} + 无可用积分账号"
                                       f"（门槛 {REMAIN_MIN}）：{info}")
                                return 2
                            r = guarded("切号流程", switch_account_flow,
                                        why_sw, a.workers,
                                        settle_min=a.settle_min,
                                        default="fail:异常（见塔日志，下轮重试）")
                            if r == "no_account":
                                log("★ 停机 ★ 无可用积分账号——产线终止")
                                return 2
                            if r == "ok":
                                _credit_streak = 0   # 新号就绪：页面信号清零重计
                                log("[切号] 工人已重建/保全续用，产线继续")
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
