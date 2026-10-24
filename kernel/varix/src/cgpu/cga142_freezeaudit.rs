//! cga142_freezeaudit — CGPU-F0142 A 域接口冻结清单复核。
//!
//! 判据原文（施工书锚点 CGPU-F0142）：F0125 冻结的复核与补遗：与
//! B 域（任务图已开工 F0124 移交后）的实际消费面核对——B 域用到的
//! 接口是否全部在冻结清单内（B 域自建接口=绕过冻结的违规检查）、
//! 语义测试覆盖复核、十年承诺重申。判据：消费面核对零缺口、绕过
//! 检查、语义测试、承诺重申、复核报告。
//!
//! **设计定盘**：冻结复核账：消费面账 = B 域实际引用的接口清单
//! （接口号 + 引用指纹）。复核 = 消费面 ⊆ 冻结面（消费了冻结清单外
//! 的接口 = 绕过冻结违规，逐条立案）。缺口 = 冻结面中无人消费的
//! 接口（不是违规但是死面——提示冻结清单膨胀）。语义测试复核 =
//! 冻结面逐接口测试指纹非零。承诺重申 = 复核通过后落重申签章。
//! 复核报告 = 消费面/缺口/违规三清单的指纹链。
//!
//! 诊断码段 0x02D0~0x02DF（0x02 段第 14 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA142_VERSION: &str = "cga142-freezeaudit-v1";

/// 诊断子区基值（0x02 段第 14 子区）。
pub const SEG_BASE: u16 = 0x02D0;

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

/// 一条冻结接口（F0125 冻结面的投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenIface {
    /// 接口号。
    pub id: u32,
    /// 签名指纹。
    pub sig_fp: u64,
    /// 语义测试指纹（零=无测试=复核红）。
    pub test_fp: u64,
}

/// 一条消费引用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConsumeRef {
    /// 接口号。
    pub id: u32,
    /// 消费方（0=B 域批次 1..=N）。
    pub batch: u32,
    /// 引用指纹（应等于冻结签名指纹）。
    pub cited_fp: u64,
}

/// 冻结复核账。
pub struct FreezeAudit {
    /// 冻结面。
    frozen: Vec<FrozenIface>,
    /// 消费面。
    consumed: Vec<ConsumeRef>,
    /// 绕过违规立案（接口号清单——消费了冻结面外的接口）。
    bypass_cases: Vec<u32>,
    /// 重申签章。
    reaffirmed: Option<u64>,
}

impl FreezeAudit {
    /// 空账。
    pub fn new() -> Self {
        FreezeAudit {
            frozen: Vec::new(),
            consumed: Vec::new(),
            bypass_cases: Vec::new(),
            reaffirmed: None,
        }
    }

    /// 冻结面导入（同号重复即拒）。
    pub fn import_frozen(&mut self, f: FrozenIface) -> Result<(), u16> {
        if self.reaffirmed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if f.id == 0 || self.frozen.iter().any(|x| x.id == f.id) {
            return Err(SEG_BASE + 5);
        }
        if f.sig_fp == 0 {
            return Err(SEG_BASE + 6);
        }
        self.frozen.push(f);
        Ok(())
    }

    /// 消费登记（判据"消费面核对"；同批次同接口重复即拒）。
    pub fn consume(&mut self, r: ConsumeRef) -> Result<(), u16> {
        if self.reaffirmed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if r.batch == 0 {
            return Err(SEG_BASE + 7);
        }
        if self
            .consumed
            .iter()
            .any(|x| x.id == r.id && x.batch == r.batch)
        {
            return Err(SEG_BASE + 8);
        }
        self.consumed.push(r);
        Ok(())
    }

    /// 绕过检查（判据"绕过冻结的违规检查"）：消费面逐条对照冻结面
    /// ——引用了未冻结接口即立案；返回违规数。
    pub fn audit_bypass(&mut self) -> Result<usize, u16> {
        if self.reaffirmed.is_some() {
            return Err(SEG_BASE + 4);
        }
        self.bypass_cases.clear();
        for c in &self.consumed {
            let hit = self.frozen.iter().find(|f| f.id == c.id);
            match hit {
                None => {
                    if !self.bypass_cases.contains(&c.id) {
                        self.bypass_cases.push(c.id);
                    }
                }
                Some(f) => {
                    if f.sig_fp != c.cited_fp {
                        // 指纹漂移也是绕过（引用了改签后的接口）。
                        if !self.bypass_cases.contains(&c.id) {
                            self.bypass_cases.push(c.id);
                        }
                    }
                }
            }
        }
        Ok(self.bypass_cases.len())
    }

    /// 违规查询。
    pub fn bypass_row(&self, idx: usize) -> Option<u32> {
        self.bypass_cases.get(idx).copied()
    }

    /// 死面清单（判据"缺口"：冻结面中无人消费的接口——不是违规，
    /// 是冻结清单膨胀提示）。
    pub fn dead_faces(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for f in &self.frozen {
            if !self.consumed.iter().any(|c| c.id == f.id) {
                out.push(f.id);
            }
        }
        out
    }

    /// 语义测试覆盖复核（判据"语义测试覆盖复核"：冻结面逐接口
    /// 测试指纹非零）。
    pub fn test_coverage(&self) -> Result<(), u16> {
        for f in &self.frozen {
            if f.test_fp == 0 {
                return Err(SEG_BASE + 9);
            }
        }
        Ok(())
    }

