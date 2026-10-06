"""会话引导三件套（工作空间 / 文件附件 / 技能提及）· 产线模块。

2026-10-06 十一轮真机探测定案（探针归档 _attic/2026-10-06-bootstrap/）：

① 工作空间
  - 空白视图 composer 底栏 button.cr-workspace-picker 是选择入口；chip 文本
    「选择工作空间」= 未选（默认值不稳定，一会儿带上次空间一会儿没有），
    塔必须显式选择并用 chip 文本验证。
  - 弹层 .cr-workspace-picker-popover__item 是长列表（时间戳目录在前，
    命名空间在底部可视区外）→ 必须先 scrollIntoView 再读坐标。
  - Radix popover 不吃合成 MouseEvent → 一律 CDP Input.dispatchMouseEvent
    （trusted 指针管线）。

② 文件附件
  - 隐藏 input[type=file] 是条件挂载（空白视图 querySelector = null），
    DOM.setFileInputFiles 路线作废。
  - ★ 正解 = CDP Input.dispatchDragEvent（dragEnter→dragOver→drop，
    DragData{items:[], files:[...]}）→ 编辑器插入真正的 resource_link
    上下文节点（file:/// URI），一次 drop 挂全部文件，实测 2/2 出现。

③ 技能
  - 「+」菜单的技能子面板（cr-skills-submenu）点击/悬停/键盘全是竞态，弃用。
  - ★ 正解（Variable 指路 + probe12~14 实测）= 「/」面板：编辑器插入 / 弹出
    技能(161)+指令(2) 面板，条目按**中文显示名**展示（无 slug 属性），
    过滤词用显示名前缀（slug 搜不到），点击前 scrollIntoView，点选后编辑器
    插入真 mention 节点（mentions 计数 +1 即验证，查询文本被消费无残留）；
    已有 mention/文件的编辑器上可连续触发（probe14 clean=False 实测）。
  - 兜底：/ 面板挂载失败的技能，由塔把 @skill:xxx 内联进首条提示词
    （前端输入时自动转成技能 mention 节点，2026-10-06 实测）。

安全约定：
  - 所有动作在「已确认的空白会话」上执行；失败可重试一次，仍失败显性报错，
    由塔点「新建任务」重置后走下一位（绝不半引导发送）。
  - 不碰用户既有会话；不点「添加文件」（会弹原生对话框）。
"""

import json
import time

# ── 默认引导载荷（Variable 2026-10-06 指定）──
WS_DEFAULT = "-Un-Real-0d23d9ux-Engine-main"
FILES_DEFAULT = [
    r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixTaskOps\VTaskBoard\dist\VTaskBoard.exe",
    r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\VE-STAR-II\CGPU Varix STAR II · 总纲与施工书.md",
    r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\VE-STAR-II\VE Varix STAR II · 总纲与施工书.md",
    r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\CoRun Varix STAR II · Unxreal.md",
]
SKILLS_DEFAULT = [
    "rust-code-audit",
    "rust-desktop-app-cn",
    "rust-raspberrypi-os",
    "rust-best-practices",
    "karpathy-coding-constraints",
]

# 技能 slug → 「/」面板显示名（probe13/14 实测：面板条目按显示名展示，
# option 上没有 slug/data-id 属性，机器匹配只能靠显示名文本）。
SLASH_SKILL_DISPLAY = {
    "rust-code-audit": "Rust代码审计",
    "rust-desktop-app-cn": "Rust桌面应用助手",
    "rust-raspberrypi-os": "Rust 裸机操作系统开发教练",
    "rust-best-practices": "Rust 编程最佳实践",
    "karpathy-coding-constraints": "Karpathy 编码四原则",
}

JS_WS_CHIP = r"""(() => {
  const c = document.querySelector('button.cr-workspace-picker');
  return c ? { found: true, text: (c.innerText || '').trim() } : { found: false };
})()"""

JS_RECT_OF = r"""((sel, name) => {
  const vis = (e) => { const r = e.getBoundingClientRect(); return r.width > 4 && r.height > 4; };
  for (const el of document.querySelectorAll(sel)) {
    if (!vis(el)) continue;
    if (name && !(el.innerText || '').includes(name)) continue;
    el.scrollIntoView({ block: 'center' });
    const r = el.getBoundingClientRect();
    return { ok: true, x: r.left + r.width / 2, y: r.top + r.height / 2,
             text: (el.innerText || '').trim().slice(0, 60) };
  }
  return { ok: false, why: (name || '') + ' not in ' + sel };
})"""

JS_EDITOR_CENTER = r"""(() => {
  const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!e) return { ok: false, why: 'no editor' };
  const r = e.getBoundingClientRect();
  return { ok: true, x: r.left + r.width / 2, y: r.top + r.height / 2 };
})()"""

