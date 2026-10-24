# -*- coding: utf-8 -*-
"""重写回本人卡：F3941-F3980 状态=已完成 + 正确结果（修复 ves157 文本污染）。"""
import io, os

SUMMARIES = {
    3941: "ves195_s08arch.rs 已落盘并注册判据（SEG 0x0980）——LoopLedger 开工闭环声明+DxAudit 五标准执法+ScopeMap 十六项映射+FamilyHub 家族枢纽。",
    3942: "ves196_s08auto.rs 已落盘并注册判据（SEG 0x0A80）——GlobalScheduler 独立档红线+DomainLedger 失败路由+Estimator 3000+ 总量估计。",
    3943: "ves197_s08review.rs 已落盘并注册判据（SEG 0x0B40）——TriadBench 三台合一+PreserveLedger 功能保全+TraceChain XOR 派生链+ProtoArbiter。",
    3944: "ves198_s08sim.rs 已落盘并注册判据（SEG 0x0B80）——SemNode 深度优先播报+Calibrator 对拍校准+DeviceGate 仿真不豁免真机 P0。",
    3945: "ves199_s08corpus.rs 已落盘并注册判据（SEG 0x0BC0）——五类语料三要素断言（伪语料拒收）+ShareHub 互通。",
    3946: "ves200_s08regress.rs 已落盘并注册判据（SEG 0x0C40）——BaselineBook 分叉红线+TriggerBook 幂等触发+RegressGate+WaiverBook 升级。",
    3947: "ves201_s08usertest.rs 已落盘并注册判据（SEG 0x0D40）——ParticipationAudit 参与度全域+CompBook 补偿断言+FindingLoop 三段闭环。",
    3948: "ves202_s08board.rs 已落盘并注册判据（SEG 0x0DC0）——MetricSnap 六指标+ViewOrder 顺序红线+HonestyAudit 粉饰 P0+CaliberBook。",
    3949: "ves203_s08apiref.rs 已落盘并注册判据（SEG 0x0E40）——ApiEntry 四段齐备+ApiBook 覆盖硬门+手写红线+漂移 repin。",
    3950: "ves204_s08examples.rs 已落盘并注册判据（SEG 0x0EC0）——ExampleIndex 孤岛扫描+ExampleStats 透明断言+reconcile 对账。",
    3951: "ves205_s08docmap.rs 已落盘并注册判据（SEG 0x0FC0）——DocMap 2 跳可达 BFS+bootstrapped 自举红线+SearchIndex。",
    3952: "ves206_s08ext.rs 已落盘并注册判据（SEG 0x10C0）——ExtEntry 三段硬门+BudgetLedger 隔离+GovGate ADR 单调。",
    3953: "ves207_s08thirdparty.rs 已落盘并注册判据（SEG 0x29C0）——ProtoHub 三态机+EqualChain 严格序过链绕检 P0+双签。",
    3954: "ves208_s08matrix.rs 已落盘并注册判据（SEG 0x2A40）——CompatMatrix 4×3×3×3=108 格双射下标+季度超期。",
    3955: "ves209_s08fuzz.rs 已落盘并注册判据（SEG 0x2A80）——SourceQuota 双配额 30%/20%+六不变量+降级违例 P0 执法。",
    3956: "ves210_s08walk.rs 已落盘并注册判据（SEG 0x2AC0）——WalkLedger 覆盖断言+summary_matches_walk 对账+日志优先裁决。",
    3957: "ves211_s08asset.rs 已落盘并注册判据（SEG 0x2DC0）——AssetMaster 全域分叉+GapBook 四态+SignOff 五签。",
    3958: "ves212_s08reserve.rs 已落盘并注册判据（SEG 0x2E40）——ReserveItem 三段断言+冻结拦截+false_promises/withdraw。",
    3959: "ves213_s09integ.rs 已落盘并注册判据（SEG 0x2EC0）——63 格 Stratified+ScriptRun 五幕确定性 FNV 指纹+三故障 RedlineField+三分流。",
    3960: "ves214_s09close.rs 已落盘并注册判据（SEG 0x2F40）——ApiLedger 十六族+LinkBook 五证据两轮上限+DualSign 异人双签+Closeout 20 项自检。",
    3961: "ves215_s09scope.rs 已落盘并注册判据（SEG 0x2F80）——SelfAuditPlan 160+28+七维+DimStandards 增维+EvidenceBook 双源。",
    3962: "ves216_s09s0103.rs 已落盘并注册判据（SEG 0x2FC0）——S01 六断言+S02 四色 4.5:1+S03 TripleOrder 三序一致。",
    3963: "ves217_s09s0406.rs 已落盘并注册判据（SEG 0x30C0）——S04 偏好链保守降级+S05 可读性阈值+S06 焦点环/冲突表。",
    3964: "ves218_s09s0708.rs 已落盘并注册判据（SEG 0x3140）——S07 五基准阈值+S08 五标准复审+误报率 PPM。",
    3965: "ves219_s09pairs.rs 已落盘并注册判据（SEG 0x3280）——pair_index 28 对双射+五维口径+ArbBook 仲裁留痕+机制九用上限。",
    3966: "ves220_s09api.rs 已落盘并注册判据（SEG 0x32C0）——三指针稳定性+double_check 缺口冗余双检+merge_redundant。",
    3967: "ves221_s09testasset.rs 已落盘并注册判据（SEG 0x3340）——四类资产+脱敏强制+OrphanMender 孤儿修补。",
    3968: "ves222_s09perfasset.rs 已落盘并注册判据（SEG 0x3380）——三档+越阈标记幂等+A11ySpec 专项。",
    3969: "ves223_s09secasset.rs 已落盘并注册判据（SEG 0x33C0）——三种别+FaceAuditor 三面复审+pref_violations 越权 P0。",
    3970: "ves224_s09duoasset.rs 已落盘并注册判据（SEG 0x3440）——双维双指针+caliber_unified+FlowCaliber 30 分钟窗。",
    3971: "ves225_s09docasset.rs 已落盘并注册判据（SEG 0x3480）——三类+画像标注+handbook_present+色标对拍。",
    3972: "ves226_s09fix.rs 已落盘并注册判据（SEG 0x34C0）——CrossScanner 六维+FixBook P0 硬门/回执催办/降级声明。",
    3973: "ves227_s09nine.rs 已落盘并注册判据（SEG 0x3540）——NineTable 九维主场位+RiskBook 四列字段执法+backfill。",
    3974: "ves228_s09prescore.rs 已落盘并注册判据（SEG 0x3580）——PreScoreCard 20 维+暂缓/复评/撤回+weighted_avg 无障碍双倍权。",
    3975: "ves229_s09walkpre.rs 已落盘并注册判据（SEG 0x35C0）——五角色剧本+PreWalkSheet 不代终查+RecordPlan 知情同意。",
    3976: "ves230_s09goreport.rs 已落盘并注册判据（SEG 0x3640）——四段合成 GO/NO-GO+backfill_orders+pre_go_implies_final。",
    3977: "ves231_s09drill.rs 已落盘并注册判据（SEG 0x3680）——四站空跑+PassPipe 直通单+GoGate+TrialRow 水印。",
    3978: "ves232_s09reserve.rs 已落盘并注册判据（SEG 0x36C0）——四口留位+occupy/migrate 迁移评审+scan_prewrite。",
    3979: "ves233_s09kb.rs 已落盘并注册判据（SEG 0x38C0）——四类知识条目+KnowledgeBase 检索+KwIndex 重建一致+index_broken。",
    3980: "ves234_s09close.rs 已落盘并注册判据（SEG 0x39C0）——VerifyRollup 20 项+PairReceiptBook 28 对+AssetRollup 六维+DualSign 禁同人。",
}

STAMP = "10-22 20:54"

BOARD = "dist/taskboard.md"
TMP = "dist/taskboard.md.tmp_wbv"

pat = None
import re
pat = re.compile(r"^## \[(VE-F(\d+))\]")

cur = None
changed = {"状态": 0, "结果": 0, "时间": 0}
out = io.open(TMP, "w", encoding="utf-8", newline="")
with io.open(BOARD, "r", encoding="utf-8", errors="replace") as f:
    for line in f:
        m = pat.match(line)
        if m:
            n = int(m.group(2))
            cur = n if 3941 <= n <= 3980 else None
        elif cur is not None:
            if line.startswith("- 状态:"):
                line = "- 状态: 已完成\n"
                changed["状态"] += 1
            elif line.startswith("- 结果:"):
                line = "- 结果: " + SUMMARIES[cur] + "\n"
                changed["结果"] += 1
            elif line.startswith("- 时间:"):
                line = "- 时间: " + STAMP + "\n"
                changed["时间"] += 1
        out.write(line)
out.close()
os.replace(TMP, BOARD)
size = os.path.getsize(BOARD)
print("changed:", changed, "size:", size)
assert size > 50_000_000, "board truncated!"
