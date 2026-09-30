#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
vx-walkcheck-e · Varix STAR I · E 个性化域（F151-F170）走查清单生成器。

职责（F170 判据消费面，与 src/system/persona/verdict.ts 的 checklist JSON 同口径）：
1. 从本脚本内置映射表生成 checklist（--checklist）——三步验收（改/生效/回退）
   逐项列出，供人工走查打勾与 CI 周跑；
2. 校验 PersonaStudio 域总检页导出的结果 JSON（--verify <file>）——
   19/19 全绿 + 证据链完整才判定 PASS；
3. 冒烟自证（--selftest）——不依赖任何输入即可跑通。

走查材料族惯例（vx-walkcheck-c.py 同族）；结果归档 docs/acceptance/ 惯例目录。

用法：
    python tools/vx-walkcheck-e.py --checklist
    python tools/vx-walkcheck-e.py --verify docs/acceptance/e-domain-verdict.json
    python tools/vx-walkcheck-e.py --selftest
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass

# ---------- 19 项三步清单（与 verdict.ts buildProbes 一处一事实对齐） ----------


@dataclass(frozen=True)
class Item:
    fid: str
    name: str
    probe: str


ITEMS: list[Item] = [
    Item("F151", "主题令牌全集", "改令牌→全表热替换→哈希还原"),
    Item("F152", "实时预览编辑器", "预览会话 dirty→apply 写正身→discard 哈希还原"),
    Item("F153", "主题深浅自动切换", "改配置→状态机进入目标侧→暂停当日可还原"),
    Item("F154", "壁纸每日一换", "改池配置→抽取入账→配置还原"),
    Item("F155", "图标包热更换", "换包→指针切换生效→回退栈复原"),
    Item("F156", "指针编辑器", "改热点→方案生效→热点还原"),
    Item("F157", "声音混合器", "拉低通知音量→独立档位生效→恢复 100%"),
    Item("F158", "开始菜单布局预设", "切简洁预设→activeId 变更→切回效率"),
    Item("F159", "字体安全档", "缓存预检结果→判定档在位→清缓存"),
    Item("F160", "动效强度预演", "切减弱档→60% 缩放生效→恢复完整档"),
    Item("F161", "个性化档案导出", "导出→差异预览→导入哈希对拍"),
    Item("F162", "每应用主题例外", "加例外→清单生效→移除还原"),
    Item("F163", "桌面小组件", "加时钟组件→实例生效→移除"),
    Item("F164", "锁屏定制", "换时间式→配置即时预览→还原默认式"),
    Item("F165", "开机动画个性化", "改密度档→烘焙计划对拍→还原标准档"),
    Item("F166", "输入法皮肤", "关跟随主题→独立配色生效→恢复跟随"),
    Item("F167", "右键菜单自定义", "隐藏打开方式→生效→恢复"),
    Item("F168", "任务栏个性化", "切大图标档→56px 生效→还原标准档"),
    Item("F169", "快捷键查看器", "重录截图热键→冲突检测+生效→改回默认"),
]

STEPS = ("mutate", "take-effect", "rollback")
BUDGET_MINUTES = 30
CHECKLIST_FORMAT = "vx-walkcheck-e"
CHECKLIST_VERSION = 1


def build_checklist() -> dict:
    return {
        "format": CHECKLIST_FORMAT,
        "version": CHECKLIST_VERSION,
        "budgetMinutes": BUDGET_MINUTES,
        "steps": list(STEPS),
        "items": [{"id": i.fid, "name": i.name, "probe": i.probe} for i in ITEMS],
    }