JS_EDITOR_FILES = r"""((names) => {
  const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!e) return { ok: false, why: 'no editor' };
  const text = (e.innerText || '');
  const html = e.innerHTML || '';
  const missing = names.filter(n => !text.includes(n));
  return { ok: missing.length === 0, missing,
           links: (html.match(/resource_link/g) || []).length,
           text: text.trim().slice(0, 300) };
})"""


def trust_click(st, x, y, tag=""):
    """可信鼠标单击（mouseMoved → mousePressed → mouseReleased）。"""
    for m, p in (("mouseMoved", {}),
                 ("mousePressed", {"button": "left", "clickCount": 1}),
                 ("mouseReleased", {"button": "left", "clickCount": 1})):
        params = {"type": m, "x": int(x), "y": int(y)}
        params.update(p)
        st.call_cdp_method("Input.dispatchMouseEvent", params)
        time.sleep(0.06)


def rect_of(st, sel, name=""):
    """scrollIntoView 后取元素中心（长列表项必须先滚进可视区）。"""
    return st.call_js(JS_RECT_OF + "(" + json.dumps(sel) + "," + json.dumps(name) + ")")


def trust_click_sel(st, sel, name=""):
    rc = rect_of(st, sel, name)
    if isinstance(rc, dict) and rc.get("ok"):
        trust_click(st, rc["x"], rc["y"])
    return rc


def select_workspace(st, ws: str = WS_DEFAULT, retries: int = 2) -> tuple:
    """显式选择工作空间并用 chip 文本验证。返回 (ok, 证据)。"""
    for attempt in range(1, retries + 1):
        chip = st.call_js(JS_WS_CHIP) or {}
        if ws in (chip.get("text") or ""):
            return True, f"chip 已是 {ws}（第{attempt}次验证，免点）"
        rc = trust_click_sel(st, 'button.cr-workspace-picker')
        if not (isinstance(rc, dict) and rc.get("ok")):
            time.sleep(1.0)
            continue
        time.sleep(1.2)
        item = rect_of(st, '.cr-workspace-picker-popover__item', ws)
        if not (isinstance(item, dict) and item.get("ok")):
            time.sleep(1.0)
            continue
        time.sleep(0.4)          # 等滚动稳定
        item = rect_of(st, '.cr-workspace-picker-popover__item', ws)
        if not (isinstance(item, dict) and item.get("ok")):
            continue
        trust_click(st, item["x"], item["y"])
        time.sleep(1.3)
        chip = st.call_js(JS_WS_CHIP) or {}
        if ws in (chip.get("text") or ""):
            return True, f"chip 已切到 {ws}（第{attempt}次尝试）"
    chip = st.call_js(JS_WS_CHIP) or {}
    return False, f"工作空间选择失败：chip={chip.get('text')!r}"


def attach_files(st, files=None, retries: int = 2) -> tuple:
    """拖拽注入文件 → 验证编辑器 resource_link 节点。返回 (ok, 证据)。"""
    files = [str(f).replace("\\", "/") for f in (files or FILES_DEFAULT)]
    names = [f.rsplit("/", 1)[-1] for f in files]
    pt = st.call_js(JS_EDITOR_CENTER)
    if not (isinstance(pt, dict) and pt.get("ok")):
        return False, f"文件挂载失败：{pt.get('why') if isinstance(pt, dict) else pt}"
    x, y = int(pt["x"]), int(pt["y"])
    last = ""
    for attempt in range(1, retries + 1):
        data = {"items": [], "files": files, "dragOperationsMask": 1}
        try:
            for t in ("dragEnter", "dragOver", "drop"):
                st.call_cdp_method("Input.dispatchDragEvent",
                                   {"type": t, "x": x, "y": y, "data": data})
                time.sleep(0.25)
        except Exception as e:
            last = f"dispatchDragEvent 异常：{e}"
            time.sleep(1.0)
            continue
        time.sleep(2.2)
        v = st.call_js(JS_EDITOR_FILES + "(" + json.dumps(names, ensure_ascii=False) + ")")
        if isinstance(v, dict) and v.get("ok"):
            return True, (f"拖拽挂载 {len(names)} 文件全验证（resource_link×{v.get('links')}，"
                          f"第{attempt}次尝试）")
        last = f"缺失 {v.get('missing') if isinstance(v, dict) else v}"
    return False, f"文件挂载失败：{last}"


def skills_inline(skills=None) -> str:
    """技能清单 → 首条提示词内的 @skill: 提及文本（/ 面板挂载失败时的兜底）。"""
    return " ".join(f"@skill:{s}" for s in (skills or SKILLS_DEFAULT))


