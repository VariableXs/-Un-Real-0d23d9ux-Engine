//! cga149_declfin — CGPU-F0149 A 域完成宣告（F0396 正式程序）。
//!
//! 判据原文（施工书锚点 CGPU-F0149）：A 域 160 项完成宣告（F0396
//! 程序正式执行）：证据包（聚合自检 F0141/验收矩阵 F0145/压测终版
//! F0146/安全终审 F0147/查重 F0144/三账 F0126-28/兼容 F0148）、双签
//! 宣告、台账状态变更、回归级规则生效。判据：证据包齐、双签、台账
//! 变更、回归规则、可追溯。
//!
//! **设计定盘**：与 F0137（阶段宣告）的域级差异：终版证据包九件
//! 闭集（聚合自检/验收矩阵/压测终版/安全终审/查重补审/性能账/内存
//! 账/错误码账/兼容终版——F0126/127/128 三账逐件点名不许合册）。
//! 宣告前提 = 九件齐 + 双签。回归规则生效 = 宣告后回归级处置立即
//! 生效（宣告账自带回归立案通道）。可追溯 = 九件 + 双签链式指纹。
//!
//! 诊断码段 0x0340~0x034F（0x03 段第 5 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA149_VERSION: &str = "cga149-declfin-v1";

/// 诊断子区基值（0x03 段第 5 子区）。
pub const SEG_BASE: u16 = 0x0340;

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

/// 九件证据闭集（判据"证据包"——三账逐件点名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinEvidence {
    /// 聚合自检（F0141）。
    AggCheck,
    /// 验收矩阵终版（F0145）。
    AcceptFin,
    /// 压测终版（F0146）。
    StressFin,
    /// 安全终审（F0147）。
    SecFin,
    /// 查重补终审（F0144）。
    DedupSup,
    /// 性能总账（F0126）。
    PerfLedger,
    /// 内存总账（F0127）。
    MemLedger,
    /// 错误码总表（F0128）。
    ErrTable,
    /// 兼容终版（F0148）。
    CompatFin,
}

impl FinEvidence {
    /// 全集稳定序。
    pub const ALL: [FinEvidence; 9] = [
        FinEvidence::AggCheck,
        FinEvidence::AcceptFin,
        FinEvidence::StressFin,
        FinEvidence::SecFin,
        FinEvidence::DedupSup,
        FinEvidence::PerfLedger,
        FinEvidence::MemLedger,
        FinEvidence::ErrTable,
        FinEvidence::CompatFin,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            FinEvidence::AggCheck => 0,
            FinEvidence::AcceptFin => 1,
            FinEvidence::StressFin => 2,
            FinEvidence::SecFin => 3,
            FinEvidence::DedupSup => 4,
            FinEvidence::PerfLedger => 5,
            FinEvidence::MemLedger => 6,
            FinEvidence::ErrTable => 7,
            FinEvidence::CompatFin => 8,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 域级宣告账。
pub struct FinDeclareLedger {
    /// 九件证据。
    evidence: [Option<u64>; 9],
    /// 施工签。
    builder: Option<u64>,
    /// 验收签。
    acceptor: Option<u64>,
    /// 宣告指纹。
    declared: Option<u64>,
    /// 回归立案账。
    regressions: Vec<u32>,
}

impl FinDeclareLedger {
    /// 空账。
    pub fn new() -> Self {
        FinDeclareLedger {
            evidence: [None; 9],
            builder: None,
            acceptor: None,
            declared: None,
            regressions: Vec::new(),
        }
    }

    /// 证据入包。
    pub fn submit(&mut self, ev: FinEvidence, fp: u64) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 10);
        }
        if fp == 0 {
            return Err(SEG_BASE + 11);
        }
        let slot = &mut self.evidence[ev.ordinal() as usize];
        if slot.is_some() {
            return Err(SEG_BASE + 12);
        }
        *slot = Some(fp);
        Ok(())
    }

    /// 缺件清单。
    pub fn missing(&self) -> Vec<FinEvidence> {
        FinEvidence::ALL
            .iter()
            .copied()
            .filter(|ev| self.evidence[ev.ordinal() as usize].is_none())
            .collect()
    }

    /// 施工签。
    pub fn sign_builder(&mut self, sig: &[u8]) -> Result<u64, u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 10);
        }
        if self.builder.is_some() || sig.is_empty() {
            return Err(SEG_BASE + 13);
        }
        let fp = fnv1a64(sig);
        self.builder = Some(fp);
        Ok(fp)
    }

    /// 验收签（同人同签即拒）。
    pub fn sign_acceptor(&mut self, sig: &[u8]) -> Result<u64, u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 10);
        }
        let b = self.builder.ok_or(SEG_BASE + 14)?;
        if self.acceptor.is_some() || sig.is_empty() {
            return Err(SEG_BASE + 15);
        }
        let fp = fnv1a64(sig);
        if fp == b {
            return Err(SEG_BASE + 16);
        }
        self.acceptor = Some(fp);
        Ok(fp)
    }

    /// 宣告（判据"证据包齐 + 双签"；宣告后回归规则生效）。
    pub fn declare(&mut self) -> Result<u64, u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 17);
        }
        for ev in FinEvidence::ALL {
            if self.evidence[ev.ordinal() as usize].is_none() {
                return Err(ev.code());
            }
        }
        let b = self.builder.ok_or(SEG_BASE + 14)?;
        let a = self.acceptor.ok_or(SEG_BASE + 15)?;
        let mut fp = FNV_OFFSET;
        for slot in &self.evidence {
            fp = fp.wrapping_mul(FNV_PRIME) ^ slot.unwrap_or(0);
        }
        fp = fp.wrapping_mul(FNV_PRIME) ^ b;
        fp = fp.wrapping_mul(FNV_PRIME) ^ a;
        self.declared = Some(fp);
        Ok(fp)
    }

    /// 回归立案（判据"回归级规则生效"：宣告后通道；重复立案即拒）。
    pub fn regression(&mut self, id: u32) -> Result<(), u16> {
        if self.declared.is_none() {
            return Err(SEG_BASE + 18);
        }
        if id == 0 || self.regressions.contains(&id) {
            return Err(SEG_BASE + 19);
        }
        self.regressions.push(id);
        Ok(())
    }

    /// 回归立案数。
    pub fn regression_count(&self) -> usize {
        self.regressions.len()
    }

    /// 宣告指纹。
    pub fn declare_fp(&self) -> Option<u64> {
        self.declared
    }

    /// 证据齐备。
    pub fn evidence_ready(&self) -> bool {
        self.evidence.iter().all(|e| e.is_some())
    }
}

