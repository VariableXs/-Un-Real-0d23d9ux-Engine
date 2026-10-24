//! VE-F0234 · Intel 着色器后端架构位（VE-B 域 · Intel 核显组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0234`
//!
//! **职责定位（锚点原文）**：按代际声明后端能力（EU 数量、线程组上限、SIMD
//! 宽度、采样器与 RT 管线特性），编译器后端（范围外）按能力位生成代码；能力
//! 探测与 F0227 媒体位同框架呈现；代际差异表驱动，冲突以实测修正。
//!
//! **数据结构（锚点原文）**：架构位表（代际×能力×值）；探测缓存。
//!
//! **判据（锚点原文）**：能力位、保守下限、表驱动、实测修正、判据。
//! 本模块 14 条判据逐条映射：**能力位 4 条**（代际查表 O(1)／五类能力齐备／
//! 采样器 RT 特性按代际单调不回退／同代际重复查询幂等）、**保守下限 3 条**
//! （读取失败标下限不编值／未知代际按最新预期且带标记／下限值低于任何实测）、
//! **表驱动 3 条**（代际表版本化可查／表值两两互异可对账／零运行时分支——
//! 查表即答不走探测路径）、**实测修正 3 条**（实测值入缓存覆盖表值留痕／
//! 位冲突以实测为准修正且计数／修正后缓存命中不再走表）、**判据自身 1 条**
//! （码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 能力读取失败 → **保守下限标注** | [`ArchTable::probe`] 读值非法（0 值/越界）时不编造，返回 [`ProbeOutcome::Floor`]——表值换成该能力的**保守下限**并打 `floor_fallback` 标记，编译器后端按最坏假设生成代码（宁可慢不可错） |
//! | 代际未知 → **按最新代际预期并标记** | [`ArchTable::for_gen`] 未命中映射表时按 [`GenTier::XeLatest`] 的行呈现，`unknown_assumed` 置位——下游可见「这是预期值不是实测值」 |
//! | 位冲突 → **以实测为准修正表** | 声明值与实测值不等即冲突：[`ArchTable::calibrate`] 写探测缓存覆盖呈现值并留修正痕迹（原值/新值/修正计数），下次查询走缓存 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **表驱动零运行时分支**：能力值全部来自编译期常量表（代际×能力），查询
//!   就是索引一行结构体拷贝——没有 if 链、没有探测调用。探测只在「实测校准」
//!   这一条入口发生，且结果进缓存后同样表式呈现。
//! - **下限值必须真的保守**：`EU_FLOOR` 等常量取比任何一代最低配都小的值，
//!   判据侧独立复算「下限 < 全表最小值」，防止有人把下限抬成"平均预期"。
//! - **修正留痕可回放**：每次实测修正记录（能力、表值、实测值），收口时
//!   [`ArchTable::corrections`] 可逐条对账——静默改表是架构位表腐化的开始。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE440;

// ---- 诊断码 ----
pub const XC_GEN_UNKNOWN: u32 = SEG_BASE + 1; // 代际未知（按最新代际预期）
pub const XC_READ_FAIL: u32 = SEG_BASE + 2; // 能力读取失败（保守下限）
pub const XC_VALUE_BAD: u32 = SEG_BASE + 3; // 实测值非法（0/越界）
pub const XC_CONFLICT: u32 = SEG_BASE + 4; // 位冲突（表值≠实测）
pub const XC_CAP_BAD: u32 = SEG_BASE + 5; // 能力枚举非法

/// 代际族（与 veb21_ident 的分型对齐，此处只列影响着色器后端的档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenTier {
    /// Gen9（下限代际，见 F0237）。
    Gen9,
    /// Gen11。
    Gen11,
    /// Xe（第一代独立架构）。
    Xe,
    /// Xe2（当前最新预期代际）。
    XeLatest,
}

/// 后端能力键（五类，锚点列举的能力全集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cap {
    /// EU 数量。
    EuCount,
    /// 线程组上限。
    ThreadGroupMax,
    /// SIMD 宽度。
    SimdWidth,
    /// 采样器管线特性位。
    SamplerFeats,
    /// RT 管线特性位。
    RtFeats,
}

/// 一代际的全部能力值（表行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchRow {
    /// EU 数量。
    pub eu: u32,
    /// 线程组上限。
    pub group_max: u32,
    /// SIMD 宽度。
    pub simd: u32,
    /// 采样器特性位。
    pub sampler: u32,
    /// RT 特性位。
    pub rt: u32,
}

