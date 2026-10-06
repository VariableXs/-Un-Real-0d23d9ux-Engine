"""引导三件套真发自检（填入不发送：零配额、不建会话）。

验证链条（对应 2026-10-06 十四轮探测定案）：
  1. 顶部新建 → 空白视图（安全闸：编辑器空 + 视图已离开原会话）
  2. vb.run()：选工作空间（chip 验证）→ 技能「/」面板挂载（mention 计数验证）
     → 拖拽挂 4 文件（resource_link 验证）
  3. fill_cdp 填入测试提示词（仅含 / 面板没挂上的技能兜底内联，若有）
  4. 终态四验：chip 文本 / 4 文件名全在 / 技能 mention 显示名全在 / 真实字数
  5. 清场：点顶部新建丢弃草稿（从未发送 → 不落库、不耗配额）
  6. 还原原视图（带重试）

用法：python tools/bootstrap_selftest.py
退出码：0=全绿；2=编辑器有内容拒绝；3=进不去空白；4=验证失败
"""

import importlib.util
import json
import sys
import time
from pathlib import Path

_HERE = Path(__file__).parent
_spec = importlib.util.spec_from_file_location("st", str(_HERE / "send_selftest.py"))
st = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(st)
_bspec = importlib.util.spec_from_file_location("vb", str(_HERE / "bootstrap.py"))
vb = importlib.util.module_from_spec(_bspec)
_bspec.loader.exec_module(vb)

TEST_PROMPT = "【VARIX-W99·引导自检】本条不发送，仅验证编辑器组合态。"

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

JS_CLICK_NEW = r"""(() => {
  const b = Array.from(document.querySelectorAll('button')).find(
    x => (x.innerText || '').trim() === '新建任务' && x.offsetWidth > 0);
  if (!b) return { ok: false, why: 'no button' };
  const r = b.getBoundingClientRect();
  const o = { bubbles: true, cancelable: true, view: window,
              clientX: r.left + 8, clientY: r.top + 8, button: 0, detail: 1 };
  for (const t of ['mouseover','mousemove','mousedown','mouseup','click'])
    b.dispatchEvent(new MouseEvent(t, o));
  return { ok: true };
})()"""

JS_WS_CHIP = r"""(() => {
  const c = document.querySelector('button.cr-workspace-picker');
  return c ? { found: true, text: (c.innerText || '').trim() } : { found: false };
})()"""

JS_EDITOR_FILES = r"""((names) => {
  const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!e) return { ok: false, why: 'no editor' };
  const text = (e.innerText || '');
  const html = e.innerHTML || '';
  const missing = names.filter(n => !text.includes(n));
  return { ok: missing.length === 0, missing,
           links: (html.match(/resource_link/g) || []).length,
           mentions: (html.match(/mention/g) || []).length,
           text: text.trim().slice(0, 500) };
})"""


def editor_files_check(names):
    """终态验：连续采样 3 次（页面 re-render 窗口期可能瞬断，2026-10-06 实测）。"""
    v = None
    for _ in range(3):
        v = st.call_js(JS_EDITOR_FILES + "(" + json.dumps(names, ensure_ascii=False) + ")")
        if isinstance(v, dict) and v.get("ok") and "text" in v:
            return v
        time.sleep(1.0)
    return v or {}


def wait_blank(timeout=10):
    for _ in range(int(timeout / 0.7)):
        time.sleep(0.7)
        if not st.call_js(JS_ACTIVE_CONV) and not st.call_js(
                "!!document.querySelector('.cr-message-list')"):
            return True
    return False


def restore_origin(origin: str) -> bool:
    if not origin:
        return True   # 原视图是空白/主页，停在空白即可
    ids = st.call_js(
        r"""(() => Array.from(document.querySelectorAll(
             'div.conversation-item[data-conversation-id]'))
             .map(e => e.getAttribute('data-conversation-id')))()""")
    full = next((x for x in (ids or []) if x.startswith(origin[:14])), "")
    if not full:
        return False
    js = (r"""((cid)=>{const el=document.querySelector('div.conversation-item[data-conversation-id="'+cid+'"]');
          if(!el) return {found:false}; const node=el.querySelector('[class*="_card_"]')||el.firstElementChild||el;
          const r=node.getBoundingClientRect(); const o={bubbles:true,cancelable:true,view:window,clientX:r.left+8,clientY:r.top+8,button:0,detail:1};
          for (const tp of ['mouseover','mousemove','mousedown','mouseup','click'])
            node.dispatchEvent(new MouseEvent(tp,o));
          return {found:true};})""" + "(" + json.dumps(full) + ")")
    for _ in range(4):
        st.call_js(js)
        time.sleep(1.8)
        if st.call_js(JS_ACTIVE_CONV) == full:
            return True
    return False


