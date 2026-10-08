"""★ 技能清单探测（真机实测版）★

## 为什么不是「输入 / 抓补全面板」
v1 实测：输入 `/` 后，页面上**只多出一个斜杠本身**（`含斜杠条目 = ["/"]`），
补全面板根本没弹。WorkBuddy 5.6.2 的技能入口是侧栏那个
**「专家·技能·连接器」** 标签页（class `conversation-list-tab-button`），
不是斜杠命令。所以走斜杠是错的路。

## 真实结构（实测）
技能卡片 class = `ec-card-*`：
  ec-card-role      「微信公众号运营专家」← 技能名
  ec-card-subtitle  「号运运」            ← 副标题/作者
  ec-card-desc      技能描述
  ec-card-tag       标签（微信生态 / 图文排版 …）
共抓到 62 张。

## ★ 副作用与还原 ★
点那个标签会**切走用户的侧栏视图**（助理 → 专家·技能·连接器）。
所以必须记下当前标签、抓完切回。
实测坑：切回后`ec-card-*` 仍有 219 个残留（虚拟滚动缓存 DOM），
对话列表能恢复但卡片不会立刻消失——
所以**还原判据用「对话列表回来了」**，而不是「卡片清空了」。

## 安全约定
1. 输入框有内容时**拒绝探测**（不能弄乱用户写的东西）
2. 无论成败都切回原标签
3. 不点击任何卡片（只读，不触发）

用法：python skills_probe.py
"""

import importlib.util
import json
import sys
import time
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "st", str(Path(__file__).parent / "send_selftest.py")
)
st = importlib.util.module_from_spec(spec)
spec.loader.exec_module(st)

TAB_EXPERT = "专家·技能·连接器"
TAB_ASSIST = "助理"

CHARS_JS = """(() => {
  const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!e) return -1;
  const c = e.cloneNode(true);
  c.querySelectorAll('[data-slate-placeholder]').forEach(n => n.remove());
  return (c.textContent || '').trim().length;
})()"""


def click_tab(label: str) -> bool:
    """点侧栏标签。返回是否点到。

    ★ 必须用 .conversation-list-tab-button 精确 class ★
    早先用「遍历所有 span/button/div 比innerText」，点「助理」时
    命中了**内层**的其它元素，click 发出去了但视图没切换——
    实测：active 标签仍是「专家」、编辑器与发送键全为 0、填入会静默失败。
    """
    r = st.call_js(
        """(() => {
      const btns = Array.from(document.querySelectorAll('.conversation-list-tab-button'));
      for (const b of btns) {
        if ((b.innerText || '').trim() === %s) { b.click(); return true; }
      }
      return false;
    })()"""
        % json.dumps(label)
    )
    return bool(r)


def active_tab() -> str:
    r = st.call_js(
        """(() => {
      const a = Array.from(document.querySelectorAll('.conversation-list-tab-button'))
        .filter(b => /active/.test(String(b.className)));
      return a.length ? (a[0].innerText || '').trim() : '';
    })()"""
    )
    return r if isinstance(r, str) else ''


def wait_assist_ready(timeout: float = 8.0) -> bool:
    """等「助理」页就绪（编辑器与发送键都在）。

    ★ 这是填入的前置条件 ★
    专家/项目/资料库等页**没有输入框**（实测编辑器与发送键均为 0），
    在那几页上填入会静默失败。必须确认就绪。
    """
    import time as _t
    t0 = _t.time()
    while _t.time() - t0 < timeout:
        v = st.call_js(
            """(() => ({
      ed: !!document.querySelector('div[data-slate-editor="true"][contenteditable="true"]'),
      btn: document.querySelectorAll('button.cr-send-button').length,
    }))()"""
        )
        if isinstance(v, dict) and v.get('ed') and v.get('btn', 0) > 0:
            return True
        _t.sleep(0.6)
    return False


def conv_count() -> int:
    r = st.call_js("document.querySelectorAll('div.conversation-item').length")
    return r if isinstance(r, int) else -1


