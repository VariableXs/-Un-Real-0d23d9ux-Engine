"""空闲判据可靠性实测（Python 通道，避免 Rust raw-string 转义地狱）。

★ 为什么用 Python 通道 ★
本轮在 Rust example 里写含 `//` 注释与 `[class*=x]` 选择器的 JS，
经r#"..."# 传递后 eval 返回 `{}`（语法错，静默）。
排查后确认 **CDP / Rust / 中文键名 / 嵌套全都没问题**，
纯粹是探针自身的转义问题。
⇒ **不要在 raw string 里放 JS 注释**。本文件用普通字符串，Python 侧无此问题。

★ 为什么必须实测判据 ★
循环发布的地基就是「检测对方生成完毕」。
判据不可靠 ⇒ 循环会在对方忙碌时误发 ⇒ 打断正在跑的活。
所以穷举候选判据，看哪些真的可用。

用法：python idle_probe.py [采样次数] [间隔秒]
"""

import importlib.util
import json
import sys
import time
from pathlib import Path

# 复用 send_selftest 的 CDP 客户端（不重复实现 WebSocket）
_spec = importlib.util.spec_from_file_location(
    "st", str(Path(__file__).parent / "send_selftest.py")
)
st = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(st)

JS = r"""(() => {
  const btn = document.querySelector('button.cr-send-button');
  const cls = btn ? String(btn.className) : '';
  const ed = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  const realChars = (() => {
    if (!ed) return -1;
    const c = ed.cloneNode(true);
    c.querySelectorAll('[data-slate-placeholder]').forEach(n => n.remove());
    return (c.textContent || '').trim().length;
  })();

  // ── 候选判据，逐个独立取值，方便看谁在变 ──
  const out = {
    btnClass: cls,
    '① 发送键无sending/stop': btn ? !/--sending|--stop/.test(cls) : null,
    '② 发送键disabled': btn ? !!btn.disabled : null,
    '③ 独立停止按钮数': document.querySelectorAll(
      'button[class*="stop"],button[aria-label*="停止"],[title*="停止"]'
    ).length,
    '④ 生成动画元素数': document.querySelectorAll(
      '[class*="streaming"],[class*="generating"],[class*="typing"],[class*="loading-"]'
    ).length,
    '⑤ 助手消息数': document.querySelectorAll(
      '[data-message-author-role="assistant"],.cr-assistant-message,[class*="assistant-message"]'
    ).length,
    '⑥ 用户消息数': document.querySelectorAll(
      '[data-message-author-role="user"],.cr-user-message,[class*="user-message"]'
    ).length,
    '⑦ 最后一个助手消息字数': (() => {
      const ms = document.querySelectorAll(
        '[data-message-author-role="assistant"],.cr-assistant-message,[class*="assistant-message"]'
      );
      if (!ms.length) return -1;
      return (ms[ms.length - 1].innerText || '').trim().length;
    })(),
    '⑧ 编辑器真字数': realChars,
    '⑨ 发送键父元素class': btn && btn.parentElement ? String(btn.parentElement.className).slice(0, 60) : '',
  };
  return out;
})()"""


def main() -> int:
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 8
    gap = float(sys.argv[2]) if len(sys.argv) > 2 else 1.5

    if not st.devtools_alive():
        print("[ERR] 9222 端口不通")
        return 1

    print(f"采样 {n} 次，间隔 {gap}s")
    print("（看哪些键在变——不变的就是不可用的判据）\n")

    prev = None
    for i in range(1, n + 1):
        v = st.call_js(JS)
        if v is None:
            print(f"第{i}次: eval 返回 None")
        else:
            changed = []
            if prev:
                for k, val in v.items():
                    if k != "btnClass" and prev.get(k) != val:
                        changed.append(k)
            tag = "  变化: " + ", ".join(changed) if changed else "  （无变化）"
            print(f"第{i}次  发送键={v.get('btnClass', '')[-30:]}")
            print(f"①空闲={v.get('① 发送键无sending/stop')}  "
                  f"③停止按钮={v.get('③ 独立停止按钮数')}  "
                  f"④生成动画={v.get('④ 生成动画元素数')}  "
                  f"⑤助手消息={v.get('⑤ 助手消息数')}  "
                  f"⑦末条字数={v.get('⑦ 最后一个助手消息字数')}  "
                  f"⑧编辑器={v.get('⑧ 编辑器真字数')}")
            print(tag + "\n")
            prev = v
        if i < n:
            time.sleep(gap)
    return 0


if __name__ == "__main__":
    sys.exit(main())
