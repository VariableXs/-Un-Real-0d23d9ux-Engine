# -*- coding: utf-8 -*-
"""F3981-F4000 (S10 组) 状态=已完成 + 正确结果写回。"""
import io, os, re

SUMMARIES = {
    3981: "ves235_s10ledger.rs 已落盘并注册判据（SEG 0x3AC0）——LedgerFinal 冻结对账+ThemeMap 二十项唯一性+PromiseBook 双承诺回执+FamilyPeerCheck 五域同构对拍。",
    3982: "ves236_s10dict.rs 已落盘并注册判据（SEG 0x3DC0）——五类术语词典冻结+冲突仲裁+八组用法比对+二选一留痕+家族词典对拍。",
    3983: "ves237_s10jitest.rs 已落盘并注册判据（SEG 0x3EC0）——40 场景四断言（a11y 硬门）+八链路贯通矩阵+隔离复测+FNV 留痕库。",
    3984: "ves238_s10perf.rs 已落盘并注册判据（SEG 0x3FC0）——五档阈值复核（含朗读排队专项档）+降级声明+家族对拍仲裁+FreezePrecheck 冻结闸。",
    3985: "ves239_s10sec.rs 已落盘并注册判据（SEG 0x40C0）——三面终审+六红线全量复核+越权偏好写入专项断言（pref_overreach）。",
    3986: "ves240_s10a11y.rs 已落盘并注册判据（SEG 0x41C0）——双维证据终版核验+心流终验（75 线+预案制）+五角色资格闸（禁入终走查）。",
    3987: "ves241_s10obs.rs 已落盘并注册判据（SEG 0x42C0）——四类埋点口径冻结+六项面板终版+采样率复核（零采样失真检出）。",
    3988: "ves242_s10fuzz.rs 已落盘并注册判据（SEG 0x43C0）——四主面归集（含偏好序列面）+三零线（崩溃/挂起/越权写）+新崩溃最小复现立案。",
    3989: "ves243_s10docs.rs 已落盘并注册判据（SEG 0x44C0）——八组文档缺章检出+专册双规范终版+四 schema 版本冻结+冻结哈希。",
    3990: "ves244_s10freeze.rs 已落盘并注册判据（SEG 0x4540）——三基线冻结（未核先冻作废重冻）+FNV 冻结哈希留痕+冻结后直改检出+变更闸转纪律。",
    3991: "ves245_s10ext.rs 已落盘并注册判据（SEG 0x45C0）——四口扩展位有效性核验+第三方契约三段审+无障碍基线硬门+越权申报拒批。",
    3992: "ves246_s10recon.rs 已落盘并注册判据（SEG 0x46C0）——11 项终审回执对账+遗留三路处置（专项只许转 T 域）+账实相符声明。",
    3993: "ves247_s10score.rs 已落盘并注册判据（SEG 0x4740）——20 维终版评分+证据暂缓+偏差说明（容忍 5）+三 90 核验+无障碍双倍权。",
    3994: "ves248_s10walk.rs 已落盘并注册判据（SEG 0x47C0）——五角色剧本转正式+知情同意禁录+不可达最高立案+发现三级分级+阻断复走闭环。",
    3995: "ves249_s10handover.rs 已落盘并注册判据（SEG 0x48C0）——移交十件封装+脱敏硬门+哈希篡改检出重封+交接面三类（国际化无障碍承接点）。",
    3996: "ves250_s10change.rs 已落盘并注册判据（SEG 0x49C0）——五步变更纪律严格按序（越步作废）+无障碍影响面专列+禁同人双签+违诺处置通告。",
    3997: "ves251_s10kb.rs 已落盘并注册判据（SEG 0x4AC0）——四类知识归档（未脱敏拒收）+指纹合并去重+倒排索引重建一致性+五角色方法沉淀。",
    3998: "ves252_s10reserve.rs 已落盘并注册判据（SEG 0x4BC0）——域级三口预留（预写越界回退）+占用迁移评审+断言位齐备断言。",
    3999: "ves253_s10evidence.rs 已落盘并注册判据（SEG 0x4DC0）——证据三件套（验收/冻结/纪律）封装哈希+无障碍专卷硬门+公示位登记。",
    4000: "ves254_s10declare.rs 已落盘并注册判据（SEG 0x6440）——200/200 总对账硬门+八组收口 8/8+六重硬门宣告+三 90 确认+条件核销+家族第十六位登记+失实撤回。",
}

STAMP = "10-22 21:35"
BOARD = "dist/taskboard.md"
TMP = "dist/taskboard.md.tmp_wbv2"

pat = re.compile(r"^## \[(VE-F(\d+))\]")
cur = None
changed = 0
out = io.open(TMP, "w", encoding="utf-8", newline="")
with io.open(BOARD, "r", encoding="utf-8", errors="replace") as f:
    for line in f:
        m = pat.match(line)
        if m:
            n = int(m.group(2))
            cur = n if 3981 <= n <= 4000 else None
        elif cur is not None:
            if line.startswith("- 状态:"):
                line = "- 状态: 已完成\n"
                changed += 1
            elif line.startswith("- 结果:"):
                line = "- 结果: " + SUMMARIES[cur] + "\n"
            elif line.startswith("- 时间:"):
                line = "- 时间: " + STAMP + "\n"
        out.write(line)
out.close()
os.replace(TMP, BOARD)
size = os.path.getsize(BOARD)
print("status changed:", changed, "size:", size)
assert size > 50_000_000, "board truncated!"
