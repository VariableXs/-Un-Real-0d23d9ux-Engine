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
  - ★ 正解 = 首条提示词内联 @skill:xxx 文本。铁证：本调度会话收到的用户
    消息就是纯文本 @skill:清单，技能全部真实挂载（后端按消息文本解析）。
  - 双保险：提示词同时要求工人用 Skill 工具按名加载。

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
    """技能清单 → 首条提示词内的 @skill: 提及文本。"""
    return " ".join(f"@skill:{s}" for s in (skills or SKILLS_DEFAULT))


def run(st, ws: str = WS_DEFAULT, files=None, need_files: bool = True) -> tuple:
    """在已确认的空白会话上执行引导（工作空间 + 文件）。

    技能不在此处 UI 挂载——走首条提示词内联 @skill: 文本（skills_inline）。
    返回 (ok, 证据dict)。任一步失败即整体失败（塔负责重置重试）。
    """
    ev = {}
    ok_ws, ev_ws = select_workspace(st, ws)
    ev["workspace"] = ev_ws
    if not ok_ws:
        return False, ev
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
