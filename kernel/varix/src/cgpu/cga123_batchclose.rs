//! cga123_batchclose — CGPU-F0123 A 域批次收口（124/160 检查点）。
//!
//! 判据原文（施工书锚点 CGPU-F0123）：A 域 124 项阶段收口：124 项自检
//! 入树核对、基准总表更新（F0109 混合基准并入）、缺陷账（🔴 零）、治理
//! 件完成度（F0111/114/115/118 四治理件状态）、阶段宣告（A 域 77.5%）。
//! 判据：124 项入树、总表更新、🔴 零、治理件状态、宣告。
//!
//! **设计定盘**：收口账五面：①入树核对（124 项编号集合入树完整性——
//! 缺号/重号即拒）；②基准总表（混合基准行并入，行指纹冻结）；③缺陷
//! 账（🔴 计数必须为零，🟡 须全部有归属波次）；④治理件状态四件套
//! （F0111/114/115/118 各自状态三态：Ready/Defer/Blocked，收口要求
//! 四件全 Ready）；⑤阶段宣告（77.5% = 124/160 的千分比 775，宣告前
//! 五面全过才可签）。任何一面不过即拒收，拒收带明确诊断码。
//!
//! 诊断码段 0x0100~0x010F 第 10 子区（0x0190~0x019F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA123_VERSION: &str = "cga123-batchclose-v1";

/// 诊断子区基值（0x01 段第 10 子区）。
pub const SEG_BASE: u16 = 0x0190;

/// A 域阶段收口项数（判据"124 项"）。
pub const BATCH_ITEMS: u32 = 124;

/// A 域总项数。
pub const DOMAIN_TOTAL: u32 = 160;

/// 阶段进度千分比（124/160 = 775‰）。
pub const PHASE_PERMILLE: u64 = (BATCH_ITEMS as u64 * 1000) / DOMAIN_TOTAL as u64;

/// 四治理件闭集（判据"治理件完成度"）。
pub const GOVERNANCE_IDS: [u32; 4] = [111, 114, 115, 118];

/// 治理件状态三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovState {
    /// 就绪。
    Ready,
    /// 押后（收口拒收）。
    Defer,
    /// 阻塞（收口拒收）。
    Blocked,
}

impl GovState {
    /// 是否收口就绪。
    pub fn ready(self) -> bool {
        matches!(self, GovState::Ready)
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            GovState::Ready => SEG_BASE + 1,
            GovState::Defer => SEG_BASE + 2,
            GovState::Blocked => SEG_BASE + 3,
        }
    }
}

/// 缺陷账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Defect {
    /// 缺陷编号。
    pub id: u32,
    /// 级别：true=🔴（阻断），false=🟡（须归属）。
    pub critical: bool,
    /// 归属波次（🟡 必须非零）。
    pub wave: u32,
}

/// 收口账。
pub struct BatchCloseLedger {
    /// 已入树编号集合。
    tree_items: Vec<u32>,
    /// 基准总表行指纹。
    bench_fps: Vec<u64>,
    /// 缺陷账。
    defects: Vec<Defect>,
    /// 治理件状态。
    gov: [Option<GovState>; 4],
    /// 宣告签章。
    declared: Option<u64>,
}

impl BatchCloseLedger {
    /// 空账。
    pub fn new() -> Self {
        BatchCloseLedger {
            tree_items: Vec::new(),
            bench_fps: Vec::new(),
            defects: Vec::new(),
            gov: [None; 4],
            declared: None,
        }
    }