def verify_result(path: str) -> tuple[bool, list[str]]:
    """校验 PersonaStudio 域总检导出的结果 JSON（DomainVerdict 形态）。"""
    problems: list[str] = []
    try:
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        return False, [f"无法读取结果文件: {e}"]

    items = data.get("items")
    if not isinstance(items, list):
        return False, ["结果缺少 items 数组"]

    expected_ids = [i.fid for i in ITEMS]
    got_ids = [it.get("id") for it in items]
    if got_ids != expected_ids:
        problems.append(f"覆盖面不符：期望 {expected_ids}，实际 {got_ids}")

    for it in items:
        fid = it.get("id", "?")
        steps = it.get("steps", [])
        if [s.get("step") for s in steps] != list(STEPS):
            problems.append(f"{fid}: 三步不完整（{[s.get('step') for s in steps]}）")
        for s in steps:
            if not s.get("ok"):
                problems.append(f"{fid}: {s.get('step')} 未过——{s.get('detail', '')}")
            if not s.get("evidence"):
                problems.append(f"{fid}: {s.get('step')} 缺证据路径（证据链完整率要求 100%）")
        if it.get("pass") is not True and not any(f.startswith(f"{fid}:") for f in problems):
            problems.append(f"{fid}: pass 标记与步骤不一致")

    total = data.get("total")
    if total != len(ITEMS):
        problems.append(f"total 应为 {len(ITEMS)}，实际 {total}")
    if data.get("allGreen") is not True:
        problems.append("allGreen ≠ true——存在红项，整域不达标")
    if data.get("evidenceComplete") is not True:
        problems.append("证据链完整率不足 100%")
    budget = data.get("elapsedMinutes")
    if not isinstance(budget, (int, float)) or budget >= BUDGET_MINUTES:
        problems.append(f"执行耗时 {budget} 超出 {BUDGET_MINUTES} 分钟预算")

    return (len(problems) == 0), problems


def selftest() -> int:
    """冒烟：checklist 生成 + verify 对合法/注入失败样本的双向判定。"""
    cl = build_checklist()
    assert cl["format"] == CHECKLIST_FORMAT
    assert len(cl["items"]) == 19
    assert all(set(it) == {"id", "name", "probe"} for it in cl["items"])

    good = {
        "items": [
            {
                "id": i.fid,
                "name": i.name,
                "probe": i.probe,
                "pass": True,
                "steps": [
                    {"step": s, "ok": True, "detail": "ok", "evidence": "e"} for s in STEPS
                ],
            }
            for i in ITEMS
        ],
        "total": 19,
        "allGreen": True,
        "evidenceComplete": True,
        "elapsedMinutes": 0.01,
    }
    # 用内存对象走同一校验逻辑（临时落盘）。
    import tempfile
    import os

    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as f:
        json.dump(good, f, ensure_ascii=False)
        tmp_good = f.name
    ok, problems = verify_result(tmp_good)
    os.unlink(tmp_good)
    if not ok:
        print("SELFTEST FAIL（合法样本被误判）:", problems)
        return 1

    bad = json.loads(json.dumps(good))
    bad["items"][3]["steps"][2]["ok"] = False  # F154 回退失败注入
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as f:
        json.dump(bad, f, ensure_ascii=False)
        tmp_bad = f.name
    ok, problems = verify_result(tmp_bad)
    os.unlink(tmp_bad)
    if ok or not any(p.startswith("F154:") for p in problems):
        print("SELFTEST FAIL（注入失败未被捕获）:", problems)
        return 1

    print("SELFTEST PASS：checklist 19 项 + verify 合法样本 PASS + 注入失败样本被检出")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="vx-walkcheck-e · E 个性化域走查清单")
    parser.add_argument("--checklist", action="store_true", help="生成 19 项三步走查 checklist JSON")
    parser.add_argument("--verify", metavar="FILE", help="校验域总检结果 JSON（19/19 全绿 + 证据链 100%）")
    parser.add_argument("--selftest", action="store_true", help="冒烟自证")
    args = parser.parse_args()

    if args.selftest:
        return selftest()
    if args.checklist:
        print(json.dumps(build_checklist(), ensure_ascii=False, indent=2))
        return 0
    if args.verify:
        ok, problems = verify_result(args.verify)
        if ok:
            print("VERIFY PASS：19/19 三步全绿，证据链完整，预算内")
            return 0
        print("VERIFY FAIL：")
        for p in problems:
            print(f"  - {p}")
        return 1
    parser.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
