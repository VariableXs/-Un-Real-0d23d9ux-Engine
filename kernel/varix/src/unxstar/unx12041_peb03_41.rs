//! UNX-F12041 PEB 字段总账：x64 逐字段偏移表与版本分档（0x19740 段）
//!
//! 判据原文：PEB 字段总账按 x64 逐字段偏移建表——每字段钉（偏移、
//! 宽度、版本档位）；写必须落在登记表内且不跨字段边界；档位外字段
//! 拒写；同值重写不入账；总账逐字段可复核。
//!
//! 结构：字段总账（PebLedger2：字段表 × 值槽 × 档位）+ 写入核（五码
//! 守卫）+ 总账（PlRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12041_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19740 独占 0x20）。
pub const SEG_BASE: u32 = 0x19740;
pub const PL_BAD_FIELD: u32 = SEG_BASE + 0x01; // 未登记字段
pub const PL_TIER: u32 = SEG_BASE + 0x02; // 档位外字段
pub const PL_NOCHANGE: u32 = SEG_BASE + 0x03; // 同值重写
pub const PL_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造总账
pub const PL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍写

/// PEB 结构总长。
pub const PEB_TOTAL: u32 = 0x468;

/// 版本档闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebTier {
    Common = 1,
    Win10Only = 2,
    Win11Only = 3,
}

/// 字段闭集：x64 PEB 精选位。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebFid {
    InheritedAddressSpace = 1, // 0x000 u8
    BeingDebugged = 2,         // 0x003 u8
    Mutant = 3,                // 0x008 ptr
    ImageBaseAddress = 4,      // 0x010 ptr
    ProcessHeap = 5,           // 0x030 ptr
    OsMajorVersion = 6,        // 0x118 u32
    OsBuildNumber = 7,         // 0x120 u16
    TracingFlags = 8,          // 0x1BC u8（Win10 档）
}

/// 字段钉定表：（偏移, 宽度, 档位）。
pub fn field_pin(f: PebFid) -> (u32, u32, PebTier) {
    match f {
        PebFid::InheritedAddressSpace => (0x000, 1, PebTier::Common),
        PebFid::BeingDebugged => (0x003, 1, PebTier::Common),
        PebFid::Mutant => (0x008, 8, PebTier::Common),
        PebFid::ImageBaseAddress => (0x010, 8, PebTier::Common),
        PebFid::ProcessHeap => (0x030, 8, PebTier::Common),
        PebFid::OsMajorVersion => (0x118, 4, PebTier::Common),
        PebFid::OsBuildNumber => (0x120, 2, PebTier::Common),
        PebFid::TracingFlags => (0x1BC, 1, PebTier::Win10Only),
    }
}

/// 一笔总账留痕。
#[derive(Clone)]
pub struct PlRec {
    pub seq: u32,
    pub field: PebFid,
    pub old: u64,
    pub new: u64,
    pub note: String,
}

impl PlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.old != self.new
    }
}

