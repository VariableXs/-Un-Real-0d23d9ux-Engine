# -*- coding: utf-8 -*-
"""任务板写回：F3850-F3857 八卡标完成（流式，先备份已在 bash 完成）。"""
import io, os, re

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"
TMP = SRC + ".tmp_wb1022"

# 卡号 -> (模块文件, SEG, 摘要)
DONE = {
    "VE-F3850": ("ves150_srtest.rs", "0xC600", "八类断言（闭环/跟随/文本/噪音/公告/地标/跳空/预算，闭环跳空P0）+生成器指纹漂移检测+真机矩阵9格（缺真机P0/缺格补跑）+七域56组合覆盖去重+断流漏检审计。"),
    "VE-F3851": ("ves151_srdebug.rs", "0xC700", "五列仪表（打开才采样）+断链扫描复用单源（漏检断言修正）+读屏模拟器（语义树深度优先播报+角色映射）+真机对拍校准（失真格数定位）+导航序严格单调+家族二十三握手。"),
    "VE-F3852": ("ves152_srdocs.rs", "0xC800", "三册结构（开发/测试/用户）+义务前置CI拦截+同源指纹对账（漂移修复归零）+自举复用（家族供给缺口补齐）+2跳复用红线迷宫审计。"),
    "VE-F3853": ("ves153_n08final.rs", "0xC900", "断链硬门S版（零发生+七域证据册+缺格补测）+序断言全域（首分歧位定位）+虚拟化语义保留证据+三方对拍终版（N08语义真源仲裁+仲裁计数+终版收敛）。"),
    "VE-F3854": ("ves154_srfuzz.rs", "0xCA00", "五级变异（风暴/乱序/标注缺失/嵌套畸形/竞态）+四不变量分级（断链=公告丢P0）+看门狗挂起终止+语料回放驱动+P0最小化闭环+预算封顶。"),
    "VE-F3855": ("ves155_srwalk.rs", "0xCB00", "63格走查矩阵（七域×三读屏器×三任务）+真实任务制红线（形式化检出重走）+发现三级分级（阻断未清不完结）+参与复述+日志优先对照裁决。"),
    "VE-F3856": ("ves156_sri18n.rs", "0xCC00", "lang单源标注（漏标P1）+播报语言跟随断言（读错防线+修正闭环）+多语言公告语种核对+RTL序严格递减断言（相等序判乱+LTR镜像）。"),
    "VE-F3857": ("ves157_s03integ.rs", "0xCD00", "63格分层抽样（七域覆盖底线）+20分钟四幕混流剧本（指纹重演确定性）+三故障注入（断链P0/风暴吸收/跳空兜底）+三分流裁决（P0即断/立案/兜底）。"),
}

NOW = "10-22 19:12"
cur_card = None
patched = 0
patched_ids = set()

def fmt_result(card):
    f, seg, desc = DONE[card]
    return f"- 结果: {f} 已落盘并注册判据（SEG {seg}）——{desc}"

with io.open(SRC, "r", encoding="utf-8", newline="") as fin, \
     io.open(TMP, "w", encoding="utf-8", newline="") as fout:
    for line in fin:
        m = re.match(r"^## \[(VE-F38(5[0-7]))\]", line)
        if m:
            cur_card = m.group(1)
        elif line.startswith("- 状态: ") and cur_card in DONE:
            line = line.replace("- 状态: 已领", "- 状态: 已完成", 1)
        elif line.startswith("- 结果: ") and cur_card in DONE:
            line = fmt_result(cur_card) + "\n"
            patched_ids.add(cur_card)
        elif line.startswith("- 时间: ") and cur_card in DONE:
            line = f"- 时间: {NOW}\n"
        fout.write(line)

os.replace(TMP, SRC)
print("patched cards:", len(patched_ids), sorted(patched_ids))
