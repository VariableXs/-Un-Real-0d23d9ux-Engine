"""调度塔发车前 DOM 体检（只读，零写入，不动 UI）。

检查项与退出码：
  1. CDP 端点可达                      —— 不通 = exit 1
  2. CDP target 列表（主窗口识别）      —— 找不到 page = exit 2
  3. 侧栏会话项 conversation-item       —— 0 个 = exit 3（提示先进入任一任务页）
  4. 新建任务入口                       —— 找不到 = WARN（发车时手动停首页）
  5. 输入框 / 发送键 / 模型选择器        —— 缺编辑器 = exit 4

用法：
  python tools/check_tower_dom.py            # 全量体检
  python tools/check_tower_dom.py --json     # 机读输出
"""

import importlib.util
import json
import sys
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "st", str(Path(__file__).parent / "send_selftest.py")
)
st = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(st)

# ── 探测 JS：ASCII 键名；raw string 保住 \n 不被 Python 吃掉；不含注释 ──
JS_TARGETS = None  # /json/list 走 HTTP，不走 eval

JS_PROBE = r"""(() => {
  const q = (sel) => document.querySelectorAll(sel).length;
  const convs = Array.from(document.querySelectorAll('div.conversation-item'))
    .slice(0, 30)
    .map(e => ({
      id: e.getAttribute('data-conversation-id') || '',
      title: ((e.innerText || '').trim().split('\n')[0] || '').slice(0, 26),
      cls: String(e.className).slice(0, 120),
    }));
  const newBtns = Array.from(document.querySelectorAll('button,[role="button"],a,div'))
    .filter(e => {
      const t = (e.innerText || '').trim();
      return (t === '新建任务' || t === '新建对话' || t === '新建') && e.offsetWidth > 0;
    })
    .slice(0, 5)
    .map(e => ({ tag: e.tagName, cls: String(e.className).slice(0, 100) }));
  return {
    convCount: q('div.conversation-item'),
    convs: convs,
    newBtns: newBtns,
    modelTriggers: q('button.cr-model-selector__trigger'),
    editorCount: q('div[data-slate-editor="true"][contenteditable="true"]'),
    sendBtnCount: q('button.cr-send-button'),
    stopBtnCount: q('button[class*="stop"],button[aria-label*="停止"],[title*="停止"]'),
    url: String(location.href).slice(0, 80),
  };
})()"""


def list_targets():
    import urllib.request

    with urllib.request.urlopen(f"{st.URL}/json/list", timeout=5) as r:
        return json.loads(r.read().decode("utf-8"))


def main() -> int:
    args = set(sys.argv[1:])
    report = {}

    # 1. 端点
    alive = st.devtools_alive()
    report["cdp_alive"] = alive
    if not alive:
        print("[FAIL] 9222 不通——WorkBuddy 没带调试端口启动。跑：开端口.bat")
        return 1
    print("[OK] CDP 9222 可达")

    # 2. targets
    try:
        targets = list_targets()
    except Exception as e:
        print(f"[FAIL] /json/list 失败：{e}")
        return 2
    pages = [t for t in targets if t.get("type") == "page"]
    report["targets"] = [
        {"url": t.get("url", "")[:80], "title": t.get("title", "")[:40]} for t in pages
    ]
    print(f"[OK] page target 数: {len(pages)}")
    for i, t in enumerate(pages):
        mark = " <= pages[0]（send_selftest 用这个）" if i == 0 else ""
        print(f"     [{i}] {t.get('url', '')[:70]}  {t.get('title', '')[:30]}{mark}")
    if not pages:
        print("[FAIL] 没有 page target")
        return 2

    # 3~5. DOM 探测
    v = st.call_js(JS_PROBE)
    if v is None:
        print("[FAIL] 页面 JS 探测返回 None（语法/执行错误或 target 非主窗口）")
        print("       对照上面 target 列表；若 pages[0] 不是 WorkBuddy 主窗口，")
        print("       关掉多余 Electron 窗口后重试。")
        return 4
    report["dom"] = v

    print(f"\n── DOM 探测（{v.get('url', '')}）")
    print(f"    会话项 conversation-item : {v.get('convCount')}")
    for c in v.get("convs", [])[:10]:
        print(f"      · id={c['id'][:12]}  title={c['title']!r}")
        print(f"        cls={c['cls']}")
    print(f"    新建任务入口候选        : {len(v.get('newBtns', []))}")
    for b in v.get("newBtns", []):
        print(f"      · {b['tag']}  cls={b['cls']}")
    print(f"    模型选择器              : {v.get('modelTriggers')}")
    print(f"    输入框(Slate)           : {v.get('editorCount')}")
    print(f"    发送键                  : {v.get('sendBtnCount')}")
    print(f"    停止键(忙时出现)        : {v.get('stopBtnCount')}")

    rc = 0
    if not v.get("convCount"):
        print("\n[WARN] 侧栏没有 conversation-item——当前可能停在首页。")
        print("       发车前请点开任意一个任务（左侧任务列表），让侧栏会话列表渲染出来。")
        rc = rc or 3
    if not v.get("editorCount"):
        print("\n[FAIL] 找不到输入框——当前页面不是对话页。")
        rc = 4
    if not v.get("newBtns"):
        print("\n[WARN] 本页没找到「新建任务」入口——发车时调度塔会先尝试，")
        print("       失败则提示你手动点一次侧栏「新建任务」。")
    if not args or "--json" not in args:
        pass
    else:
        print("\n" + json.dumps(report, ensure_ascii=False, indent=2))
    print(f"\n体检结论: {'PASS（可发车）' if rc == 0 else f'FAIL code={rc}'}")
    return rc


if __name__ == "__main__":
    sys.exit(main())