def main() -> int:
    if not st.devtools_alive():
        print("[ERR] 9222 不通。先跑 开端口.bat")
        return 1
    origin = st.call_js(JS_ACTIVE_CONV) or ""
    chars = st.call_js(
        r"""(() => { const e=document.querySelector('div[data-slate-editor="true"][contenteditable="true"]'); if(!e) return -1;
             const t=(e.innerText||'').trim(); if(/今天帮你做些什么|有什么我可以帮|How can I help/i.test(t)) return 0;
             let s=0; e.querySelectorAll('[data-slate-string]').forEach(x=>s+=(x.textContent||'').length); return s||t.length; })()""")
    print(f"S0 原视图={origin[:16] or '(空白)'} 编辑器字数={chars}")
    if isinstance(chars, int) and chars > 0:
        # 自家 W99 残留 → 自愈清空后继续；他人内容 → 拒绝
        full_text = st.call_js(
            r"""(() => { const e=document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
                 return e ? (e.innerText||'') : ''; })()""") or ""
        if "VARIX-W99" in full_text:
            print("[自愈] 检测到自家 W99 残留草稿 → 清空后继续")
            if not st.clear_editor_cdp():
                print("[ERR] 残留清空失败")
                return 2
        else:
            print("[拒绝] 编辑器有他人内容，不动")
            return 2

    ok_all = True
    try:
        print("── 1/5 顶部新建 → 空白 ──")
        st.call_js(JS_CLICK_NEW)
        if not wait_blank():
            print("[FAIL] 未进入空白视图")
            return 3
        time.sleep(1.0)

        print("── 2/5 引导：工作空间 → 技能 / 面板挂载 → 4 文件 ──")
        ok_b, ev = vb.run(st)
        print(f"  {json.dumps(ev, ensure_ascii=False)}")
        ok_all &= ok_b
        fallback = ev.get("skills_fallback", []) if ok_b else list(vb.SKILLS_DEFAULT)
        mounted = ev.get("skills_ui", {}).get("mounted", []) if ok_b else []

        print("── 3/5 填入测试提示词（不发送） ──")
        prompt = TEST_PROMPT
        if fallback:
            prompt += "必载技能：" + vb.skills_inline(fallback) + "。"
        ok_f, ev_f = st.fill_cdp(prompt)
        print(f"  fill: {ev_f}")
        ok_all &= ok_f

        print("── 4/5 终态四验 ──")
        chip = st.call_js(JS_WS_CHIP) or {}
        ws_ok = vb.WS_DEFAULT in (chip.get("text") or "")
        print(f"  工作空间 chip: {chip.get('text')!r} → {'✓' if ws_ok else '✗'}")
        names = [f.replace("\\", "/").rsplit("/", 1)[-1] for f in vb.FILES_DEFAULT]
        v = editor_files_check(names)
        files_ok = bool(isinstance(v, dict) and v.get("ok"))
        print(f"  4 文件在编辑器: {'✓' if files_ok else '✗ missing=' + str(v.get('missing'))}"
              f"（resource_link×{v.get('links')}）")
        txt = v.get("text", "")
        ui_ok = all(vb.SLASH_SKILL_DISPLAY.get(s, s) in txt for s in mounted)
        fb_ok = all(f"@skill:{s}" in txt for s in fallback)
        print(f"  技能 / 面板挂载 {len(mounted)}/{len(vb.SKILLS_DEFAULT)}: "
              f"{'✓' if ui_ok else '✗'}（mention 节点×{v.get('mentions')}）")
        if fallback:
            print(f"  兜底内联 @skill: {','.join(fallback)} → {'✓' if fb_ok else '✗'}")
        sk_ok = ui_ok and fb_ok
        s = st.probe_state() or {}
        print(f"  编辑器真实字数: {s.get('charsReal')}（含提示词文本）")
        ok_all &= ws_ok and files_ok and sk_ok

        print("── 5/5 清场（丢弃草稿，从未发送）+ 还原 ──")
        st.call_js(JS_CLICK_NEW)
        wait_blank()
        time.sleep(1.0)
        back = restore_origin(origin)
        print(f"  还原: {back}")
        ok_all &= back
    except Exception as e:
        print(f"[EXC] {e}")
        ok_all = False
        try:
            st.call_js(JS_CLICK_NEW)
            time.sleep(1.2)
            restore_origin(origin)
        except Exception:
            pass

    print("\n★ 自检 " + ("全绿" if ok_all else "存在失败项") + " ★")
    return 0 if ok_all else 4


if __name__ == "__main__":
    sys.exit(main())