# ── 「/」面板技能挂载（probe12~14 实测路径）──

# 根面板判定必须用 class token 完全匹配——[class*="cr-trigger-search-panel"]
# 会命中 __group-title-count 等子元素（probe12 的误配教训）。
JS_SLASH_PANEL = r"""(() => {
  const vis = (e) => { const r = e.getBoundingClientRect(); return r.width > 4 && r.height > 4; };
  const roots = Array.from(document.querySelectorAll('[class*="cr-trigger-search-panel"]'))
    .filter(e => String(e.className).split(/\s+/).includes('cr-trigger-search-panel'))
    .filter(vis);
  if (!roots.length) return { open: false };
  const opts = Array.from(document.querySelectorAll('[role="option"]')).filter(vis);
  return { open: true, n: opts.length };
})()"""

# 按显示名点选（startsWith 优先、includes 放宽）；长列表必须先 scrollIntoView
# （probe5 教训：可视区外的项坐标点击落空）。
JS_SKILL_CLICK = r"""((name) => {
  const vis = (e) => { const r = e.getBoundingClientRect(); return r.width > 4 && r.height > 4; };
  const items = Array.from(document.querySelectorAll('[role="option"]')).filter(vis);
  let hit = null;
  for (const it of items) {
    const t = (it.innerText || '').trim();
    if (t.startsWith(name) || t.includes(name)) { hit = it; break; }
  }
  if (!hit) return { ok: false, why: 'no option named ' + name };
  hit.scrollIntoView({ block: 'center' });
  const r = hit.getBoundingClientRect();
  if (r.width < 4 || r.height < 4) return { ok: false, why: 'invisible after scroll' };
  const o = { bubbles: true, cancelable: true, view: window,
              clientX: r.left + 12, clientY: r.top + Math.min(14, r.height / 2), button: 0, detail: 1 };
  for (const tp of ['mouseover','mousemove','mousedown','mouseup','click'])
    hit.dispatchEvent(new MouseEvent(tp, o));
  return { ok: true, text: (hit.innerText || '').trim().replace(/\s+/g, ' ').slice(0, 60) };
})"""

# 编辑器 mention 计数（挂载验证：点击后 mentions 必须 +1）
JS_SKILL_STATE = r"""(() => {
  const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!e) return { gone: true };
  return { gone: false,
           mentions: (e.innerHTML.match(/mention/g) || []).length,
           text: (e.innerText || '') };
})()"""


def insert_raw(st, text: str) -> bool:
    """裸 focus + Input.insertText（trusted 输入）。

    不能用 st.fill_cdp：它有 charsReal>=10 的门槛，而 / 和过滤词只有几个
    字符，会被误判为失败。
    """
    if not st.call_js(st.JS_FOCUS_EDIT):
        return False
    try:
        st.call_cdp_method("Input.insertText", {"text": text})
    except Exception:
        return False
    time.sleep(0.35)
    return True


def _wait_slash_panel(st, timeout: float, want_items: bool) -> dict:
    """轮询 / 面板；want_items=True 时等到选项数 >0（搜索是异步的，~2-5s）。"""
    deadline = time.time() + timeout
    last = {"open": False}
    while time.time() < deadline:
        time.sleep(0.5)
        v = st.call_js(JS_SLASH_PANEL)
        if isinstance(v, dict) and v.get("open"):
            last = v
            if (not want_items) or v.get("n", 0) > 0:
                return v
        elif isinstance(v, dict):
            last = v
    return last


def _mention_count(st) -> int:
    v = st.call_js(JS_SKILL_STATE)
    return v.get("mentions", -1) if isinstance(v, dict) else -1


def _close_slash_panel(st):
    """trusted Escape 关掉残留面板（合成 keydown 关不掉 Radix，但 trusted 可以）。"""
    try:
        for _ in range(2):
            st.call_cdp_method("Input.dispatchKeyEvent",
                               {"type": "rawKeyDown", "key": "Escape", "code": "Escape",
                                "windowsVirtualKeyCode": 27})
            st.call_cdp_method("Input.dispatchKeyEvent",
                               {"type": "keyUp", "key": "Escape", "code": "Escape",
                                "windowsVirtualKeyCode": 27})
            time.sleep(0.3)
    except Exception:
        pass


