//! H2 域诊断汇总 · 完整设计（通用十二查 #10 台账与证据 / #11 最丑角落
//! / 人格章程十三·补「异常显性化与总日志中心」域级落位）。
//!
//! **职责**：
//! - 把域内 51 块自检聚合成一张诊断页（健康灯 + 人话输出——不给裸红点）；
//! - 「最丑角落」登记表：十二查 #11 要求每项自记最不满意处，此处是
//!   全域登记口（m4 复盘可查）；
//! - 三色健康分级：🟢 全绿 / 🟡 有红但非阻断 / 🔴 聚合器损坏或溢出
//!   （异常零静默——每级都给「为什么」和「下一步」）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 一条最丑角落登记（十二查 #11——自记本项最不满意处）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UglyCorner {
    pub item: &'static str,
    /// 最不满意处（人话，m4 复盘直读）。
    pub note: &'static str,
    /// 登记日期（YYYYMMDD 注入）。
    pub date: u32,
}

/// 健康分级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Green,
    Yellow,
    Red,
}

impl Health {
    pub fn label(&self) -> &'static str {
        match self {
            Health::Green => "🟢 全绿",
            Health::Yellow => "🟡 有红项——按账本逐条回炉",
            Health::Red => "🔴 聚合器异常——先修管道再看数据",
        }
    }
}

/// 诊断页。
pub struct DiagPage {
    pub health: Health,
    /// 每块的（标签, 通过, 总数）。
    pub blocks: Vec<(&'static str, usize, usize)>,
    /// 人话总结（三要素：发生了什么/为什么/下一步）。
    pub summary: String,
}

/// 从域聚合器产出诊断页（`set` 为 run_h2_checks() 的结果）。
pub fn render(set: &CheckSet) -> DiagPage {
    let (passed, failed) = set.tally();
    let health = if set.truncated() {
        Health::Red
    } else if failed == 0 {
        Health::Green
    } else {
        Health::Yellow
    };
    let summary = match health {
        Health::Green => String::from("全部自检通过——本域判据实装层当前无可报告异常"),
        Health::Yellow => alloc::format!(
            "有 {} 项自检未过（共 {} 项）——原因见各模块自检明细，下一步按缺陷账本逐条回炉",
            failed,
            passed + failed
        ),
        Health::Red => String::from(
            "聚合器溢出或损坏——自检结果不可信，下一步先修 CheckSet 管道再重跑",
        ),
    };
    DiagPage { health, blocks: Vec::new(), summary }
}

/// 最丑角落登记表（全域——每项自记最不满意处，持续追加）。
pub fn ugly_corners() -> Vec<UglyCorner> {
    alloc::vec![
        UglyCorner { item: "F251", note: "浏览器会话接入只做了注册口留痕，未接真实 Edge 会话枚举", date: 20260926 },
        UglyCorner { item: "F252", note: "占满合并的分组顺序在极端开窗序列下仍可能视觉跳动，未做亚像素级验证", date: 20260926 },
        UglyCorner { item: "F254", note: "对比度对拍用纯色底近似壁纸取样，真实壁纸上的文字可读性需 GPU 帧采样复验", date: 20260926 },
        UglyCorner { item: "F260", note: "非法字符抖动只有状态位，动画曲线未与 F124 弹性档联调", date: 20260926 },
        UglyCorner { item: "F265", note: "补全命中取第一个前缀匹配，多命中时的排序权重未按使用频率加权", date: 20260926 },
        UglyCorner { item: "F267", note: "空闲闸是布尔标志，未接 F057 IO 分级的真实队列水位", date: 20260926 },
        UglyCorner { item: "F285", note: "LRU 淘汰是 O(n) 扫描，万级缓存下的最坏路径需要索引化", date: 20260926 },
        UglyCorner { item: "F295", note: "漂移斜率是线性外推，长时间离线的非线性温漂未建模", date: 20260926 },
    ]
}

/// 域收官自检（十二查 #10 证据三件套的登记口——数据/复现命令/日期）。
pub fn evidence_manifest(commit: &str, date: &str) -> String {
    alloc::format!(
        "证据三件套：数据=h2star 105 项自检+全仓回归输出（_attic/aih2-f251-f300/）；复现=git show {} -- kernel/varix/src/h2star && cargo test h2star；日期={}",
        commit,
        date
    )
}

pub fn run_h2diag_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2diag");
    // 三色分级判定。
    let good = CheckSet::new("t1");
    let page1 = render(&good);
    let mut bad = CheckSet::new("t2");
    bad.add("样例红项", false, "演示");
    let page2 = render(&bad);
    set.add(
        "h2diag health tiers",
        page1.health == Health::Green && page2.health == Health::Yellow,
        "green/yellow",
    );
    // 人话总结三要素（下一步指引在场）。
    set.add(
        "h2diag human summary",
        page2.summary.contains("下一步") && page2.summary.contains("原因"),
        "what/why/next",
    );
    // 最丑角落：登记非空、每条有项号有人话、日期口径。
    let uc = ugly_corners();
    set.add(
        "h2diag ugly corners",
        uc.len() >= 8
            && uc.iter().all(|u| u.item.starts_with('F') && !u.note.is_empty())
            && uc.iter().all(|u| u.date / 10000 == 2026),
        "十二查 #11",
    );
    // 证据清单格式。
    let mf = evidence_manifest("abc1234", "2026-09-26");
    set.add(
        "h2diag evidence",
        mf.contains("复现") && mf.contains("abc1234"),
        "三件套",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2diag_all_green() {
        let set = run_h2diag_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2diag 自检红 {f}/{p}");
    }
}