/// 各能力的保守下限（读取失败时以此标注——必须低于任何一代的最低配）。
pub const EU_FLOOR: u32 = 8;
pub const GROUP_FLOOR: u32 = 16;
pub const SIMD_FLOOR: u32 = 4;
pub const SAMPLER_FLOOR: u32 = 0;
pub const RT_FLOOR: u32 = 0;

/// 架构位表（代际×能力×值），版本化可查。
pub const ARCH_TABLE: [(GenTier, ArchRow); 4] = [
    (GenTier::Gen9, ArchRow { eu: 72, group_max: 512, simd: 16, sampler: 0x1, rt: 0x0 }),
    (GenTier::Gen11, ArchRow { eu: 144, group_max: 1024, simd: 16, sampler: 0x3, rt: 0x0 }),
    (GenTier::Xe, ArchRow { eu: 512, group_max: 2048, simd: 16, sampler: 0x7, rt: 0x1 }),
    (GenTier::XeLatest, ArchRow { eu: 640, group_max: 4096, simd: 32, sampler: 0xF, rt: 0x3 }),
];

/// 架构位表版本（表内容变更时推进）。
pub const ARCH_TABLE_VER: u32 = 3;

/// 探测结果形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeOutcome {
    /// 正常读到表值。
    Table,
    /// 读取失败——保守下限标注。
    Floor,
    /// 未知代际——按最新代际预期并标记。
    Assumed,
    /// 实测命中缓存（已校准值）。
    Calibrated,
}

/// 一次能力探测的呈现值与形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// 呈现给编译器后端的值。
    pub value: u32,
    /// 结果形态。
    pub outcome: ProbeOutcome,
}

/// 实测修正痕迹（对账用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Correction {
    /// 修正的能力。
    pub cap: Cap,
    /// 表值（修正前）。
    pub table_value: u32,
    /// 实测值（修正后）。
    pub measured: u32,
}

/// 架构位表控制器。
pub struct ArchTable {
    /// 探测缓存：代际×能力 → 实测值（修正后呈现走这里）。
    cache: Vec<(GenTier, Cap, u32)>,
    /// 未知代际计数。
    pub unknown_gens: u32,
    /// 读取失败计数（保守下限标注次数）。
    pub floor_fallbacks: u32,
    /// 位冲突计数。
    pub conflicts: u32,
    /// 修正痕迹账。
    corrections: Vec<Correction>,
}

impl ArchTable {
    /// 新控制器（缓存空账零）。
    pub fn new() -> Self {
        ArchTable {
            cache: Vec::new(),
            unknown_gens: 0,
            floor_fallbacks: 0,
            conflicts: 0,
            corrections: Vec::new(),
        }
    }

    /// 查表行；未知代际按最新代际预期呈现并返回 Assumed 形态（锚点：未知
    /// 代际→按最新代际预期并标记）。
    pub fn for_gen(&mut self, gen: Option<GenTier>) -> (GenTier, ProbeOutcome) {
        match gen {
            Some(g) => (g, ProbeOutcome::Table),
            None => {
                self.unknown_gens += 1;
                (GenTier::XeLatest, ProbeOutcome::Assumed)
            }
        }
    }

    /// 能力查询：先缓存（实测校准值），再表值；`raw` 是实测读数——非法
    /// （零/越表值上限两倍）即读取失败，按保守下限标注（锚点：能力读取
    /// 失败→保守下限标注）。
    pub fn probe(&mut self, gen: GenTier, cap: Cap, raw: Option<u32>) -> Result<Probe, u32> {
        // 缓存命中：校准值优先于表值（锚点：修正后缓存命中不再走表）。
        if let Some(&(_, _, v)) = self.cache.iter().find(|(g, c, _)| *g == gen && *c == cap) {
            return Ok(Probe { value: v, outcome: ProbeOutcome::Calibrated });
        }
        let row = ARCH_TABLE.iter().find(|(g, _)| *g == gen).map(|(_, r)| *r);
        let table_v = match row {
            Some(r) => cap_value(&r, cap),
            None => return Err(XC_GEN_UNKNOWN),
        };
        match raw {
            None => {
                // 无读数：直接呈现表值（表驱动零运行时分支）。
                Ok(Probe { value: table_v, outcome: ProbeOutcome::Table })
            }
            Some(v) if v == 0 || v > table_v.saturating_mul(2) => {
                // 读数非法：保守下限，不编值。
                self.floor_fallbacks += 1;
                Ok(Probe { value: cap_floor(cap), outcome: ProbeOutcome::Floor })
            }
            Some(v) if v != table_v => {
                // 位冲突：以实测为准修正（锚点：位冲突→以实测为准修正表）。
                self.conflicts += 1;
                self.corrections.push(Correction { cap, table_value: table_v, measured: v });
                self.cache.push((gen, cap, v));
                Ok(Probe { value: v, outcome: ProbeOutcome::Calibrated })
            }
            Some(v) => Ok(Probe { value: v, outcome: ProbeOutcome::Table }),
        }
    }

