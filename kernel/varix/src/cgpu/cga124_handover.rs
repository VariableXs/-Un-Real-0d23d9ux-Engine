//! cga124_handover — CGPU-F0124 A 域剩余收尾与 B 域组移交。
//!
//! 判据原文（施工书锚点 CGPU-F0124）：A 域剩余收尾与 B 域（任务图调度）
//! 的移交：A 域移交包（SIMD 算子清单冻结版/线程池接口冻结/任务图对接
//! 接口 F0042 已冻的确认/剩余 36 项的收尾计划——A 域主体完成后以治理
//! 与收尾为主），B 域开工验证（任务图调度第一批次引用移交包开工）。
//! 判据：移交包四件、B 域开工验证、剩余计划、纪要归档、台账更新。
//!
//! **设计定盘**：移交包四件闭集（算子清单冻结/线程池接口冻结/对接接口
//! 确认/收尾计划），四件各有指纹，包指纹 = 四件指纹链 FNV。B 域开工
//! 验证 = B 域首批内容对移交包的引用登记（引用了包内哪件、引用指纹
//! 是否与包一致——引用指纹不符即开工包版本漂移，拒绝开工）。剩余计划
//! = 36 项收尾计划的条目账（逐条有负责人域与完成判据）。纪要归档 =
//! 移交纪要指纹落账；台账更新 = 包指纹写入移交台账（可追溯）。
//!
//! 诊断码段 0x01A0~0x01AF（0x01 段第 11 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA124_VERSION: &str = "cga124-handover-v1";

/// 诊断子区基值（0x01 段第 11 子区）。
pub const SEG_BASE: u16 = 0x01A0;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 移交包四件闭集序号。
pub const PACKAGE_PARTS: usize = 4;

/// 剩余收尾项数（判据"剩余 36 项"）。
pub const REMAINING_ITEMS: u32 = 36;

/// 移交包。
pub struct HandoverPackage {
    /// 四件指纹（序 0..4：算子清单/线程池接口/对接确认/收尾计划）。
    parts: [Option<u64>; PACKAGE_PARTS],
    /// 剩余收尾计划条目（编号 1..=36）。
    plan_items: Vec<u32>,
    /// 包指纹（四件齐后冻结）。
    package_fp: Option<u64>,
}

impl HandoverPackage {
    /// 空包。
    pub fn new() -> Self {
        HandoverPackage {
            parts: [None; PACKAGE_PARTS],
            plan_items: Vec::new(),
            package_fp: None,
        }
    }

    /// 装件（判据"移交包四件"：件号 0..4；冻结后禁改）。
    pub fn put_part(&mut self, part: usize, body: &[u8]) -> Result<(), u16> {
        if self.package_fp.is_some() {
            return Err(SEG_BASE + 1);
        }
        if part >= PACKAGE_PARTS {
            return Err(SEG_BASE + 2);
        }
        if body.is_empty() {
            return Err(SEG_BASE + 3);
        }
        if self.parts[part].is_some() {
            return Err(SEG_BASE + 4);
        }
        self.parts[part] = Some(fnv1a64(body));
        Ok(())
    }

    /// 登记收尾计划项（判据"剩余计划"：1..=36 且不重号）。
    pub fn add_plan_item(&mut self, item: u32) -> Result<(), u16> {
        if self.package_fp.is_some() {
            return Err(SEG_BASE + 1);
        }
        if item == 0 || item > REMAINING_ITEMS {
            return Err(SEG_BASE + 5);
        }
        if self.plan_items.contains(&item) {
            return Err(SEG_BASE + 6);
        }
        self.plan_items.push(item);
        Ok(())
    }

    /// 冻结包（判据"移交包四件"：四件齐 + 36 项计划齐才可冻结）。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.package_fp.is_some() {
            return Err(SEG_BASE + 7);
        }
        for p in &self.parts {
            if p.is_none() {
                return Err(SEG_BASE + 8);
            }
        }
        if self.plan_items.len() != REMAINING_ITEMS as usize {
            return Err(SEG_BASE + 9);
        }
        let mut fp = FNV_OFFSET;
        for p in &self.parts {
            fp = fp.wrapping_mul(FNV_PRIME) ^ p.unwrap_or(0);
        }
        self.package_fp = Some(fp);
        Ok(fp)
    }

    /// 包指纹。
    pub fn fingerprint(&self) -> Option<u64> {
        self.package_fp
    }
}

/// B 域开工验证账（判据"B 域开工验证"）。
pub struct KickoffLedger {
    /// 包指纹（开工验证的对照真源）。
    package_fp: u64,
    /// B 域首批引用（批次号 → 引用件号 + 引用指纹）。
    refs: Vec<(u32, usize, u64)>,
    /// 台账（判据"台账更新"：开工验证通过的批次号）。
    ledger: Vec<u32>,
    /// 纪要指纹（判据"纪要归档"）。
    minutes_fp: Option<u64>,
    /// 封账。
    sealed: Option<u64>,
}

impl KickoffLedger {
    /// 以冻结包指纹开账。
    pub fn new(package_fp: u64) -> Self {
        KickoffLedger {
            package_fp,
            refs: Vec::new(),
            ledger: Vec::new(),
            minutes_fp: None,
            sealed: None,
        }
    }

