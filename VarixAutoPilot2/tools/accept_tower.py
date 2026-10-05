"""调度塔实弹验收（2026-10-06 实测重写版）。

验收项（每项带客观证据，失败即停并写明原因）：
  S0  活跃会话识别：拿到当前会话 id 作 anchor，结束后切回
  S1  新建会话实弹：点「新建任务」→ 判该空白会话可发送 → 填提示词 → 真发送
      → 三重证据之一成立 → 差集捕获新会话 id → 写 dispatch/workers/W99.conv
  S2  文件总线实弹：轮询 W99.state 出现 BUSY→READY（AI 在另一会话独立执行，≤420s）
  S3  切换+续单实弹：switch_conv(W99) → 发 PROBE-2 → 轮询 READY PROBE-2 → 切回 anchor
  S4  汇总 PASS/FAIL，LEDGER 记行

2026-10-06 实测修正的三处旧缺陷：
  1. anchor 判据：侧栏项本身无选中 class，选中态在后代的 `_selected_` 类上。
  2. 发送证据：[data-message-author-role] 在本版 DOM 恒 0 个；chars==0 因 placeholder
     恒 25 字也恒不成立 —— 旧双重证据两条都是死路，必假阴性。改用 tag/会话数/真实字数。
  3. 新建会话顺序：不能先 wait_idle（全局忙会把自己卡死），应先点新建再判目标会话。

已知风险（日志里如实呈现）：
  - 账号被限流（429）时，消息提交后后端不建会话、AI 不响应 —— S2 会超时，
    日志会明确指向限流而不是代码缺陷。

日志：dispatch/acceptance.log（stdout 同步打印）
"""

import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import dispatch_tower as tower

_LOG = tower.DISPATCH / "acceptance.log"
_ROOT_S = str(tower._ROOT).replace("\\", "/")


def alog(msg: str):
    tower.log(msg)
    try:
        with open(_LOG, "a", encoding="utf-8") as f:
            f.write(f"[{time.strftime('%H:%M:%S')}] {msg}\n")
    except OSError:
        pass


def prompt_1(tag: str) -> str:
    return f"""【产线验收探针 · W99 · {tag}】你是调度塔的验收工人，只做下面三件事，做完即止，
不要写别的文件、不要输出长篇说明：

1. 把文件 `{_ROOT_S}/dispatch/workers/W99.state` 写为一行：`BUSY PROBE-1`
2. 把同一个文件覆写为一行：`READY PROBE-1`
3. 把文件 `{_ROOT_S}/dispatch/reports/PROBE-1.md` 写三行：验收时间 / 你执行的动作 / 一句话结论

路径必须用上面给的绝对路径，不要改写其他任何文件。"""


def prompt_2(tag: str) -> str:
    return f"""续单（验收 PROBE-2 · {tag}）：同一协议再走一遍——
1. `{_ROOT_S}/dispatch/workers/W99.state` 写为：`BUSY PROBE-2`
2. 覆写为：`READY PROBE-2`
3. 报告写 `{_ROOT_S}/dispatch/reports/PROBE-2.md`（三行即可）。做完即止。"""


def poll_state(wid: str, expect: str, timeout_s: int) -> str:
    t0 = time.time()
    while time.time() - t0 < timeout_s:
        s = tower.read_state(wid)
        if expect in s:
            return s
        time.sleep(3)
    return tower.read_state(wid)


def main() -> int:
    tower.WORKERS_DIR.mkdir(parents=True, exist_ok=True)
    tower.REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    results = []

    # ── S0 活跃会话 ──
    anchor = tower.active_conv()
    alog(f"S0 活跃会话识别：anchor={anchor or '(未识别——结束后不切回)'}")
    results.append(("S0 活跃会话识别", bool(anchor)))

    # ── S1 新建会话实弹 ──
    tag1 = f"VAP{uuid.uuid4().hex[:8]}"
    known = tower.snapshot_conv_ids()
    alog(f"S1 新建会话实弹开始（现有会话 {len(known)} 个，tag={tag1}）…")
    conv, ev = tower.new_conversation(prompt_1(tag1), known, tag=tag1)
    results.append(("S1 新建+首条发送", bool(conv)))
    if not conv:
        alog(f"[FAIL] S1 失败：{ev}")
        alog("[HINT] 若证据是「三重均未成立」或「没捕捉到新会话 id」，先看页面是否报 429 限"
             "流——限流时后端不建会话，属环境阻塞而非代码缺陷。")
        if anchor:
            tower.switch_conv(anchor)   # 把视图还给用户，不留空白页
            alog(f"视图已切回 anchor {anchor[:12]}…")
        return _summary(results)
    tower.write_conv("W99", conv)
    tower.ledger("验收发车", "PROBE-1", "W99", f"conv={conv[:12]}… {ev}")
    alog(f"S1 通过：新会话 {conv[:16]}…（{ev}）")

    # ── S2 文件总线 ──
    alog("S2 轮询 W99.state（等 AI 写 BUSY→READY PROBE-1，≤420s）…")
    saw_busy = "BUSY" in poll_state("W99", "BUSY", 180)
    s2 = poll_state("W99", "READY PROBE-1", 420)
    saw_ready = "READY PROBE-1" in s2
    alog(f"S2 BUSY={saw_busy} READY={saw_ready} state={s2[:40]!r}")
    results.append(("S2 worker 写 BUSY", saw_busy))
    results.append(("S2 worker 写 READY PROBE-1", saw_ready))
    if not saw_ready:
        alog("[HINT] 5 分钟无 READY——去 W99 会话看：①是否弹了权限确认（允许完全访问）"
             "②是否 429 限流 ③AI 是否误解了指令。")

    # ── S3 切换 + 续单 ──
    tag2 = f"VAP{uuid.uuid4().hex[:8]}"
    alog(f"S3 切到 W99 发续单 PROBE-2（tag={tag2}）…")
    ok_switch = tower.switch_conv(conv)
    ok_send = False
    ev3 = "未执行（切换失败）"
    if ok_switch:
        if tower.wait_idle(90):
            ok_send, ev3 = tower.send_text(prompt_2(tag2), tag2)
        else:
            ev3 = "切过去后 90s 仍忙"
    alog(f"S3 切换={ok_switch} 续单发送={ok_send}（{ev3}）")
    results.append(("S3 切换会话", ok_switch))
    results.append(("S3 续单发送", ok_send))

    s3 = poll_state("W99", "READY PROBE-2", 420)
    ok3 = "READY PROBE-2" in s3
    alog(f"S3 续单闭环={'通过' if ok3 else '未确认'} state={s3[:40]!r}")
    results.append(("S3 续单 READY PROBE-2", ok3))

    # 视图还给用户
    if anchor:
        tower.switch_conv(anchor)
        alog(f"视图已切回 anchor {anchor[:12]}…")
    return _summary(results)


def _summary(results) -> int:
    alog("── 验收汇总 ──")
    passed = sum(1 for _, ok in results if ok)
    for name, ok in results:
        alog(f"  {'PASS' if ok else 'FAIL'}  {name}")
    total = len(results)
    verdict = "PASS" if passed == total else f"PARTIAL({passed}/{total})"
    alog(f"结论：{verdict}")
    tower.ledger("验收结论", "-", "W99", f"{verdict} {passed}/{total}")
    return 0 if passed == total else 1


if __name__ == "__main__":
    sys.exit(main())