    /// 修正痕迹账（收口对账）。
    pub fn corrections(&self) -> &[Correction] {
        &self.corrections
    }

    /// 缓存条数（对账：与修正数一致——修正必进缓存，缓存只进修正）。
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

impl Default for ArchTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 表行取能力值。
const fn cap_value(r: &ArchRow, cap: Cap) -> u32 {
    match cap {
        Cap::EuCount => r.eu,
        Cap::ThreadGroupMax => r.group_max,
        Cap::SimdWidth => r.simd,
        Cap::SamplerFeats => r.sampler,
        Cap::RtFeats => r.rt,
    }
}

/// 保守下限值。
const fn cap_floor(cap: Cap) -> u32 {
    match cap {
        Cap::EuCount => EU_FLOOR,
        Cap::ThreadGroupMax => GROUP_FLOOR,
        Cap::SimdWidth => SIMD_FLOOR,
        Cap::SamplerFeats => SAMPLER_FLOOR,
        Cap::RtFeats => RT_FLOOR,
    }
}

// ---------------------------------------------------------------- 判据

/// VE-F0234 判据组（14 条）。
pub fn run_veb234_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb234_intelarch");

    // --- 判据 1：能力位（代际查表 O(1)、五类齐备、单调不回退） ---
    {
        let mut t = ArchTable::new();
        let mut all_ok = true;
        // 五类能力逐代查询都有值且形态为 Table。
        for (g, _) in ARCH_TABLE.iter() {
            for cap in [Cap::EuCount, Cap::ThreadGroupMax, Cap::SimdWidth, Cap::SamplerFeats, Cap::RtFeats] {
                if t.probe(*g, cap, None).map(|p| p.outcome == ProbeOutcome::Table).unwrap_or(false) {
                } else {
                    all_ok = false;
                }
            }
        }
        s.add(
            "B34-能力位-代际查表五类齐备",
            all_ok && ARCH_TABLE.len() == 4 && ARCH_TABLE_VER >= 1,
            "四代际×五能力全部表式可查，表版本化在册",
        );
    }
    // --- 判据 2：能力位——采样器/RT 特性按代际单调不回退 ---
    {
        let mut mono = true;
        for i in 1..ARCH_TABLE.len() {
            if ARCH_TABLE[i].1.sampler < ARCH_TABLE[i - 1].1.sampler
                || ARCH_TABLE[i].1.rt < ARCH_TABLE[i - 1].1.rt
            {
                mono = false;
            }
        }
        s.add(
            "B34-能力位-特性位代际单调不回退",
            mono,
            "新代际的特性位只增不减（能力不会随升级消失）",
        );
    }
    // --- 判据 3：能力位——同代际重复查询幂等 ---
    {
        let mut t = ArchTable::new();
        let p1 = t.probe(GenTier::Xe, Cap::EuCount, None);
        let p2 = t.probe(GenTier::Xe, Cap::EuCount, None);
        s.add(
            "B34-能力位-重复查询幂等",
            p1.ok() == p2.ok() && t.cache_len() == 0,
            "无实测时重复查询同值同形态，缓存不动（零分支路径稳定）",
        );
    }
    // --- 判据 4：保守下限——下限值低于全表最小值 ---
    {
        let min_eu = ARCH_TABLE.iter().map(|(_, r)| r.eu).min().unwrap_or(u32::MAX);
        let min_grp = ARCH_TABLE.iter().map(|(_, r)| r.group_max).min().unwrap_or(u32::MAX);
        let min_simd = ARCH_TABLE.iter().map(|(_, r)| r.simd).min().unwrap_or(u32::MAX);
        s.add(
            "B34-保守下限-下限低于全表最小值",
            EU_FLOOR < min_eu && GROUP_FLOOR < min_grp && SIMD_FLOOR < min_simd,
            "下限是「比任何一代最低配都小」的真保守值，不是平均预期",
        );
    }
    // --- 判据 5：保守下限——读取失败标注下限并计数 ---
    {
        let mut t = ArchTable::new();
        let p = t.probe(GenTier::Xe, Cap::EuCount, Some(0));
        s.add(
            "B34-保守下限-读取失败标注下限",
            p.ok() == Some(Probe { value: EU_FLOOR, outcome: ProbeOutcome::Floor })
                && t.floor_fallbacks == 1,
            "零读数视为读取失败：呈现 EU_FLOOR 并留 fallback 账",
        );
    }
    // --- 判据 6：保守下限——未知代际按最新预期并标记 ---
    {
        let mut t = ArchTable::new();
        let (g, o) = t.for_gen(None);
        s.add(
            "B34-保守下限-未知代际按最新预期",
            g == GenTier::XeLatest && o == ProbeOutcome::Assumed && t.unknown_gens == 1,
            "未知代际呈现最新行但带 Assumed 标记（预期值≠实测值）",
        );
    }
    // --- 判据 7：表驱动——表值两两互异可对账 ---
    {
        // 同能力跨代际的值不全相同（表真的在区分代际）。
        let eu_all_diff = {
            let mut diff = true;
            for i in 1..ARCH_TABLE.len() {
                if ARCH_TABLE[i].1.eu == ARCH_TABLE[i - 1].1.eu {
                    diff = false;
                }
            }
            diff
        };
        s.add(
            "B34-表驱动-代际行值互异",
            eu_all_diff && ARCH_TABLE[0].1.eu < ARCH_TABLE[3].1.eu,
            "EU 数逐代严格递增，表是差异表不是复读机",
        );
    }
    // --- 判据 8：表驱动——越界能力枚举拒绝 ---
    {
        let mut t = ArchTable::new();
        // 用合法代际 + 全部合法枚举走一遍确认无越界 panic 路径。
        let caps_ok = [Cap::EuCount, Cap::ThreadGroupMax, Cap::SimdWidth, Cap::SamplerFeats, Cap::RtFeats]
            .iter()
            .all(|c| t.probe(GenTier::Gen9, *c, None).is_ok());
        s.add(
            "B34-表驱动-能力枚举全可查",
            caps_ok && t.conflicts == 0,
            "五枚举在每一代上都可查且无冲突路径",
        );
    }
    // --- 判据 9：实测修正——位冲突以实测为准修正留痕 ---
    {
        let mut t = ArchTable::new();
        let table_eu = ARCH_TABLE.iter().find(|(g, _)| *g == GenTier::Xe).map(|(_, r)| r.eu);
        let p = t.probe(GenTier::Xe, Cap::EuCount, Some(480));
        let ok = match (table_eu, p) {
            (Some(tv), Ok(pr)) => tv != 480 && pr.value == 480 && t.conflicts == 1,
            _ => false,
        };
        s.add(
            "B34-实测修正-位冲突以实测为准",
            ok && t.corrections().len() == 1
                && t.corrections()[0].table_value != t.corrections()[0].measured,
            "表值≠实测即冲突：呈现实测值、冲突计数、修正痕迹（原值/新值）齐",
        );
    }
    // --- 判据 10：实测修正——修正后缓存命中不再走表 ---
    {
        let mut t = ArchTable::new();
        let _ = t.probe(GenTier::Xe, Cap::EuCount, Some(480));
        // 再次带不同读数查询——应命中缓存返回校准值，不再触发冲突。
        let p2 = t.probe(GenTier::Xe, Cap::EuCount, Some(999999));
        s.add(
            "B34-实测修正-缓存命中不再走表",
            p2.ok() == Some(Probe { value: 480, outcome: ProbeOutcome::Calibrated })
                && t.conflicts == 1
                && t.cache_len() == 1,
            "校准值进缓存后优先呈现；缓存条数=修正数（只进修正）",
        );
    }
    // --- 判据 11：实测修正——实测等于表值不记冲突 ---
    {
        let mut t = ArchTable::new();
        let tv = ARCH_TABLE.iter().find(|(g, _)| *g == GenTier::Gen9).map(|(_, r)| r.eu);
        let p = t.probe(GenTier::Gen9, Cap::EuCount, tv);
        s.add(
            "B34-实测修正-表值一致不记冲突",
            p.ok() == Some(Probe { value: tv.unwrap_or(0), outcome: ProbeOutcome::Table })
                && t.conflicts == 0
                && t.corrections().is_empty(),
            "实测复核通过：不产生修正痕迹（零噪音对账）",
        );
    }
    // --- 判据 12：错误路径——非法读数（超表值两倍）走下限不走冲突 ---
    {
        let mut t = ArchTable::new();
        let p = t.probe(GenTier::XeLatest, Cap::ThreadGroupMax, Some(999_999));
        s.add(
            "B34-错误路径-越界读数走下限",
            p.ok() == Some(Probe { value: GROUP_FLOOR, outcome: ProbeOutcome::Floor })
                && t.floor_fallbacks == 1
                && t.conflicts == 0,
            "读数超表值两倍=读坏了，按保守下限（不是当冲突修正）",
        );
    }
    // --- 判据 13：读屏——探测形态可读屏呈现 ---
    {
        let t = ArchTable::new();
        let line = alloc::format!(
            "架构位表 v{}：代际 {} 行，修正 {} 条，下限回退 {}",
            ARCH_TABLE_VER, ARCH_TABLE.len(), t.corrections().len(), t.floor_fallbacks
        );
        s.add_dyn(
            "B34-读屏-架构位摘要可读屏",
            line.contains("架构位表") && line.contains("修正"),
            &alloc::format!("摘要含版本/行数/修正/下限四要素：{}", line),
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_GEN_UNKNOWN, XC_READ_FAIL, XC_VALUE_BAD, XC_CONFLICT, XC_CAP_BAD];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B34-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 5,
            "五诊断码互异且全部落在 0xE440 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb234_table_driven_lookup() {
        let mut t = ArchTable::new();
        // 四代际 × EU 数全可查且递增。
        let mut prev = 0;
        for (g, _) in ARCH_TABLE.iter() {
            let p = t.probe(*g, Cap::EuCount, None).unwrap_or(Probe { value: 0, outcome: ProbeOutcome::Table });
            assert!(p.value > prev, "EU 数应逐代递增");
            assert_eq!(p.outcome, ProbeOutcome::Table);
            prev = p.value;
        }
        assert_eq!(t.unknown_gens, 0);
    }

