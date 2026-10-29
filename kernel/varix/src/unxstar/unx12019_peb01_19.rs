//! UNX-F12019 PEB/TEB 版本分档表隔离机制（Win10/11 共性字段先行）（0x19480 段）
//!
//! 判据原文：版本分档二档闭集（Win10 档 build<22000，Win11 档 build
//! ≥22000）；字段可用性按档声明——共性字段两档皆可用，档专属字段在
//! 异档访问即拒（TIER_MISMATCH）；未知 build 归不了档即拒。
//!
//! 结构：版本档（VerTier 闭集 × build 归档核）+ 字段可用表
//! （FieldId × 档位闭集）+ 访问核（三码守卫）+ 访问账（VtRec 留痕）+
//! 折叠指纹 + 印账冻结 + 注入核 + 判据聚合（run_unx12019_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19480 独占 0x20）。
pub const SEG_BASE: u32 = 0x19480;
pub const VT_UNKNOWN_BUILD: u32 = SEG_BASE + 0x01; // 归不了档
pub const VT_TIER_MISMATCH: u32 = SEG_BASE + 0x02; // 异档访问专属字段
pub const VT_BAD_FIELD: u32 = SEG_BASE + 0x03; // 未知字段
pub const VT_FROZEN: u32 = SEG_BASE + 0x04; // 印账后仍访问
pub const VT_NO_TIER: u32 = SEG_BASE + 0x05; // 未归档先访问

/// 版本档闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerTier {
    Win10 = 1,
    Win11 = 2,
}

/// build 归档核：22000 分界。
pub fn tier_of(build: u32) -> Option<VerTier> {
    if build >= 10240 && build < 22000 {
        Some(VerTier::Win10)
    } else if build >= 22000 && build <= 99999 {
        Some(VerTier::Win11)
    } else {
        None
    }
}

/// PEB/TEB 字段闭集（分档声明面精选）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    BeingDebugged,   // 共性
    ProcessHeap,     // 共性
    ClientId,        // 共性（TEB）
    SelfPtr,         // 共性（TEB）
    ImageBaseAddress, // 共性
    Win11PebHeapVa,  // Win11 档专属（语义预告位）
    Win10TracingFlags, // Win10 档专属
}

/// 字段可用档集：共性字段两档皆可用。
pub fn field_tiers(f: FieldId) -> [bool; 2] {
    // [Win10 可用, Win11 可用]
    match f {
        FieldId::BeingDebugged => [true, true],
        FieldId::ProcessHeap => [true, true],
        FieldId::ClientId => [true, true],
        FieldId::SelfPtr => [true, true],
        FieldId::ImageBaseAddress => [true, true],
        FieldId::Win11PebHeapVa => [false, true],
        FieldId::Win10TracingFlags => [true, false],
    }
}

/// 一次字段访问的账目。
#[derive(Clone)]
pub struct VtRec {
    pub seq: u32,
    pub tier: VerTier,
    pub field: FieldId,
    pub granted: bool,
    pub note: String,
}

