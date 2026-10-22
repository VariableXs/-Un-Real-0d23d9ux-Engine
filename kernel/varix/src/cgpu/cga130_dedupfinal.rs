//! cga130_dedupfinal — CGPU-F0130 A 域查重终审。
//!
//! 判据原文（施工书锚点 CGPU-F0130）：A 域 160 项查重终审（F0387 范式）：
//! 与全仓 + 与 VE 域的矩阵比对（A 域风险点：SIMD 算子与 VE 域效果实现的
//! 语义重叠——分工声明复核：CGPU 管算力实现、VE 管视觉语义），抽检 20%，
//! 三态处置。判据：矩阵比对、抽检、三态、报告、证据。
//!
//! **设计定盘**：查重账：比对矩阵（A 域项 × 外部域项）的相似度分桶
/// （整数千分比相似度），三态处置闭集：Distinct（不同——放行）、
/// OverlapDeclared（重叠但已声明分工——放行 + 分工声明引用）、
/// Duplicate（重复——立案处置）。抽检 20% = 至少抽总量的 20% 且
/// 不少于 32 项（160 的 20%）。分工声明 =（A 域项, VE 项）对的声明
/// 登记表——已声明的重叠对进 OverlapDeclared 态，未声明的重叠对进
/// Duplicate 态。报告 = 三态计数 + 立案清单的封账指纹。
///
/// 诊断码段 0x0210~0x021F（0x02 段第 1 子区）。
/// no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA130_VERSION: &str = "cga130-dedupfinal-v1";

/// 诊断子区基值（0x02 段第 1 子区）。
pub const SEG_BASE: u16 = 0x0210;

/// A 域总量（判据"160 项"）。
pub const DOMAIN_ITEMS: u32 = 160;

/// 抽检下限（20%，整除向上取整）。
pub const SAMPLE_MIN: u32 = (DOMAIN_ITEMS + 4) / 5;

/// 相似度分桶线：≥ 800‰ 视为重叠候选。
pub const OVERLAP_PERMILLE: u64 = 800;

/// 三态处置闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedupVerdict {
    /// 不同（放行）。
    Distinct,
    /// 重叠已声明（放行 + 分工声明引用）。
    OverlapDeclared,
    /// 重复（立案）。
    Duplicate,
}

impl DedupVerdict {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            DedupVerdict::Distinct => SEG_BASE + 1,
            DedupVerdict::OverlapDeclared => SEG_BASE + 2,
            DedupVerdict::Duplicate => SEG_BASE + 3,
        }
    }
}

/// 一条比对记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompareRow {
    /// A 域项号。
    pub a_item: u32,
    /// 外部域标记（0=全仓内，1=VE 域）。
    pub external: u8,
    /// 外部项号。
    pub ext_item: u32,
    /// 相似度（千分比）。
    pub similarity_permille: u64,
    /// 判定。
    pub verdict: DedupVerdict,
}

/// 查重终审账。
pub struct DedupLedger {
    /// 比对记录。
    rows: Vec<CompareRow>,
    /// 分工声明表（(A 项, 外部项) 对）。
    declared: Vec<(u32, u32)>,
    /// 立案清单。
    cases: Vec<u32>,
    /// 封账。
    sealed: Option<u64>,
}

impl DedupLedger {
    /// 空账。
    pub fn new() -> Self {
        DedupLedger {
            rows: Vec::new(),
            declared: Vec::new(),
            cases: Vec::new(),
            sealed: None,
        }
    }

    /// 分工声明登记（判据"分工声明复核"：对重复即拒）。
    pub fn declare_division(&mut self, a_item: u32, ext_item: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if a_item == 0 || a_item > DOMAIN_ITEMS {
            return Err(SEG_BASE + 5);
        }
        if self
            .declared
            .iter()
            .any(|&(a, e)| a == a_item && e == ext_item)
        {
            return Err(SEG_BASE + 6);
        }
        self.declared.push((a_item, ext_item));
        Ok(())
    }