    /// B 域首批引用登记（判据"引用指纹不符即拒绝开工"）：
    /// 批次引用包内件，引用指纹必须等于包指纹（包级引用语义——
    /// B 域引用的是冻结包整体版本）。
    pub fn cite(&mut self, batch: u32, part: usize, cited_fp: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 10);
        }
        if part >= PACKAGE_PARTS {
            return Err(SEG_BASE + 2);
        }
        if self.refs.iter().any(|(b, _, _)| *b == batch) {
            return Err(SEG_BASE + 11);
        }
        if cited_fp != self.package_fp {
            return Err(SEG_BASE + 12);
        }
        self.refs.push((batch, part, cited_fp));
        Ok(())
    }

    /// 开工验证通过（引用过的批次才可签入台账）。
    pub fn approve_batch(&mut self, batch: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 10);
        }
        if !self.refs.iter().any(|(b, _, _)| *b == batch) {
            return Err(SEG_BASE + 13);
        }
        if self.ledger.contains(&batch) {
            return Err(SEG_BASE + 14);
        }
        self.ledger.push(batch);
        Ok(())
    }

    /// 纪要归档（判据"纪要归档"；重复归档即拒）。
    pub fn archive_minutes(&mut self, minutes: &[u8]) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 10);
        }
        if self.minutes_fp.is_some() {
            return Err(SEG_BASE + 15);
        }
        if minutes.is_empty() {
            return Err(SEG_BASE + 16);
        }
        let fp = fnv1a64(minutes);
        self.minutes_fp = Some(fp);
        Ok(fp)
    }

    /// 归档核验收口（判据五面：台账非空 + 纪要在档 + 引用全验真）。
    pub fn close(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 17);
        }
        if self.ledger.is_empty() {
            return Err(SEG_BASE + 18);
        }
        if self.minutes_fp.is_none() {
            return Err(SEG_BASE + 19);
        }
        let mut fp = self.package_fp;
        for (b, p, _) in &self.refs {
            fp = fp.wrapping_mul(FNV_PRIME) ^ ((*b as u64) << 8 | *p as u64);
        }
        fp = fp.wrapping_mul(FNV_PRIME) ^ self.minutes_fp.unwrap_or(0);
        self.sealed = Some(fp);
        Ok(fp)
    }

    /// 台账批次数。
    pub fn approved_count(&self) -> usize {
        self.ledger.len()
    }
}

impl HandoverPackage {
    /// 单件验真（判据"移交包四件"的核对面）：对包内第 part 件
    /// 重算指纹与登记指纹比对（防移交后被篡改）。包未冻结返回
    /// Err（冻结是验真的前提）。body 为空即 Err。
    pub fn verify_part(&self, part: usize, body: &[u8]) -> Result<bool, u16> {
        if self.package_fp.is_none() {
            return Err(SEG_BASE + 7);
        }
        if part >= PACKAGE_PARTS {
            return Err(SEG_BASE + 2);
        }
        if body.is_empty() {
            return Err(SEG_BASE + 3);
        }
        let want = match self.parts[part] {
            Some(f) => f,
            None => return Err(SEG_BASE + 8),
        };
        Ok(fnv1a64(body) == want)
    }
}

/// 计划核销条目（判据"剩余计划"的执行面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanSettle {
    /// 计划项编号（1..=36）。
    pub item: u32,
    /// 核销结果：true=已收尾，false=滚入下一批次。
    pub done: bool,
}

/// 收尾计划执行账（判据"剩余计划"的核销面；独立于包冻结——
/// 计划的执行跨越移交边界，B 域接续记账）。
pub struct PlanSettleBook {
    /// 核销条目。
    settles: Vec<PlanSettle>,
    /// 封账。
    sealed: Option<u64>,
}

impl PlanSettleBook {
    /// 空账。
    pub fn new() -> Self {
        PlanSettleBook {
            settles: Vec::new(),
            sealed: None,
        }
    }

    /// 核销登记（重复核销同项即拒；编号越界即拒）。
    pub fn settle(&mut self, item: u32, done: bool) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 20);
        }
        if item == 0 || item > REMAINING_ITEMS {
            return Err(SEG_BASE + 5);
        }
        if self.settles.iter().any(|s| s.item == item) {
            return Err(SEG_BASE + 21);
        }
        self.settles.push(PlanSettle { item, done });
        Ok(())
    }

    /// 完成率（千分比；全 36 项未核销完时按已核销中完成比例）。
    pub fn done_permille(&self) -> Option<u64> {
        if self.settles.is_empty() {
            return None;
        }
        let done = self.settles.iter().filter(|s| s.done).count();
        Some(done as u64 * 1000 / REMAINING_ITEMS as u64)
    }

    /// 未核销项数。
    pub fn unsettled(&self) -> u32 {
        REMAINING_ITEMS - self.settles.len() as u32
    }

    /// 封账（全部 36 项核销完才能封——半截计划不许当完结证据）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 22);
        }
        if self.settles.len() != REMAINING_ITEMS as usize {
            return Err(SEG_BASE + 23);
        }
        let mut fp = FNV_OFFSET;
        for s in &self.settles {
            fp = fp.wrapping_mul(FNV_PRIME) ^ s.item as u64 ^ (s.done as u64);
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for PlanSettleBook {
    fn default() -> Self {
        Self::new()
    }
}