    /// 入树登记（判据"124 项入树"：编号 1..=124 域内且不重号）。
    pub fn plant(&mut self, item: u32) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 4);
        }
        if item == 0 || item > BATCH_ITEMS {
            return Err(SEG_BASE + 5);
        }
        if self.tree_items.contains(&item) {
            return Err(SEG_BASE + 6);
        }
        self.tree_items.push(item);
        Ok(())
    }

    /// 入树完整性（判据"124 项入树"：编号 1..=124 全在且无缺号）。
    pub fn tree_complete(&self) -> Result<(), u16> {
        if self.tree_items.len() != BATCH_ITEMS as usize {
            return Err(SEG_BASE + 7);
        }
        let mut seen = [false; 125];
        for &i in &self.tree_items {
            seen[i as usize] = true;
        }
        for i in 1..=BATCH_ITEMS as usize {
            if !seen[i] {
                return Err(SEG_BASE + 8);
            }
        }
        Ok(())
    }

    /// 基准总表并入（判据"总表更新"：行指纹非零；重复行指纹即拒——
    /// 指纹相同说明重复并入同一行）。
    pub fn merge_bench_row(&mut self, row_fp: u64) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 4);
        }
        if row_fp == 0 {
            return Err(SEG_BASE + 9);
        }
        if self.bench_fps.contains(&row_fp) {
            return Err(SEG_BASE + 10);
        }
        self.bench_fps.push(row_fp);
        Ok(())
    }

    /// 缺陷登记（🔴 直接入账；🟡 无归属波次即拒）。
    pub fn report_defect(&mut self, id: u32, critical: bool, wave: u32) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 4);
        }
        if id == 0 || self.defects.iter().any(|d| d.id == id) {
            return Err(SEG_BASE + 11);
        }
        if !critical && wave == 0 {
            return Err(SEG_BASE + 12);
        }
        self.defects.push(Defect { id, critical, wave });
        Ok(())
    }

    /// 治理件状态登记（判据"治理件状态"：四件套逐件登记，件号必须
    /// 在治理件闭集内）。
    pub fn set_governance(&mut self, gid: u32, st: GovState) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 4);
        }
        let slot = GOVERNANCE_IDS
            .iter()
            .position(|&g| g == gid)
            .ok_or(SEG_BASE + 13)?;
        self.gov[slot] = Some(st);
        Ok(())
    }

    /// 🔴 计数。
    pub fn critical_count(&self) -> usize {
        self.defects.iter().filter(|d| d.critical).count()
    }

    /// 阶段宣告（判据"宣告"：五面全过才可签——入树完整、总表非空、
    /// 🔴 零、🟡 全有归属、四治理件全 Ready；签章 = 五面账 FNV）。
    pub fn declare(&mut self) -> Result<u64, u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 14);
        }
        self.tree_complete()?;
        if self.bench_fps.is_empty() {
            return Err(SEG_BASE + 15);
        }
        if self.critical_count() != 0 {
            return Err(SEG_BASE + 16);
        }
        for i in 0..4 {
            match self.gov[i] {
                None => return Err(SEG_BASE + 17),
                Some(st) if !st.ready() => return Err(st.code()),
                Some(_) => {}
            }
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for &i in &self.tree_items {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ i as u64;
        }
        for &f in &self.bench_fps {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ f;
        }
        self.declared = Some(fp);
        Ok(fp)
    }

    /// 阶段进度（千分比）。
    pub fn phase_permille(&self) -> u64 {
        PHASE_PERMILLE
    }

    /// 已入树数。
    pub fn planted(&self) -> usize {
        self.tree_items.len()
    }
}

impl Default for BatchCloseLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl BatchCloseLedger {
    /// 🟡 波次归属投影（判据"🟡 指派核验"的账面）：波次号升序的
    /// (波次, 计数) 列表。
    pub fn wave_census(&self) -> Vec<(u32, usize)> {
        let mut out: Vec<(u32, usize)> = Vec::new();
        for d in &self.defects {
            if d.critical {
                continue;
            }
            match out.iter_mut().find(|(w, _)| *w == d.wave) {
                Some((_, c)) => *c += 1,
                None => out.push((d.wave, 1)),
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// 无归属 🟡 计数（波次为零的 🟡——入账时已拦，此处为投影兜底）。
    pub fn unassigned_yellow(&self) -> usize {
        self.defects
            .iter()
            .filter(|d| !d.critical && d.wave == 0)
            .count()
    }

    /// 治理件状态快照（判据"治理件状态"的投影面）：
    /// 与 GOVERNANCE_IDS 对齐的四元组。
    pub fn governance_snapshot(&self) -> [Option<GovState>; 4] {
        self.gov
    }

    /// 拒收诊断聚合（判据"宣告"的拒收面）：逐面检查，收集全部
    /// 失败面的诊断码（面全过返回空表——与 declare 的首错即停互补，
    /// 本 API 给收口会话一次看全所有欠账）。
    pub fn rejection_report(&self) -> Vec<u16> {
        let mut out: Vec<u16> = Vec::new();
        if self.tree_complete().is_err() {
            out.push(SEG_BASE + 7);
        }
        if self.bench_fps.is_empty() {
            out.push(SEG_BASE + 15);
        }
        if self.critical_count() != 0 {
            out.push(SEG_BASE + 16);
        }
        for i in 0..4 {
            match self.gov[i] {
                None => out.push(SEG_BASE + 17),
                Some(st) if !st.ready() => out.push(st.code()),
                Some(_) => {}
            }
        }
        out
    }

    /// 基准行数。
    pub fn bench_rows(&self) -> usize {
        self.bench_fps.len()
    }

    /// 缺陷总数。
    pub fn defect_count(&self) -> usize {
        self.defects.len()
    }

    /// 🟡 全归属核验（🟡 逐条波次非零）。
    pub fn all_yellow_assigned(&self) -> bool {
        self.defects
            .iter()
            .filter(|d| !d.critical)
            .all(|d| d.wave != 0)
    }
}