impl Default for FinDeclareLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl FinDeclareLedger {
    /// 证据齐备度（九件中已落几件）。
    pub fn evidence_count(&self) -> usize {
        self.evidence.iter().filter(|e| e.is_some()).count()
    }

    /// 缺件清单：未落证据的槽位号（0~8）。
    pub fn missing_slots(&self) -> Vec<usize> {
        self.evidence
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_none())
            .map(|(i, _)| i)
            .collect()
    }

    /// 双签完整性（判据「施工签/验收签互异」核对面）：
    /// 两签齐备且不同指纹（同指纹 = 自己验自己）。
    pub fn dual_sign_ok(&self) -> Result<(), u16> {
        match (self.builder, self.acceptor) {
            (Some(b), Some(a)) if b != a => Ok(()),
            (Some(_), Some(_)) => Err(SEG_BASE + 20),
            _ => Err(SEG_BASE + 21),
        }
    }

    /// 回归立案清空检查（判据「宣告前回归清零」）：立案账非空即拒。
    pub fn regressions_clear(&self) -> Result<(), u16> {
        if self.regressions.is_empty() {
            Ok(())
        } else {
            Err(SEG_BASE + 22)
        }
    }

    /// 宣告前置总检：九件齐 + 双签异 + 回归清零。
    pub fn pre_declare(&self) -> Result<(), u16> {
        if self.evidence_count() < 9 {
            return Err(SEG_BASE + 23);
        }
        self.dual_sign_ok()?;
        self.regressions_clear()
    }
}

/// 宣告指纹合成（独立函数面）：九件证据按槽序折入 FNV 链，
/// 双签指纹收尾——宣告指纹是全链证据的确定性摘要，任何一件
/// 变动都会改变宣告指纹（防换件不换签）。
pub fn declare_fingerprint(
    evidence: &[Option<u64>; 9],
    builder: u64,
    acceptor: u64,
) -> Result<u64, u16> {
    let mut acc: u64 = 0xcbf29ce484222325;
    for slot in evidence {
        let v = match slot {
            Some(v) => *v,
            None => return Err(SEG_BASE + 24),
        };
        acc = acc.wrapping_mul(0x100000001b3) ^ v;
    }
    acc = acc.wrapping_mul(0x100000001b3) ^ builder;
    acc = acc.wrapping_mul(0x100000001b3) ^ acceptor;
    if acc == 0 {
        return Err(SEG_BASE + 25);
    }
    Ok(acc)
}
