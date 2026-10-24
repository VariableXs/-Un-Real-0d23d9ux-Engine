//! VE-F3540 · E07 组收口 —— 组级门禁 + 六件齐备 + 双人签署 + 证据包哈希。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3540`
//!
//! # 职责（锚点原文拆解）
//!
//! E07 组（F3521~F3539）收口门禁——结构与 E06 组收口（F3520）
//! 同构，域专属差异：六件的落点锚指向 ver09 系列文件；
//! fuzz 全量重跑对接 F3536 语料。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 缺件 | 阻断 |
//! | 不全 | 黄门 |
//! | 损坏 | 重建哈希 |
//!
//! # 复杂度
//!
//! 全部 O(件数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A90 起）
// ---------------------------------------------------------------------------

/// E07 组收口域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PCloseCode {
    /// 六件缺件。
    Missing = 0x5A90,
    /// 判据未全绿。
    AssertionsRed = 0x5A91,
    /// fuzz 未全量重跑。
    FuzzNotRerun = 0x5A92,
    /// 签署角色重复。
    SameRoleSign = 0x5A93,
    /// 签名指纹过期。
    SignStale = 0x5A94,
    /// 台账为空。
    LedgerEmpty = 0x5A95,
}

impl PCloseCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PCloseCode::Missing => "组收口缺件",
            PCloseCode::AssertionsRed => "判据未全绿",
            PCloseCode::FuzzNotRerun => "fuzz 未全量重跑",
            PCloseCode::SameRoleSign => "签署角色重复",
            PCloseCode::SignStale => "签名指纹过期",
            PCloseCode::LedgerEmpty => "台账为空",
        }
    }

    /// 全阻断。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、六件
// ---------------------------------------------------------------------------

/// 六件类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PDeliverable {
    Impl,
    Checks,
    Docs,
    Bench,
    Fuzz,
    Ledger,
}

impl PDeliverable {
    /// 全集。
    pub const ALL: [PDeliverable; 6] = [
        PDeliverable::Impl,
        PDeliverable::Checks,
        PDeliverable::Docs,
        PDeliverable::Bench,
        PDeliverable::Fuzz,
        PDeliverable::Ledger,
    ];

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            PDeliverable::Impl => "实现",
            PDeliverable::Checks => "判据",
            PDeliverable::Docs => "文档",
            PDeliverable::Bench => "基准",
            PDeliverable::Fuzz => "fuzz 语料",
            PDeliverable::Ledger => "台账",
        }
    }
}

/// 六件核对。
pub fn check_six(items: &[(PDeliverable, &str)]) -> Result<(), PCloseCode> {
    for d in PDeliverable::ALL {
        match items.iter().find(|(k, _)| *k == d) {
            None => return Err(PCloseCode::Missing),
            Some((_, a)) if a.is_empty() => return Err(PCloseCode::Missing),
            _ => {}
        }
    }
    Ok(())
}

/// fuzz 重跑账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PFuzzRerun {
    pub rounds: u32,
    pub samples: u32,
    pub green: bool,
}

impl PFuzzRerun {
    /// 全量达标（语料 ≥ 24：E07 三类语料规模）。
    pub fn sufficient(&self) -> bool {
        self.green && self.samples >= 24
    }
}

// ---------------------------------------------------------------------------
// 三、双签
// ---------------------------------------------------------------------------

/// 签名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PSignature {
    pub who: &'static str,
    pub author: bool,
    pub fingerprint: u64,
}

/// 双签册。
#[derive(Default)]
pub struct PSignBook {
    sigs: Vec<PSignature>,
}

impl PSignBook {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 签署（角色互异）。
    pub fn sign(&mut self, who: &'static str, author: bool, fp: u64) -> Result<(), PCloseCode> {
        if self.sigs.iter().any(|s| s.who == who || s.author == author) {
            return Err(PCloseCode::SameRoleSign);
        }
        self.sigs.push(PSignature { who, author, fingerprint: fp });
        Ok(())
    }

