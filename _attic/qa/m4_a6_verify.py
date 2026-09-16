# -*- coding: utf-8 -*-
# M4-A6 FINAL VERIFY v2（动态坐标版）：
# v1 教训：硬编码 (564,834) 是 M4 时代布局；startmenu-board/dailyFeed 改版后
# 按钮簇右移 ~22px，硬编码点落在 w=423 的「开始」容器 DIV 左缘（容器横跨整个
# 中央簇，aria-label 也叫「开始」——正是 M4 报告里的 selector 陷阱）→ 点击
# 落空。v2 改为：呼出后实时读 button[aria-label="开始"]（真 BUTTON 标签）的
# rect 中心，elementFromPoint 交叉验证身份后再物理点击。
# 流程：summon -> hover(动态) -> 物理点击开菜单 -> 再点关 -> 再点开
#       -> backdrop 点关 -> 移开光标收起。No manual FG.
import ctypes, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import (CDP, JS_STATE, find_taskbar_hwnds, move_cursor,
                         phys_click, cursor_pos, u32, grab)

APPLOG = os.path.join(os.environ["APPDATA"], "com.variable.app", "logs")


def tail_applog(n=6):
    try:
        files = sorted(f for f in os.listdir(APPLOG) if f.startswith("applog-"))
        with open(os.path.join(APPLOG, files[-1]), encoding="utf-8", errors="replace") as fh:
            return fh.readlines()[-n:]
    except Exception as e:
        return [f"applog-err: {e}"]


def start_btn_phys(cdp, dpr):
    """实时定位真开始按钮（BUTTON 标签）→ 物理像素中心；并 elementFromPoint
    交叉验证该点确实落在按钮内（防容器/叠层误命中）。返回 (px, py, ok)。"""
    r = json.loads(cdp.evaluate('''(function(){
        const b = document.querySelector('button[aria-label="\\u5f00\\u59cb"]');
        if (!b) return JSON.stringify({err: "no-button"});
        const r = b.getBoundingClientRect();
        const cx = r.x + r.width / 2, cy = r.y + r.height / 2;
        const el = document.elementFromPoint(cx, cy);
        const inside = !!(el && (el === b || b.contains(el)));
        return JSON.stringify({cx: cx, cy: cy, inside: inside, tag: el ? el.tagName : null});
    })()'''))
    if "err" in r:
        return None, None, False
    return int(r["cx"] * dpr), int(r["cy"] * dpr), r["inside"]


def main():
    rep = {}
    tb = find_taskbar_hwnds()[0]
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)

    # summon
    move_cursor(sw // 2, sh - 3)
    time.sleep(1.0)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    rep["summoned_shown"] = "tbw-hidden" not in (st1["rootClass"] or "")
    dpr = st1.get("dpr") or 1.25

    # hover REAL start button center（实时定位 + 身份交叉验证）
    px, py, ok = start_btn_phys(cdp, dpr)
    rep["btn_located"] = ok and px is not None
    rep["btn_at"] = [px, py]
    move_cursor(px, py)
    time.sleep(0.9)  # watcher cycles: hover timer 250ms + set_focus
    rep["fg_before_click"] = u32.GetForegroundWindow() == tb
    rep["hover_identity_ok"] = ok

    # physical click -> menu should OPEN
    phys_click()
    time.sleep(0.6)
    st2 = json.loads(cdp.evaluate(JS_STATE))
    rep["click1_menu_opened"] = st2["startMenu"]
    grab("_attic/qa/m4_a6_V1_menu_open.png")

    # physical click again -> menu should CLOSE (toggle)
    phys_click()
    time.sleep(0.6)
    st3 = json.loads(cdp.evaluate(JS_STATE))
    rep["click2_menu_closed"] = not st3["startMenu"]

    # open again, then backdrop click (menu area outside) -> close
    phys_click()
    time.sleep(0.6)
    st4 = json.loads(cdp.evaluate(JS_STATE))
    rep["click3_reopened"] = st4["startMenu"]
    if st4["startMenu"]:
        move_cursor(px, int(sh * 0.4))  # menu backdrop zone（上方菜单区，x 对齐按钮）
        time.sleep(0.4)
        phys_click()
        time.sleep(0.6)
        st5 = json.loads(cdp.evaluate(JS_STATE))
        rep["backdrop_click_closed"] = not st5["startMenu"]
        # move cursor away -> taskbar should collapse again
        move_cursor(sw // 2, sh // 2)
        time.sleep(2.0)
        st6 = json.loads(cdp.evaluate(JS_STATE))
        rep["away_collapsed"] = "tbw-hidden" in (st6["rootClass"] or "")
    rep["applog_tail"] = [l.strip() for l in tail_applog(8)]
    print(json.dumps(rep, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
