//! 深化层五 · F150 生态域总判据续深（2026-09-27 批次五收尾件 · f150i）。
//!
//! 域总装与闸门面：20 项逐项行数快照（脚本实数硬登记，随批重生成）、
//! 90% 回炉线闸门、跨域接口总账（h 层 20 件 → 消费域映射）、季度
//! 闸门审计记录。本文件是 F150「域就是打勾标准」判据的机器总装——
//! 数据来自 wc 实数（生成命令见账本），一处一事实。

use super::f150g::completion_per_mille;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 域快照：逐项 (编号, 六层合计行数, 主册目标上限)——wc 实数登记
// ---------------------------------------------------------------------------

/// (编号, 实际行数, 目标上限)
pub const DOMAIN_SNAPSHOT: [(&'static str, usize, usize); 20] = [
    ("F131", 1792, 1950),
    ("F132", 1627, 1495),
    ("F133", 1418, 195),
    ("F134", 1606, 1430),
    ("F135", 1671, 2665),
    ("F136", 1794, 2470),
    ("F137", 1498, 1755),
    ("F138", 1389, 1430),
    ("F139", 1447, 1950),
    ("F140", 1403, 1690),
    ("F141", 1325, 1300),
    ("F142", 1480, 2340),
    ("F143", 1298, 1820),
    ("F144", 1325, 2600),
    ("F145", 1261, 1885),
    ("F146", 1538, 1430),
    ("F147", 1378, 1300),
    ("F148", 1475, 2470),
    ("F149", 1365, 1235),
    ("F150", 1452, 1300),
];

/// 快照合计行数（20 项；不含 ebase/mod 聚合器——另账）。
pub fn snapshot_total() -> usize {
    DOMAIN_SNAPSHOT.iter().map(|(_, a, _)| a).sum()
}

/// 快照合计目标。
pub fn snapshot_target() -> usize {
    DOMAIN_SNAPSHOT.iter().map(|(_, _, t)| t).sum()
}

// ---------------------------------------------------------------------------
// 90% 回炉线闸门：合计达标 → 逐项未达标名单（线是域级，账要逐项）
// ---------------------------------------------------------------------------

pub const GATE_LINE: u32 = 900;

pub struct GateVerdict {
    pub overall_per_mille: u32,
    pub passed: bool,
    /// 域达标但单项 <600‰ 的欠账项（如实点名，不许整域绿掩盖单项红）。
    pub laggards: alloc::vec::Vec<&'static str>,
}

pub fn gate_verdict() -> GateVerdict {
    let overall = completion_per_mille(snapshot_total(), snapshot_target());
    let laggards: alloc::vec::Vec<&'static str> = DOMAIN_SNAPSHOT
        .iter()
        .filter(|(_, a, t)| completion_per_mille(*a, *t) < 600)
        .map(|(n, _, _)| *n)
        .collect();
    GateVerdict {
        overall_per_mille: overall,
        passed: overall >= GATE_LINE,
        laggards,
    }
}

// ---------------------------------------------------------------------------
// 跨域接口总账：h 层 20 件 → 消费域映射（接口登记册）
// ---------------------------------------------------------------------------

/// (h 件, 接口名, 消费域/消费面)
pub const CROSS_DOMAIN_LEDGER: [(&'static str, &'static str, &'static str); 20] = [
    ("f131h", "贡献者页+判例线查询", "F129/F118"),
    ("f132h", "星卡馈送+迁移横幅", "F036"),
    ("f133h", "桌面图标源+预览网格", "F084/F093"),
    ("f134h", "令牌导入适配+三区装配", "F151"),
    ("f135h", "帮助锚点映射+结果卡片", "F119"),
    ("f136h", "欢迎五卡+启动器行", "F118"),
    ("f137h", "版本史行+三态徽标", "F135"),
    ("f138h", "关于页行+更新预约", "F122/F123"),
    ("f139h", "诊断体检灯+深链", "F120"),
    ("f140h", "热切语言清单+降级行", "F398"),
    ("f141h", "设置页行+对照通知", "F245/F246"),
    ("f142h", "安全面板+脱敏导出", "F120/F193"),
    ("f143h", "关于资产行+申请页+色板", "F123/F151"),
    ("f144h", "角标三处一致+争议深链", "F352/F148"),
    ("f145h", "学习路径+文章卡+待译角标", "F119"),
    ("f146h", "数据源选择行+插件页", "F101"),
    ("f147h", "同步页行+冲突入口", "F324"),
    ("f148h", "移交门+公示脱敏+指标卡", "F134"),
    ("f149h", "订阅行+缺季诚实+区块门", "F118/F123"),
    ("f150h", "域完成度总表+回归看板", "F200"),
];

/// 总检：账面 20 件与 h 层模块名清单一致（漏挂/多挂都点名）。
pub fn ledger_matches_wiring(declared: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    let mut gap: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for (h, _, _) in CROSS_DOMAIN_LEDGER.iter() {
        if !declared.contains(h) {
            gap.push(h);
        }
    }
    for d in declared {
        if !CROSS_DOMAIN_LEDGER.iter().any(|(h, _, _)| h == d) && !gap.contains(d) {
            gap.push(d);
        }
    }
    gap
}

// ---------------------------------------------------------------------------
// 季度闸门审计记录：每季闸门结论 append-only（回退不许改历史）
// ---------------------------------------------------------------------------

pub struct GateAudit {
    pub quarter: u32,
    pub passed: bool,
    pub overall_per_mille: u32,
}

pub struct GateAuditLog {
    records: alloc::vec::Vec<GateAudit>,
}

impl GateAuditLog {
    pub fn new() -> GateAuditLog {
        GateAuditLog { records: alloc::vec::Vec::new() }
    }

    /// 记录：季序必须递增；结论如实（失败也入册——闸门不是粉饰器）。
    pub fn record(&mut self, quarter: u32, passed: bool, per_mille: u32) -> Result<(), &'static str> {
        if let Some(last) = self.records.last() {
            if quarter <= last.quarter {
                return Err("季序倒退：闸门记录 append-only");
            }
        }
        self.records.push(GateAudit { quarter, passed, overall_per_mille: per_mille });
        Ok(())
    }

    /// 回退审计：相邻两季达标率下降 → 如实点名（不掩链）。
    pub fn regressions(&self) -> alloc::vec::Vec<u32> {
        self.records
            .windows(2)
            .filter(|w| w[1].overall_per_mille < w[0].overall_per_mille)
            .map(|w| w[1].quarter)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150I_TAG: &str = "stareco-F150-deep5b";

pub fn run_f150_deep5b_checks() -> CheckSet {
    let mut set = CheckSet::new(F150I_TAG);

    // 快照完整性
    set.add("f150i snapshot 20", DOMAIN_SNAPSHOT.len() == 20, "20 项全登记");
    set.add(
        "f150i snapshot total",
        snapshot_total() == 29542,
        "wc 实数合计（脚本生成口径）",
    );
    set.add(
        "f150i snapshot target",
        snapshot_target() == 34710,
        "目标合计=主册 34,710",
    );

    // 闸门
    let v = gate_verdict();
    set.add(
        "f150i gate honest",
        v.passed == (v.overall_per_mille >= 900),
        "闸门判定=口径",
    );
    set.add(
        "f150i laggards honest",
        v.laggards.iter().all(|l| {
            let (_, a, t) = *DOMAIN_SNAPSHOT.iter().find(|(n, _, _)| n == l).expect("found");
            a * 1000 / t < 600
        }),
        "欠账项名单可复算",
    );

    // 跨域账
    set.add("f150i ledger 20", CROSS_DOMAIN_LEDGER.len() == 20, "接口 20 件全账");
    let declared: alloc::vec::Vec<&'static str> =
        CROSS_DOMAIN_LEDGER.iter().map(|(h, _, _)| *h).collect();
    set.add(
        "f150i ledger matches",
        ledger_matches_wiring(&declared).is_empty(),
        "账面与挂载一致",
    );
    let partial = &declared[..5];
    set.add(
        "f150i ledger gap",
        ledger_matches_wiring(partial).len() == 15,
        "漏挂 15 件点名",
    );

    // 闸门审计
    let mut log = GateAuditLog::new();
    let _ = log.record(1, false, 307);
    let _ = log.record(2, false, 497);
    let _ = log.record(3, false, 635);
    let _ = log.record(4, false, 781);
    set.add(
        "f150i audit no backfill",
        log.record(3, true, 900).is_err(),
        "季序倒退拒绝",
    );
    set.add("f150i audit len", log.len() == 4, "四季如实");
    set.add("f150i audit no regress", log.regressions().is_empty(), "逐季上行零回退");
    let _ = log.record(5, false, 877);
    let _ = log.record(6, false, 500); // 模拟回退季（闸门如实记账）
    set.add(
        "f150i audit regress seen",
        log.regressions() == alloc::vec![6],
        "下降季如实点名",
    );
    set.add("f150i audit len after", log.len() == 6, "回退季也入册（append-only）");

    set
}

#[cfg(test)]
mod deep5b_tests {
    use super::*;

    #[test]
    fn snapshot_targets_match_master() {
        // 目标列必须与分工书一致——错一个就是账面污染。
        let expected: [usize; 20] = [
            1950, 1495, 195, 1430, 2665, 2470, 1755, 1430, 1950, 1690, 1300,
            2340, 1820, 2600, 1885, 1430, 1300, 2470, 1235, 1300,
        ];
        for (i, (_, _, t)) in DOMAIN_SNAPSHOT.iter().enumerate() {
            assert_eq!(*t, expected[i], "第 {} 项目标不符", i);
        }
    }

    #[test]
    fn gate_honest_when_below() {
        let v = gate_verdict();
        // 域在 90% 线下时闸门必须如实红——不许粉饰。
        assert_eq!(v.passed, v.overall_per_mille >= 900);
    }
}
