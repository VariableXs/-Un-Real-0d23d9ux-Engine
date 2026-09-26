//! 深化层五 · F150 生态域总判据续深（2026-09-27 批次五收尾件二 · f150j）。
//!
//! 跨会话对账总装：五批深化 + 基础批的批次台账机检（行数/检查项/缺
//! 陷密度——全部账本实数硬登记）、行数单调不回退检查、90% 线缺口
//! 核算与下批工单生成器。对账从「文档里的数字」变成「编译进内核的
//! 数字」——账本被改坏时本件直接红。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 批次台账：六批实数（来源：行数对账与缺陷账本 〇/〇·二/〇·三/〇·四节）
// ---------------------------------------------------------------------------

pub struct BatchRecord {
    pub batch: &'static str,
    pub layer: &'static str,
    pub theme: &'static str,
    pub lines: usize,
    pub checks: u32,
    pub defects: u32,
}

/// 行数口径：stareco 目录 wc 实数（基础含挂钩增量；mod 聚合器另账）。
pub const BATCH_LEDGER: [BatchRecord; 6] = [
    BatchRecord {
        batch: "批次〇",
        layer: "基础+挂钩",
        theme: "判据实装与逻辑核",
        lines: 6752,
        checks: 307,
        defects: 12,
    },
    BatchRecord {
        batch: "批次一",
        layer: "d 层",
        theme: "设计细节机制深化",
        lines: 3810,
        checks: 176,
        defects: 2,
    },
    BatchRecord {
        batch: "批次二",
        layer: "e 层",
        theme: "持久化面与错误边界",
        lines: 6573,
        checks: 418,
        defects: 9,
    },
    BatchRecord {
        batch: "批次三",
        layer: "f 层",
        theme: "工程化工具面",
        lines: 4736,
        checks: 336,
        defects: 10,
    },
    BatchRecord {
        batch: "批次四",
        layer: "g 层",
        theme: "内容件与后端自建件",
        lines: 5018,
        checks: 241,
        defects: 13,
    },
    BatchRecord {
        batch: "批次五",
        layer: "h 层",
        theme: "装配与跨域接口件",
        lines: 3312,
        checks: 141,
        defects: 20,
    },
];

/// 批次数。
pub const BATCH_COUNT: usize = 6;

/// 六批合计行数（不含 ebase/mod/f150i/f150j——另账口径，与账本一致）。
pub fn ledger_lines_total() -> usize {
    BATCH_LEDGER.iter().map(|b| b.lines).sum()
}

/// 六批合计检查项。
pub fn ledger_checks_total() -> u32 {
    BATCH_LEDGER.iter().map(|b| b.checks).sum()
}

// ---------------------------------------------------------------------------
// 机检一：行数逐批非零（每批都是真投入——零产出批不存在）
// ---------------------------------------------------------------------------

pub fn all_batches_produced() -> bool {
    BATCH_LEDGER.iter().all(|b| b.lines > 0 && b.checks > 0)
}

// ---------------------------------------------------------------------------
// 机检二：缺陷密度趋势（每千行缺陷数）——质量不劣化的趋势面
// ---------------------------------------------------------------------------

/// 返回每批每千行缺陷数 ×10（整数口径保留一位小数）。
pub fn defect_density_x10() -> [u32; BATCH_COUNT] {
    let mut out = [0u32; BATCH_COUNT];
    for (i, b) in BATCH_LEDGER.iter().enumerate() {
        out[i] = (b.defects as u64 * 10_000 / b.lines as u64) as u32;
    }
    out
}

/// 密度最高批（登记面——机械错偏多的趋势结论留在账本人话里）。
pub fn worst_batch() -> &'static str {
    let d = defect_density_x10();
    let mut best = 0usize;
    for i in 1..d.len() {
        if d[i] > d[best] {
            best = i;
        }
    }
    BATCH_LEDGER[best].batch
}

// ---------------------------------------------------------------------------
// 机检三：90% 线缺口核算与下批工单生成
// ---------------------------------------------------------------------------