    /// 比对入账（判据"矩阵比对"：同一对重复比对即拒；相似度
    /// 超线且已声明 → OverlapDeclared，超线未声明 → Duplicate 并立案）。
    pub fn compare(
        &mut self,
        a_item: u32,
        external: u8,
        ext_item: u32,
        similarity_permille: u64,
    ) -> Result<DedupVerdict, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if a_item == 0 || a_item > DOMAIN_ITEMS {
            return Err(SEG_BASE + 5);
        }
        if external > 1 {
            return Err(SEG_BASE + 7);
        }
        if similarity_permille > 1000 {
            return Err(SEG_BASE + 8);
        }
        if self
            .rows
            .iter()
            .any(|r| r.a_item == a_item && r.external == external && r.ext_item == ext_item)
        {
            return Err(SEG_BASE + 9);
        }
        let declared = self
            .declared
            .iter()
            .any(|&(a, e)| a == a_item && e == ext_item);
        let verdict = if similarity_permille < OVERLAP_PERMILLE {
            DedupVerdict::Distinct
        } else if declared {
            DedupVerdict::OverlapDeclared
        } else {
            self.cases.push(a_item);
            DedupVerdict::Duplicate
        };
        self.rows.push(CompareRow {
            a_item,
            external,
            ext_item,
            similarity_permille,
            verdict,
        });
        Ok(verdict)
    }

    /// 抽检核验（判据"抽检 20%"：已比对的不同 A 项数 ≥ 32）。
    pub fn sample_ok(&self) -> Result<(), u16> {
        let mut seen: Vec<u32> = Vec::new();
        for r in &self.rows {
            if !seen.contains(&r.a_item) {
                seen.push(r.a_item);
            }
        }
        if (seen.len() as u32) < SAMPLE_MIN {
            return Err(SEG_BASE + 10);
        }
        Ok(())
    }

    /// 立案数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 立案查询。
    pub fn case(&self, idx: usize) -> Option<u32> {
        self.cases.get(idx).copied()
    }

    /// 三态计数。
    pub fn verdict_counts(&self) -> (usize, usize, usize) {
        let d = self
            .rows
            .iter()
            .filter(|r| r.verdict == DedupVerdict::Distinct)
            .count();
        let o = self
            .rows
            .iter()
            .filter(|r| r.verdict == DedupVerdict::OverlapDeclared)
            .count();
        let u = self
            .rows
            .iter()
            .filter(|r| r.verdict == DedupVerdict::Duplicate)
            .count();
        (d, o, u)
    }

    /// 封账（判据"报告、证据"：抽检达标才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 11);
        }
        self.sample_ok()?;
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for r in &self.rows {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ (r.a_item as u64)
                ^ (r.verdict as u64) << 24;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for DedupLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 相似度分档（判据"三态处置"的量化面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimBand {
    /// 低相似（< 500‰）。
    Low,
    /// 中相似（500‰ ~ 超线之间）。
    Mid,
    /// 高相似（≥ 超线）。
    High,
}

/// 相似度分档判定。
pub fn sim_band(sim_permille: u64) -> SimBand {
    if sim_permille < 500 {
        SimBand::Low
    } else if sim_permille < OVERLAP_PERMILLE {
        SimBand::Mid
    } else {
        SimBand::High
    }
}

impl DedupLedger {
    /// 声明对数（判据"分工声明复核"的账面）。
    pub fn declared_count(&self) -> usize {
        self.declared.len()
    }

    /// 相似度均值（千分比；空账 None）。
    pub fn avg_similarity_permille(&self) -> Option<u64> {
        if self.rows.is_empty() {
            return None;
        }
        let sum: u64 = self.rows.iter().map(|r| r.similarity_permille).sum();
        Some(sum / self.rows.len() as u64)
    }

    /// 分档计数（三档各多少对）。
    pub fn band_census(&self) -> (usize, usize, usize) {
        let mut low = 0;
        let mut mid = 0;
        let mut high = 0;
        for r in &self.rows {
            match sim_band(r.similarity_permille) {
                SimBand::Low => low += 1,
                SimBand::Mid => mid += 1,
                SimBand::High => high += 1,
            }
        }
        (low, mid, high)
    }

    /// 已覆盖 A 项数（判据"新增项全覆盖"的进度面）。
    pub fn covered_a_items(&self) -> usize {
        let mut seen: Vec<u32> = Vec::new();
        for r in rows_of(self).iter() {
            if !seen.contains(&r.a_item) {
                seen.push(r.a_item);
            }
        }
        seen.len()
    }

    /// 与 VE 域交叉比对对数（判据"与 VE 域新增域的交叉比对"）：
    /// external 通道 1 的行数。
    pub fn ve_cross_rows(&self) -> usize {
        self.rows.iter().filter(|r| r.external == 1).count()
    }

    /// 立案核销（判据"三态处置"的闭环面：Duplicate 立案经处置
    /// （拆分/声明补登/豁免 ADR）后核销；未立案即拒）。
    pub fn resolve_case(&mut self, a_item: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        let pos = match self.cases.iter().position(|&c| c == a_item) {
            Some(p) => p,
            None => return Err(SEG_BASE + 12),
        };
        self.cases.remove(pos);
        Ok(())
    }

    /// 未决立案核验（判据"报告"的闭环前提：封账前立案必须清零）。
    pub fn cases_clear(&self) -> bool {
        self.cases.is_empty()
    }

    /// 封账指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }
}

/// 只读行访问（覆盖面辅助：避免 DedupLedger 字段外泄的同时给
/// 投影面提供行迭代）。
pub fn rows_of(l: &DedupLedger) -> &Vec<CompareRow> {
    &l.rows
}