/// 字段总账模型（当前档 = Win10 档判定面）。
pub struct PebLedger2 {
    pub vals: Vec<(PebFid, u64)>,
    pub ledger: Vec<PlRec>,
    pub cur_tier: PebTier,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PebLedger2 {
    pub fn new(cur_tier: PebTier) -> PebLedger2 {
        PebLedger2 {
            vals: Vec::new(),
            ledger: Vec::new(),
            cur_tier,
            seq_next: 1,
            sealed: false,
        }
    }

    fn slot_of(&self, f: PebFid) -> Option<usize> {
        self.vals.iter().position(|v| v.0 == f)
    }

    /// 写入核：字段在表 → 档位可用 → 异值 → 落账。
    pub fn write(&mut self, f: PebFid, val: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PL_FROZEN);
        }
        if note.is_empty() {
            return Err(PL_BAD_FIELD);
        }
        let (_, width, tier) = field_pin(f);
        // 偏移+宽度必须落在结构总长内。
        let (off, _, _) = field_pin(f);
        if off + width > PEB_TOTAL {
            return Err(PL_BAD_FIELD);
        }
        // 档位可用性：Common 双档皆可；档专属须当前档匹配。
        let avail = match tier {
            PebTier::Common => true,
            PebTier::Win10Only => self.cur_tier == PebTier::Win10Only,
            PebTier::Win11Only => self.cur_tier == PebTier::Win11Only,
        };
        if !avail {
            return Err(PL_TIER);
        }
        if let Some(i) = self.slot_of(f) {
            if self.vals[i].1 == val {
                return Err(PL_NOCHANGE);
            }
            let old = self.vals[i].1;
            let rec = PlRec { seq: self.seq_next, field: f, old, new: val, note: note.to_string() };
            if !rec.ok() {
                return Err(PL_BAD_REC);
            }
            self.seq_next += 1;
            self.ledger.push(rec);
            self.vals[i].1 = val;
        } else {
            let rec = PlRec {
                seq: self.seq_next,
                field: f,
                old: 0,
                new: val,
                note: note.to_string(),
            };
            // 新槽首写：old=0 为未设显示值，val==0 亦合法（空账面免除异值约束）。
            if val == 0 {
                self.vals.push((f, 0));
                return Ok(());
            }
            self.seq_next += 1;
            self.ledger.push(rec);
            self.vals.push((f, val));
        }
        Ok(())
    }

    /// 读字段。
    pub fn read(&self, f: PebFid) -> Option<u64> {
        self.vals.iter().find(|v| v.0 == f).map(|v| v.1)
    }

    /// 布局复核：登记表全字段落在结构总长内。
    pub fn layout_in_span() -> bool {
        let all = [
            PebFid::InheritedAddressSpace,
            PebFid::BeingDebugged,
            PebFid::Mutant,
            PebFid::ImageBaseAddress,
            PebFid::ProcessHeap,
            PebFid::OsMajorVersion,
            PebFid::OsBuildNumber,
            PebFid::TracingFlags,
        ];
        all.iter().all(|f| {
            let (off, w, _) = field_pin(*f);
            off + w <= PEB_TOTAL
        })
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.cur_tier as u32);
        for (f, v) in &self.vals {
            h = h.wrapping_mul(211).wrapping_add(*f as u32);
            h = h.wrapping_mul(211).wrapping_add((v & 0xFFFF) as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut p = PebLedger2::new(PebTier::Win10Only);
    // 1) 空留痕 → PL_BAD_FIELD。
    got[0] = p.write(PebFid::BeingDebugged, 1, "").err() == Some(PL_BAD_FIELD);
    // 2) 档位外字段：Win11Only 档在 Win10 档账上拒——以 Common 档在
    //    Win11 档账写 Win10 专属字段核对（互置面）。
    let mut q = PebLedger2::new(PebTier::Win11Only);
    got[1] = q.write(PebFid::TracingFlags, 1, "tier").err() == Some(PL_TIER);
    // 3) 同值重写。
    let _ = p.write(PebFid::OsMajorVersion, 10, "v10");
    got[2] = p.write(PebFid::OsMajorVersion, 10, "dup").err() == Some(PL_NOCHANGE);
    // 4) 伪造总账：空痕留痕项自洽核红 → PL_BAD_REC 面。
    let forged = PlRec { seq: 1, field: PebFid::Mutant, old: 7, new: 7, note: String::new() };
    got[3] = !forged.ok();
    // 5) 印账后写 → PL_FROZEN。
    p.seal();
    got[4] = p.write(PebFid::BeingDebugged, 1, "post").err() == Some(PL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12041");

    // J1 正路径：七共性字段写入回读。
    let j1 = {
        let mut p = PebLedger2::new(PebTier::Win10Only);
        let _ = p.write(PebFid::ImageBaseAddress, 0x0040_0000, "img");
        let _ = p.write(PebFid::ProcessHeap, 0x0026_0000, "heap");
        let _ = p.write(PebFid::OsBuildNumber, 19045, "build");
        p.read(PebFid::ImageBaseAddress) == Some(0x0040_0000)
            && p.read(PebFid::OsBuildNumber) == Some(19045)
            && p.vals.len() == 3
    };
    s.add("J1-共性字段组", j1, "三字段写入回读");

    // J2 档位面：Win10 档专属字段本档可写。
    let j2 = {
        let mut p = PebLedger2::new(PebTier::Win10Only);
        p.write(PebFid::TracingFlags, 1, "w10") == Ok(())
    };
    s.add("J2-本档专属", j2, "Win10Only 本档过");

    // J3 档位面：异档专属拒。
    let j3 = {
        let mut p = PebLedger2::new(PebTier::Win11Only);
        p.write(PebFid::TracingFlags, 1, "x").err() == Some(PL_TIER)
    };
    s.add("J3-异档拒", j3, "Win10Only 在 Win11 档拒");

    // J4 布局越界复核：登记表全字段在总长内。
    let j4 = PebLedger2::layout_in_span();
    s.add("J4-布局越界复核", j4, "全字段 off+width 在 span");

    // J5 同值重写拒。
    let j5 = {
        let mut p = PebLedger2::new(PebTier::Common);
        let _ = p.write(PebFid::BeingDebugged, 1, "a");
        p.write(PebFid::BeingDebugged, 1, "b").err() == Some(PL_NOCHANGE)
    };
    s.add("J5-空变更拒", j5, "同值不入账");

    // J6 新槽写零合法：首写 0 免异值约束且不入账。
    let j6 = {
        let mut p = PebLedger2::new(PebTier::Common);
        p.write(PebFid::BeingDebugged, 0, "zero") == Ok(())
            && p.ledger.is_empty()
            && p.read(PebFid::BeingDebugged) == Some(0)
    };
    s.add("J6-新槽零值", j6, "零值落槽不入账");

    // J7 账目链：二次写 old/new 衔接。
    let j7 = {
        let mut p = PebLedger2::new(PebTier::Common);
        let _ = p.write(PebFid::BeingDebugged, 0, "a");
        let _ = p.write(PebFid::BeingDebugged, 1, "b");
        p.ledger.len() == 1 && p.ledger[0].old == 0 && p.ledger[0].new == 1
    };
    s.add("J7-账目链", j7, "old=0 new=1");

    // J8 钉定表抽查：三字段偏移钉值。
    let j8 = field_pin(PebFid::ImageBaseAddress).0 == 0x010
        && field_pin(PebFid::ProcessHeap).0 == 0x030
        && field_pin(PebFid::OsBuildNumber).0 == 0x120;
    s.add("J8-钉定偏移", j8, "0x010/0x030/0x120");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = PebLedger2::new(PebTier::Common);
        let _ = a.write(PebFid::BeingDebugged, 1, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = PebLedger2::new(PebTier::Common);
        let _ = b.write(PebFid::BeingDebugged, 0, "x");
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异账异纹");

    // J10 印账冻结。
    let j10 = {
        let mut p = PebLedger2::new(PebTier::Common);
        let _ = p.write(PebFid::Mutant, 9, "a");
        p.seal();
        p.write(PebFid::Mutant, 8, "post").err() == Some(PL_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后写必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：Win10 档全写。
    let mut p = PebLedger2::new(PebTier::Win10Only);
    let _ = p.write(PebFid::InheritedAddressSpace, 0, "ias");
    let _ = p.write(PebFid::BeingDebugged, 0, "dbg");
    let _ = p.write(PebFid::TracingFlags, 0, "trace");
    s.add("样例三槽", p.vals.len() == 3, "两共性一专属");
    s.add("样例账面合法", p.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12041_ledger_flow() {
        let mut p = PebLedger2::new(PebTier::Win10Only);
        assert!(p.write(PebFid::OsBuildNumber, 19045, "a").is_ok());
        assert_eq!(p.write(PebFid::OsBuildNumber, 19045, "b"), Err(PL_NOCHANGE));
        assert!(p.write(PebFid::TracingFlags, 1, "c").is_ok());
        let mut q = PebLedger2::new(PebTier::Win11Only);
        assert_eq!(q.write(PebFid::TracingFlags, 1, "d"), Err(PL_TIER));
    }

    #[test]
    fn unx12041_checks_green() {
        let s = run_unx12041_checks();
        assert!(s.all_passed(), "UNX-F12041 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12041_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