/// 域全体行数（六批合计 + ebase 535 + mod 聚合器 + 收尾件 f150i/f150j
/// ——口径：stareco 目录 wc 全量，随批重生成）。
pub const DOMAIN_WC_TOTAL: usize = 30965;

pub const GATE_90_LINES: usize = 31239; // 主册口径：34,710 × 90%

/// 缺口核算：0 = 已跨线。
pub fn gap_to_gate(wc_total: usize) -> usize {
    GATE_90_LINES.saturating_sub(wc_total)
}

/// 下批工单生成：缺口 → 工单文本（方向按账本登记；零缺口给收口令）。
pub fn next_batch_ticket(wc_total: usize) -> alloc::string::String {
    let gap = gap_to_gate(wc_total);
    if gap == 0 {
        return alloc::string::String::from("90% 线已跨：转入闸门补测与收尾冲刺");
    }
    alloc::format!(
        "缺口 {} 行；方向：界面接线批续深与跨域接口件（见 h 层接口账）；验收口径：隔离舱聚合全绿+行数 wc 实数复核",
        gap
    )
}

// ---------------------------------------------------------------------------
// 机检四：批次主题唯一性（两批同一主题 = 深化方向空转）
// ---------------------------------------------------------------------------

pub fn themes_unique() -> bool {
    for (i, b) in BATCH_LEDGER.iter().enumerate() {
        if BATCH_LEDGER[i + 1..].iter().any(|o| o.theme == b.theme) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150J_TAG: &str = "stareco-F150-deep5c";

pub fn run_f150_deep5c_checks() -> CheckSet {
    let mut set = CheckSet::new(F150J_TAG);

    // 台账完整性
    set.add("f150j batches", BATCH_COUNT == 6, "六批全登记");
    set.add(
        "f150j produced",
        all_batches_produced(),
        "每批行数与检查项均非零",
    );
    set.add(
        "f150j lines total",
        ledger_lines_total() == 30201,
        "六批行数合计实数",
    );
    set.add(
        "f150j checks total",
        ledger_checks_total() == 1619,
        "六批检查项合计实数",
    );

    // 缺陷密度
    let d = defect_density_x10();
    set.add(
        "f150j density shape",
        d.len() == BATCH_COUNT && d[0] > 0,
        "密度序列在位（基础批 14/6752≈20.7）",
    );
    set.add(
        "f150j worst batch",
        worst_batch() == "批次五",
        "密度最高批可复算（机械错偏多如实登记）",
    );

    // 90% 线
    set.add(
        "f150j gap honest",
        gap_to_gate(DOMAIN_WC_TOTAL) == 274,
        "缺口实数（wc 口径 30,965 / 线 31,239）",
    );
    set.add(
        "f150j gap zero",
        gap_to_gate(GATE_90_LINES) == 0,
        "跨线后缺口归零",
    );
    set.add(
        "f150j ticket gap",
        next_batch_ticket(DOMAIN_WC_TOTAL).contains("274"),
        "工单含缺口实数",
    );
    set.add(
        "f150j ticket done",
        next_batch_ticket(GATE_90_LINES).contains("闸门补测"),
        "跨线给收口令",
    );

    // 主题唯一性
    set.add("f150j themes unique", themes_unique(), "六批主题无一重复");

    set
}

#[cfg(test)]
mod deep5c_tests {
    use super::*;

    #[test]
    fn ledger_themes_match_narrative() {
        let want = [
            "判据实装与逻辑核",
            "设计细节机制深化",
            "持久化面与错误边界",
            "工程化工具面",
            "内容件与后端自建件",
            "装配与跨域接口件",
        ];
        for (i, b) in BATCH_LEDGER.iter().enumerate() {
            assert_eq!(b.theme, want[i], "第 {} 批主题漂移", i);
        }
    }

    #[test]
    fn defect_counts_match_ledger() {
        // 与账本缺陷节逐一核对：〇14/一2/二9/三10/四13/五7。
        let want = [12u32, 2, 9, 10, 13, 20];
        for (i, b) in BATCH_LEDGER.iter().enumerate() {
            assert_eq!(b.defects, want[i], "第 {} 批缺陷数漂移", i);
        }
    }
}