def _backspace_residue(st, filt: str):
    """失败重试路径上编辑器可能残留 /filt 查询文本 → trusted Backspace 清掉。"""
    for _ in range(3):
        v = st.call_js(JS_SKILL_STATE)
        if not (isinstance(v, dict) and not v.get("gone")):
            return
        if ("/" + filt) not in v.get("text", ""):
            return
        if not st.call_js(st.JS_FOCUS_EDIT):
            return
        try:
            for _i in range(len(filt) + 2):
                st.call_cdp_method("Input.dispatchKeyEvent", {
                    "type": "rawKeyDown", "key": "Backspace", "code": "Backspace",
                    "windowsVirtualKeyCode": 8})
                st.call_cdp_method("Input.dispatchKeyEvent", {
                    "type": "keyUp", "key": "Backspace", "code": "Backspace",
                    "windowsVirtualKeyCode": 8})
                time.sleep(0.05)
        except Exception:
            return
        time.sleep(0.4)


def add_skill_via_slash(st, slug: str, retries: int = 2) -> tuple:
    """经「/」面板按显示名点选技能 → 编辑器插入真 mention 节点。

    返回 (ok, 证据)。无显示名映射的 slug 直接 False（调用方走内联兜底）。
    """
    display = SLASH_SKILL_DISPLAY.get(slug)
    if not display:
        return False, {"slug": slug, "why": "无 / 面板显示名映射"}
    filt = display[:4]
    last = {}
    for attempt in range(1, retries + 1):
        _close_slash_panel(st)
        _backspace_residue(st, filt)
        before = _mention_count(st)
        if not insert_raw(st, "/"):
            last = {"why": "插入 / 失败"}
            continue
        p = _wait_slash_panel(st, 8, want_items=False)
        if not p.get("open"):
            last = {"why": "/ 面板未开"}
            continue
        if not insert_raw(st, filt):
            last = {"why": "插入过滤词失败"}
            continue
        p2 = _wait_slash_panel(st, 12, want_items=True)
        if not p2.get("n"):
            last = {"why": f"过滤[{filt}]无结果", "panel_n": p2.get("n")}
            continue
        click = st.call_js(JS_SKILL_CLICK + "(" + json.dumps(display, ensure_ascii=False) + ")")
        time.sleep(1.6)
        v = st.call_js(JS_SKILL_STATE)
        ok = (isinstance(v, dict) and not v.get("gone")
              and display in v.get("text", "")
              and isinstance(v.get("mentions"), int) and v["mentions"] > before
              and ("/" + filt) not in v.get("text", ""))
        if ok:
            return True, {"slug": slug, "display": display, "attempt": attempt,
                          "mentions": v.get("mentions")}
        last = {"why": "点击后未见新 mention", "click":
                (click or {}).get("text", "")[:40] if isinstance(click, dict) else click,
                "mentions": v.get("mentions") if isinstance(v, dict) else None}
    _close_slash_panel(st)
    _backspace_residue(st, filt)
    return False, {"slug": slug, "display": display, **last}


def mount_skills(st, skills=None) -> tuple:
    """逐个经 / 面板挂载技能（连续挂载：编辑器已有 mention 仍可触发，实测）。"""
    mounted, failed, ev = [], [], []
    for s in (skills or SKILLS_DEFAULT):
        ok, e = add_skill_via_slash(st, s)
        ev.append(e)
        (mounted if ok else failed).append(s)
        time.sleep(0.6)
    return mounted, failed, ev


def run(st, ws: str = WS_DEFAULT, files=None, need_files: bool = True,
        skills=None) -> tuple:
    """在已确认的空白会话上执行引导三件套。

    顺序：工作空间 → 技能（/ 面板 UI 挂载，编辑器最干净时做最娇气的交互）
    → 文件（拖拽注入）。
    skills=None 用 SKILLS_DEFAULT；空 list 跳过技能挂载。
    / 面板挂载失败的技能记入 ev["skills_fallback"]，由塔把它们以
    @skill: 内联渲染进首条提示词（前端输入时自动转 mention，实测）。
    返回 (ok, 证据dict)。工作空间或文件失败即整体失败（塔负责重置重试）。
    """
    ev = {}
    ok_ws, ev_ws = select_workspace(st, ws)
    ev["workspace"] = ev_ws
    if not ok_ws:
        return False, ev
    skill_list = SKILLS_DEFAULT if skills is None else list(skills)
    if skill_list:
        mounted, failed, ev_sk = mount_skills(st, skill_list)
        ev["skills_ui"] = {"mounted": mounted, "failed": failed}
        ev["skills_detail"] = ev_sk
    else:
        mounted, failed = [], []
    ev["skills_fallback"] = list(failed)
    if need_files:
        ok_f, ev_f = attach_files(st, files)
        ev["files"] = ev_f
        if not ok_f:
            return False, ev
    return True, ev


def cleanup_reset(st, click_new_js: str):
    """引导失败后的现场还原：点「新建任务」回干净空白（弹层随之销毁）。"""
    try:
        st.call_js(click_new_js)
        time.sleep(1.5)
    except Exception:
        pass