impl VtRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 分档访问模型。
pub struct TierGate {
    pub tier: Option<VerTier>,
    pub ledger: Vec<VtRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TierGate {
    pub fn new() -> TierGate {
        TierGate { tier: None, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 归档核。
    pub fn adopt(&mut self, build: u32) -> Result<VerTier, u32> {
        if self.sealed {
            return Err(VT_FROZEN);
        }
        match tier_of(build) {
            Some(t) => {
                self.tier = Some(t);
                Ok(t)
            }
            None => Err(VT_UNKNOWN_BUILD),
        }
    }

    /// 访问核：未归档拒 / 未知字段拒 / 异档专属拒 / 共性放行。
    pub fn access(&mut self, f: FieldId, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(VT_FROZEN);
        }
        if note.is_empty() {
            return Err(VT_BAD_FIELD);
        }
        let t = match self.tier {
            Some(t) => t,
            None => return Err(VT_NO_TIER),
        };
        let avail = field_tiers(f);
        let idx = match t {
            VerTier::Win10 => 0usize,
            VerTier::Win11 => 1usize,
        };
        let granted = avail[idx];
        self.ledger.push(VtRec {
            seq: self.seq_next,
            tier: t,
            field: f,
            granted,
            note: note.to_string(),
        });
        self.seq_next += 1;
        if granted {
            Ok(())
        } else {
            Err(VT_TIER_MISMATCH)
        }
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 放行/拒收清点。
    pub fn granted_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.granted).count()
    }
    pub fn denied_count(&self) -> usize {
        self.ledger.iter().filter(|r| !r.granted).count()
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.tier.map(|t| t as u32).unwrap_or(0));
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            h = h.wrapping_mul(211).wrapping_add(r.field as u32);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut g = TierGate::new();
    // 1) 归不了档的 build（95/220000）→ VT_UNKNOWN_BUILD。
    got[0] = g.adopt(95).err() == Some(VT_UNKNOWN_BUILD)
        && g.adopt(220000).err() == Some(VT_UNKNOWN_BUILD);
    // 2) 未归档先访问 → VT_NO_TIER。
    got[1] = g.access(FieldId::BeingDebugged, "pre").err() == Some(VT_NO_TIER);
    // 3) 异档专属访问：Win10 档访问 Win11 专属 → VT_TIER_MISMATCH。
    let _ = g.adopt(19045);
    got[2] = g.access(FieldId::Win11PebHeapVa, "x").err() == Some(VT_TIER_MISMATCH);
    // 4) 空留痕访问 → VT_BAD_FIELD。
    got[3] = g.access(FieldId::ClientId, "").err() == Some(VT_BAD_FIELD);
    // 5) 印账后访问 → VT_FROZEN。
    g.seal();
    got[4] = g.access(FieldId::ClientId, "post").err() == Some(VT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12019_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12019");

    // J1 build 归档分界：22000 精确切档。
    let j1 = tier_of(19045) == Some(VerTier::Win10)
        && tier_of(21999) == Some(VerTier::Win10)
        && tier_of(22000) == Some(VerTier::Win11)
        && tier_of(22631) == Some(VerTier::Win11);
    s.add("J1-归档分界", j1, "22000 精确切档");

    // J2 归不了档拒：低界与高界双面。
    let j2 = tier_of(95).is_none() && tier_of(0).is_none() && tier_of(220000).is_none();
    s.add("J2-幽灵 build 拒", j2, "闭集外全拒");

    // J3 归档下界收：10240 恰好入 Win10 档。
    let j3 = tier_of(10240) == Some(VerTier::Win10);
    s.add("J3-下界收", j3, "10240 入档");

    // J4 共性字段双档可用。
    let j4 = {
        let common = [
            FieldId::BeingDebugged,
            FieldId::ProcessHeap,
            FieldId::ClientId,
            FieldId::SelfPtr,
            FieldId::ImageBaseAddress,
        ];
        common.iter().all(|f| {
            let t = field_tiers(*f);
            t[0] && t[1]
        })
    };
    s.add("J4-共性双档", j4, "五共性字段全通");

    // J5 专属字段单档可用。
    let j5 = {
        let w11 = field_tiers(FieldId::Win11PebHeapVa);
        let w10 = field_tiers(FieldId::Win10TracingFlags);
        w11 == [false, true] && w10 == [true, false]
    };
    s.add("J5-专属单档", j5, "专属面精确");

    // J6 未归档先访问拒。
    let j6 = {
        let mut g = TierGate::new();
        g.access(FieldId::ProcessHeap, "pre").err() == Some(VT_NO_TIER)
    };
    s.add("J6-未归档拒", j6, "先归档后访问");

    // J7 异档专属访问拒（Win11 档访问 Win10 专属）。
    let j7 = {
        let mut g = TierGate::new();
        let _ = g.adopt(22631);
        g.access(FieldId::Win10TracingFlags, "x").err() == Some(VT_TIER_MISMATCH)
    };
    s.add("J7-异档拒", j7, "专属隔离");

    // J8 专属字段本档放行。
    let j8 = {
        let mut g = TierGate::new();
        let _ = g.adopt(22631);
        g.access(FieldId::Win11PebHeapVa, "ok").is_ok()
            && g.access(FieldId::BeingDebugged, "ok2").is_ok()
    };
    s.add("J8-本档放行", j8, "专属+共性全通");

    // J9 访问账对平：放行/拒收合计==账数。
    let j9 = {
        let mut g = TierGate::new();
        let _ = g.adopt(19045);
        let _ = g.access(FieldId::BeingDebugged, "g");
        let _ = g.access(FieldId::Win11PebHeapVa, "d");
        g.granted_count() + g.denied_count() == g.ledger.len()
            && g.granted_count() == 1 && g.denied_count() == 1
    };
    s.add("J9-账面对平", j9, "一放一拒对账");

    // J10 印账冻结。
    let j10 = {
        let mut g = TierGate::new();
        let _ = g.adopt(19045);
        let f1 = g.fold();
        let f2 = g.fold();
        g.seal();
        f1 == f2 && g.access(FieldId::ClientId, "post").err() == Some(VT_FROZEN)
    };
    s.add("J10-指纹封账", j10, "指纹幂等且印后必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：Win11 档访问面。
    let mut g = TierGate::new();
    let _ = g.adopt(26100);
    let _ = g.access(FieldId::SelfPtr, "sum1");
    let _ = g.access(FieldId::Win11PebHeapVa, "sum2");
    s.add("样例双访问过", g.granted_count() == 2, "Win11 档全通");
    s.add("样例账面合法", g.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12019_tier_flow() {
        let mut g = TierGate::new();
        assert_eq!(g.adopt(19045), Ok(VerTier::Win10));
        assert_eq!(g.adopt(22631), Ok(VerTier::Win11));
        assert_eq!(g.adopt(1), Err(VT_UNKNOWN_BUILD));
        assert!(g.access(FieldId::BeingDebugged, "a").is_ok());
        // Win11 档：本档专属放行、异档专属拒。
        assert!(g.access(FieldId::Win11PebHeapVa, "b").is_ok());
        assert_eq!(g.access(FieldId::Win10TracingFlags, "c"), Err(VT_TIER_MISMATCH));
    }

    #[test]
    fn unx12019_checks_green() {
        let s = run_unx12019_checks();
        assert!(s.all_passed(), "UNX-F12019 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12019_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