    /// 双签齐新。
    pub fn complete_and_fresh(&self, current_fp: u64) -> Result<(), PCloseCode> {
        if self.sigs.len() < 2 {
            return Err(PCloseCode::SameRoleSign);
        }
        for s in &self.sigs {
            if s.fingerprint != current_fp {
                return Err(PCloseCode::SignStale);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、证据包与门禁
// ---------------------------------------------------------------------------

/// FNV-1a 64。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 证据包。
pub struct PEvidencePack {
    pub items: Vec<(PDeliverable, &'static str)>,
    pub fuzz: PFuzzRerun,
}

impl PEvidencePack {
    /// 包哈希。
    pub fn hash(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for (k, a) in &self.items {
            buf.extend_from_slice((*k as u16).to_le_bytes().as_slice());
            buf.extend_from_slice(a.as_bytes());
        }
        buf.extend_from_slice(&self.fuzz.rounds.to_le_bytes());
        buf.extend_from_slice(&self.fuzz.samples.to_le_bytes());
        buf.push(self.fuzz.green as u8);
        fnv1a64(&buf)
    }
}

/// 门禁结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PGateVerdict {
    Green,
    Red(PCloseCode),
}

/// 组级门禁。
#[derive(Default)]
pub struct PGroupGate {
    pub runs: u32,
}

impl PGroupGate {
    /// 收口审计。
    pub fn audit(
        &mut self,
        pack: &PEvidencePack,
        book: &PSignBook,
        ledger_entries: usize,
    ) -> PGateVerdict {
        self.runs += 1;
        if let Err(c) = check_six(&pack.items) {
            return PGateVerdict::Red(c);
        }
        if !pack.fuzz.sufficient() {
            return PGateVerdict::Red(PCloseCode::FuzzNotRerun);
        }
        if let Err(c) = book.complete_and_fresh(pack.hash()) {
            return PGateVerdict::Red(c);
        }
        if ledger_entries == 0 {
            return PGateVerdict::Red(PCloseCode::LedgerEmpty);
        }
        PGateVerdict::Green
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> PEvidencePack {
        PEvidencePack {
            items: vec![
                (PDeliverable::Impl, "ver09a..ver09s"),
                (PDeliverable::Checks, "E07 判据册"),
                (PDeliverable::Docs, "ver09q_pdocs"),
                (PDeliverable::Bench, "ver09r_pbench"),
                (PDeliverable::Fuzz, "ver09p_pfuzz"),
                (PDeliverable::Ledger, "E07-ledger"),
            ],
            fuzz: PFuzzRerun { rounds: 2, samples: 32, green: true },
        }
    }

    #[test]
    fn green_path() {
        let mut g = PGroupGate::default();
        let p = pack();
        let fp = p.hash();
        let mut b = PSignBook::new();
        b.sign("wb1022", true, fp).unwrap();
        b.sign("rev", false, fp).unwrap();
        assert_eq!(g.audit(&p, &b, 3), PGateVerdict::Green);
    }

    #[test]
    fn fuzz_short_blocked() {
        let mut g = PGroupGate::default();
        let mut p = pack();
        p.fuzz.samples = 10;
        let fp = p.hash();
        let mut b = PSignBook::new();
        b.sign("a", true, fp).unwrap();
        b.sign("b", false, fp).unwrap();
        assert_eq!(g.audit(&p, &b, 1), PGateVerdict::Red(PCloseCode::FuzzNotRerun));
    }

    #[test]
    fn missing_piece_blocked() {
        let mut items = vec![(PDeliverable::Impl, "x"), (PDeliverable::Ledger, "l")];
        assert_eq!(check_six(&items), Err(PCloseCode::Missing));
        items.push((PDeliverable::Checks, ""));
        assert_eq!(check_six(&items), Err(PCloseCode::Missing));
    }
}