    /// 违规清零核验。
    pub fn bypass_clear(&self) -> bool {
        self.bypass_cases.is_empty()
    }

    /// 消费面批次投影（判据"消费面核对零缺口"：有多少批次在消费）。
    pub fn consumer_batches(&self) -> usize {
        let mut seen: Vec<u32> = Vec::new();
        for c in &self.consumed {
            if !seen.contains(&c.batch) {
                seen.push(c.batch);
            }
        }
        seen.len()
    }

    /// 重申签章（判据"十年承诺重申"：绕过清零 + 语义测试全覆 +
    /// 冻结面非空才能重申）。
    pub fn reaffirm(&mut self) -> Result<u64, u16> {
        if self.reaffirmed.is_some() {
            return Err(SEG_BASE + 10);
        }
        if self.frozen.is_empty() {
            return Err(SEG_BASE + 11);
        }
        if !self.bypass_clear() {
            return Err(SEG_BASE + 12);
        }
        self.test_coverage()?;
        let mut fp = FNV_OFFSET;
        for f in &self.frozen {
            fp = fp.wrapping_mul(FNV_PRIME) ^ f.sig_fp ^ f.test_fp;
        }
        for c in &self.consumed {
            fp = fp.wrapping_mul(FNV_PRIME) ^ c.cited_fp;
        }
        self.reaffirmed = Some(fp);
        Ok(fp)
    }

    /// 冻结面数。
    pub fn frozen_count(&self) -> usize {
        self.frozen.len()
    }
}

impl Default for FreezeAudit {
    fn default() -> Self {
        Self::new()
    }
}

/// 测试缺口语义档（判据"语义测试覆盖复核"的定位面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TestGap {
    /// 接口号。
    pub id: u32,
}

impl FreezeAudit {
    /// 测试缺口清单（test_fp 为零的冻结接口）。
    pub fn test_gaps(&self) -> Vec<TestGap> {
        self.frozen
            .iter()
            .filter(|f| f.test_fp == 0)
            .map(|f| TestGap { id: f.id })
            .collect()
    }

    /// 消费批次清单（判据"消费面核对"的投影：每批次消费了多少
    /// 冻结接口）。
    pub fn batch_census(&self) -> Vec<(u32, usize)> {
        let mut out: Vec<(u32, usize)> = Vec::new();
        for c in &self.consumed {
            match out.iter_mut().find(|(b, _)| *b == c.batch) {
                Some((_, n)) => *n += 1,
                None => out.push((c.batch, 1)),
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// 死面数（判据"缺口"：冻结面无人消费数）。
    pub fn dead_count(&self) -> usize {
        self.dead_faces().len()
    }

    /// 违规数。
    pub fn bypass_count(&self) -> usize {
        self.bypass_cases.len()
    }

    /// 引用指纹漂移清单（判据"指纹不符即开工包版本漂移"的定位面：
    /// 引用了冻结接口但指纹与冻结签名不一致的 (批次, 接口) 对）。
    pub fn drifted_refs(&self) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        for c in &self.consumed {
            if let Some(f) = self.frozen.iter().find(|f| f.id == c.id) {
                if f.sig_fp != c.cited_fp {
                    out.push((c.batch, c.id));
                }
            }
        }
        out
    }

    /// 冻结面投影（只读行迭代）。
    pub fn frozen_rows(&self) -> &Vec<FrozenIface> {
        &self.frozen
    }

    /// 重申签章。
    pub fn reaffirm_fp(&self) -> Option<u64> {
        self.reaffirmed
    }

    /// 复核报告摘要（判据"复核报告"：冻结数/消费数/违规数/死面数
    /// 四元组——报告一行的账面）。
    pub fn report_summary(&self) -> (usize, usize, usize, usize) {
        (
            self.frozen.len(),
            self.consumed.len(),
            self.bypass_cases.len(),
            self.dead_faces().len(),
        )
    }

    /// 消费面总数。
    pub fn consume_count(&self) -> usize {
        self.consumed.len()
    }
}


// ===== 深化块（cga142_freezeaudit · 冻结审计深化）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_freezeaudit {
    //! 冻结审计深化 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 冻结漂移检测：冻结指纹两次采样不一致 = 有人改了冻结面。
    pub fn drift_detected(h1: u64, h2: u64) -> Result<(), u16> {
        if h1 == 0 || h2 == 0 {
            return Err(1);
        }
        if h1 != h2 {
            return Err(2);
        }
        Ok(())
    }

    /// 审计留痕完整性：审计项必须带理由，空理由 = 审计作废。
    pub fn audit_reasoned(entries: &[(u64, bool)]) -> Result<(), u16> {
        for (i, (reason, _)) in entries.iter().enumerate() {
            if *reason == 0 {
                return Err(1 + i as u16);
            }
        }
        Ok(())
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0142-深化");
        cs.add("指纹一致放行", drift_detected(0xA1B2, 0xA1B2).is_ok(), "");
        cs.add("漂移检出", drift_detected(0xA1B2, 0xC3D4).is_err(), "");
        cs.add("空指纹拒", drift_detected(0, 1).is_err(), "");
        cs.add("留痕必须带因", audit_reasoned(&[(1, true), (2, false)]).is_ok() && audit_reasoned(&[(0, true)]).is_err(), "");
        cs
    }
}