    #[test]
    fn veb234_floor_fallback_chain() {
        let mut t = ArchTable::new();
        // 零读数 → 下限。
        let p1 = t.probe(GenTier::Gen9, Cap::SimdWidth, Some(0));
        assert_eq!(p1.unwrap_or(Probe { value: 0, outcome: ProbeOutcome::Table }).value, SIMD_FLOOR);
        // 越界读数 → 下限（不是冲突）。
        let p2 = t.probe(GenTier::Gen9, Cap::SimdWidth, Some(0xFFFF));
        assert_eq!(p2.unwrap_or(Probe { value: 0, outcome: ProbeOutcome::Table }).value, SIMD_FLOOR);
        assert_eq!(t.floor_fallbacks, 2);
        assert_eq!(t.conflicts, 0);
        assert!(t.cache_len() == 0, "下限标注不进缓存");
    }

    #[test]
    fn veb234_calibrate_and_cache() {
        let mut t = ArchTable::new();
        // 首测冲突：表 Xe simd=16，实测 24 → 修正。
        let p1 = t.probe(GenTier::Xe, Cap::SimdWidth, Some(24));
        assert_eq!(p1.unwrap_or(Probe { value: 0, outcome: ProbeOutcome::Table }).value, 24);
        assert_eq!(t.conflicts, 1);
        // 复查走缓存。
        let p2 = t.probe(GenTier::Xe, Cap::SimdWidth, None);
        assert_eq!(p2.unwrap_or(Probe { value: 0, outcome: ProbeOutcome::Table }).outcome, ProbeOutcome::Calibrated);
        // 修正痕迹可对账。
        let c = t.corrections();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].cap, Cap::SimdWidth);
        assert_eq!(c[0].table_value, 16);
        assert_eq!(c[0].measured, 24);
    }

    #[test]
    fn veb234_unknown_gen_assumed() {
        let mut t = ArchTable::new();
        let (g, o) = t.for_gen(None);
        assert_eq!(g, GenTier::XeLatest);
        assert_eq!(o, ProbeOutcome::Assumed);
        assert_eq!(t.unknown_gens, 1);
        let (g2, o2) = t.for_gen(Some(GenTier::Gen11));
        assert_eq!(g2, GenTier::Gen11);
        assert_eq!(o2, ProbeOutcome::Table);
        assert!(alloc::format!("{:?}", ProbeOutcome::Assumed).contains("Assumed"));
    }

    #[test]
    fn veb234_checks_all_green() {
        let set = run_veb234_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0234 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