def main() -> int:
    if not st.devtools_alive():
        print("[ERR] 9222 端口不通。先双击桌面「VarixAutoPilot 开端口.bat --kill」")
        return 1

    # ① 安全闸：输入框有内容就不动
    before = st.call_js(CHARS_JS)
    print(f"探测前编辑器真字数: {before}")
    if isinstance(before, int) and before > 0:
        print("[拒绝] 输入框里有内容，探测会切走你的侧栏视图，先清空再试")
        return 2

    # ② 记下当前标签（用于还原）
    cur = st.call_js(
        """(() => {
      for (const b of document.querySelectorAll('.conversation-list-tab-button')) {
        if (/active|selected/.test(String(b.className))) {
          return (b.innerText || '').trim();
        }
      }
      return '';
    })()"""
    )
    origin = cur if isinstance(cur, str) and cur else TAB_ASSIST
    print(f"当前标签: {origin!r}")

    try:
        # ③ 切到专家页
        if origin == TAB_EXPERT:
            pass   # 已经在那一页
        elif not click_tab(TAB_EXPERT):
            print("[ERR] 找不到「专家·技能·连接器」标签（WorkBuddy 可能改版了）")
            return 3
        time.sleep(2.2)

        # ④ 抓卡片（只读，不点任何卡片）
        v = st.call_js(
            """(() => {
      const vis = (e) => {
        const r = e.getBoundingClientRect();
        return r.width > 0 && r.height > 0 && getComputedStyle(e).opacity !== '0';
      };
      const out = [];
      for (const e of document.querySelectorAll('div,span,a,button,li')) {
        if (!vis(e)) continue;
        let own = '';
        for (const n of e.childNodes) if (n.nodeType === 3) own += n.textContent;
        own = own.trim().replace(/\s+/g, ' ');
        if (!own || own.length > 60) continue;
        const cls = String(e.className || '');
        // 角色 = 技能名；标签 = 领域词
        if (/ec-card-role|ec-card-tag/.test(cls)) {
          out.push({ 文本: own, 类型: /role/.test(cls) ? '技能' : '标签', cls: cls.slice(0, 40) });
        }
      }
      const uniq = new Map();
      for (const x of out) {
        const k = x.类型 + '\\u0001' + x.文本;
        if (!uniq.has(k)) uniq.set(k, x);
      }
      return { 总数: uniq.size, 条目: Array.from(uniq.values()) };
    })()"""
        )
    finally:
        # ★ 无论成败都还原视图，且必须确认就绪 ★
        back = origin if origin != TAB_EXPERT else TAB_ASSIST
        if active_tab() != back:
            click_tab(back)
        ok_ready = wait_assist_ready()

    # ⑤ 还原判据：对话列表回来（卡片残留是虚拟滚动缓存，不可当判据）
    convs = conv_count()
    print(f"还原后：active 标签={active_tab()!r} 对话项={convs} 助理页就绪={ok_ready}")
    if not ok_ready:
        print("[WARN] 助理页没就绪（编辑器/发送键缺失），填入会失败")

    items = (v or {}).get("条目") or []
    skills = [x for x in items if x.get("类型") == "技能"]
    tags = [x for x in items if x.get("类型") == "标签"]
    print(f"\n抓���技能 {len(skills)} 个，标签 {len(tags)} 个：")
    for x in skills[:30]:
        print(f"   {x['文本']}")

    # 输出机器可读
    outp = Path(__file__).parent / "skills.json"
    outp.write_text(
        json.dumps(
            {"skills": [x["文本"] for x in skills], "tags": [x["文本"] for x in tags]},
            ensure_ascii=False, indent=1,
        ),
        encoding="utf-8",
    )
    print(f"\n已写 {outp}")
    if convs <= 0 or not ok_ready:
        print("[WARN] 侧栏视图没恢复干净，请手工点回「助理」")
        return 4
    return 0


if __name__ == "__main__":
    sys.exit(main())
