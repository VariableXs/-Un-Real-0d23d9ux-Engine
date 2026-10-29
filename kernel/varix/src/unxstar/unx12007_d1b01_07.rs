//! UNX-F12007 x64 .pdata 解析与 RUNTIME_FUNCTION 查找（0x19300 段）
//!
//! 判据原文：.pdata 异常目录以 RUNTIME_FUNCTION（Begin/End/Unwind 三元组）
//! 连续排布——目录必须按 Begin 严格升序且区间互不重叠（每条 Begin<End）；
//! RIP 二分查找只命中唯一条目；查无命中返回空而不报错；空目录合法但
//! 查找恒空。
//!
//! 结构：函数表册（Vec<RtFunc>）+ 目录装载核（排序/重叠/空区间三码守卫）
//! + RIP 二分查找 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12007_checks）。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19300 独占 0x20）。
pub const SEG_BASE: u32 = 0x19300;
pub const PD_UNSORTED: u32 = SEG_BASE + 0x01; // Begin 非升序
pub const PD_OVERLAP: u32 = SEG_BASE + 0x02; // 区间重叠
pub const PD_EMPTY_RANGE: u32 = SEG_BASE + 0x03; // Begin >= End
pub const PD_FROZEN: u32 = SEG_BASE + 0x04; // 印账后仍装载
pub const PD_ZERO_UNWIND: u32 = SEG_BASE + 0x05; // Unwind 零悬挂

/// RUNTIME_FUNCTION 三元组（x64：三个 u32 RVA）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RtFunc {
    pub begin: u32,
    pub end: u32,
    pub unwind: u32,
}

impl RtFunc {
    pub fn ok(&self) -> bool {
        self.begin < self.end && self.unwind != 0
    }
}

/// .pdata 目录模型。
pub struct PdataDir {
    pub funcs: Vec<RtFunc>,
    pub sealed: bool,
}

impl PdataDir {
    pub fn new() -> PdataDir {
        PdataDir { funcs: Vec::new(), sealed: false }
    }

    /// 目录装载核：逐条校验 + 排序/重叠守卫，追加装载。
    pub fn load(&mut self, e: RtFunc) -> Result<(), u32> {
        if self.sealed {
            return Err(PD_FROZEN);
        }
        if !e.ok() {
            if e.begin >= e.end {
                return Err(PD_EMPTY_RANGE);
            }
            return Err(PD_ZERO_UNWIND);
        }
        // 升序守卫：新条目 Begin 必须不小于上一条 End（不重叠且有序）。
        if let Some(last) = self.funcs.last() {
            if e.begin < last.begin {
                return Err(PD_UNSORTED);
            }
            if e.begin < last.end {
                return Err(PD_OVERLAP);
            }
        }
        self.funcs.push(e);
        Ok(())
    }

    /// RIP 二分查找：命中返回条目下标，查无返回 None（合法空结果）。
    pub fn lookup(&self, rip: u32) -> Option<usize> {
        let n = self.funcs.len();
        if n == 0 {
            return None;
        }
        let mut lo: usize = 0;
        let mut hi: usize = n;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let f = &self.funcs[mid];
            if rip < f.begin {
                hi = mid;
            } else if rip >= f.end {
                lo = mid + 1;
            } else {
                return Some(mid);
            }
        }
        None
    }

    /// 目录有序性复核：全表 Begin 升序且互不重叠。
    pub fn sorted_no_overlap(&self) -> bool {
        for w in self.funcs.windows(2) {
            if w[1].begin < w[0].begin || w[1].begin < w[0].end {
                return false;
            }
        }
        true
    }

    /// 目录条目覆盖审计：Begin 单调不减。
    pub fn begins_monotonic(&self) -> bool {
        for w in self.funcs.windows(2) {
            if w[1].begin < w[0].begin {
                return false;
            }
        }
        true
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for f in &self.funcs {
            h = h.wrapping_mul(211).wrapping_add(f.begin);
            h = h.wrapping_mul(211).wrapping_add(f.end);
            h = h.wrapping_mul(211).wrapping_add(f.unwind);
        }
        h
    }

    /// 诊断面：目录摘要。
    pub fn summary(&self) -> String {
        format!("entries={}|sealed={}", self.funcs.len(), self.sealed as u32)
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut d = PdataDir::new();
    let _ = d.load(RtFunc { begin: 0x1000, end: 0x1100, unwind: 0x8000 });
    // 1) 空区间：Begin == End → PD_EMPTY_RANGE。
    got[0] =
        d.load(RtFunc { begin: 0x1200, end: 0x1200, unwind: 0x8100 }).err()
            == Some(PD_EMPTY_RANGE);
    // 2) 零 Unwind 悬挂 → PD_ZERO_UNWIND。
    got[1] =
        d.load(RtFunc { begin: 0x1200, end: 0x1300, unwind: 0 }).err()
            == Some(PD_ZERO_UNWIND);
    // 3) 区间重叠：Begin 落在上一条 End 内 → PD_OVERLAP。
    got[2] =
        d.load(RtFunc { begin: 0x1080, end: 0x1400, unwind: 0x8200 }).err()
            == Some(PD_OVERLAP);
    // 4) 乱序：Begin 小于已装载最小 Begin 但不重叠（越前回插）→ PD_UNSORTED。
    let mut e = PdataDir::new();
    let _ = e.load(RtFunc { begin: 0x2000, end: 0x2100, unwind: 0x9000 });
    let _ = e.load(RtFunc { begin: 0x2100, end: 0x2200, unwind: 0x9100 });
    got[3] =
        e.load(RtFunc { begin: 0x1800, end: 0x1900, unwind: 0x9200 }).err()
            == Some(PD_UNSORTED);
    // 5) 印账后仍装载 → PD_FROZEN。
    d.seal();
    got[4] =
        d.load(RtFunc { begin: 0x1400, end: 0x1500, unwind: 0x8300 }).err()
            == Some(PD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12007_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12007");

    // J1 正路径：三函数目录装载 + 有序无重叠复核。
    let j1 = {
        let mut d = PdataDir::new();
        let _ = d.load(RtFunc { begin: 0x1000, end: 0x1100, unwind: 0x8000 });
        let _ = d.load(RtFunc { begin: 0x1100, end: 0x1200, unwind: 0x8100 });
        let _ = d.load(RtFunc { begin: 0x1200, end: 0x1300, unwind: 0x8200 });
        d.funcs.len() == 3 && d.sorted_no_overlap() && d.begins_monotonic()
    };
    s.add("J1-目录正路径", j1, "三函数装载有序");

    // J2 RIP 查找命中唯一条目：区间内各点命中同条。
    let j2 = {
        let mut d = PdataDir::new();
        let _ = d.load(RtFunc { begin: 0x1000, end: 0x1100, unwind: 0x8000 });
        let _ = d.load(RtFunc { begin: 0x1100, end: 0x1200, unwind: 0x8100 });
        d.lookup(0x1000) == Some(0)
            && d.lookup(0x10FF) == Some(0)
            && d.lookup(0x1100) == Some(1)
            && d.lookup(0x1199) == Some(1)
    };
    s.add("J2-查找命中", j2, "区间内外命中点对账");

    // J3 查无命中返回空（合法）。
    let j3 = {
        let mut d = PdataDir::new();
        let _ = d.load(RtFunc { begin: 0x1000, end: 0x1100, unwind: 0x8000 });
        d.lookup(0x0FFF).is_none() && d.lookup(0x1100).is_none()
    };
    s.add("J3-查空合法", j3, "目录外 RIP 返回空");

    // J4 空目录查找恒空。
    let j4 = { PdataDir::new().lookup(0x1000).is_none() };
    s.add("J4-空目录空查", j4, "零条目恒空");

    // J5 空区间拒。
    let j5 = {
        let mut d = PdataDir::new();
        d.load(RtFunc { begin: 0x2000, end: 0x2000, unwind: 0x8000 }).err()
            == Some(PD_EMPTY_RANGE)
    };
    s.add("J5-空区间拒", j5, "Begin==End 必拒");

    // J6 倒区间拒：Begin > End。
    let j6 = {
        let mut d = PdataDir::new();
        d.load(RtFunc { begin: 0x2100, end: 0x2000, unwind: 0x8000 }).err()
            == Some(PD_EMPTY_RANGE)
    };
    s.add("J6-倒区间拒", j6, "Begin>End 必拒");

    // J7 零 Unwind 拒。
    let j7 = {
        let mut d = PdataDir::new();
        d.load(RtFunc { begin: 0x2000, end: 0x2100, unwind: 0 }).err()
            == Some(PD_ZERO_UNWIND)
    };
    s.add("J7-零Unwind拒", j7, "展开信息悬挂必拒");

    // J8 区间重叠拒。
    let j8 = {
        let mut d = PdataDir::new();
        let _ = d.load(RtFunc { begin: 0x2000, end: 0x2100, unwind: 0x8000 });
        d.load(RtFunc { begin: 0x2080, end: 0x2200, unwind: 0x8100 }).err()
            == Some(PD_OVERLAP)
    };
    s.add("J8-重叠拒", j8, "区间互斥");

    // J9 乱序拒：不重叠但 Begin 回插。
    let j9 = {
        let mut d = PdataDir::new();
        let _ = d.load(RtFunc { begin: 0x2000, end: 0x2100, unwind: 0x8000 });
        let _ = d.load(RtFunc { begin: 0x2100, end: 0x2200, unwind: 0x8100 });
        d.load(RtFunc { begin: 0x1900, end: 0x1A00, unwind: 0x8200 }).err()
            == Some(PD_UNSORTED)
    };
    s.add("J9-乱序拒", j9, "回插必拒");

    // J10 指纹区分度与幂等。
    let j10 = {
        let mut a = PdataDir::new();
        let _ = a.load(RtFunc { begin: 1, end: 2, unwind: 3 });
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = PdataDir::new();
        let _ = b.load(RtFunc { begin: 1, end: 2, unwind: 4 });
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J10-指纹性质", j10, "幂等且异目录异纹");

    // J11 印账冻结。
    let j11 = {
        let mut d = PdataDir::new();
        let _ = d.load(RtFunc { begin: 1, end: 2, unwind: 3 });
        d.seal();
        d.load(RtFunc { begin: 2, end: 3, unwind: 4 }).err() == Some(PD_FROZEN)
    };
    s.add("J11-印账冻结", j11, "印后装载必拒");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例：十函数目录（等宽 0x100）。
    let mut d = PdataDir::new();
    let mut ok = true;
    for k in 0..10u32 {
        let b = 0x1000 + k * 0x100;
        if d.load(RtFunc { begin: b, end: b + 0x100, unwind: 0x8000 + k }).is_err() {
            ok = false;
        }
    }
    s.add("样例十函数目录", ok && d.funcs.len() == 10, "等宽目录全收");
    s.add("样例中段查找", d.lookup(0x1550) == Some(5), "第 6 条命中");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12007_lookup_edges() {
        let mut d = PdataDir::new();
        assert!(d.load(RtFunc { begin: 0x1000, end: 0x1100, unwind: 1 }).is_ok());
        assert!(d.load(RtFunc { begin: 0x1100, end: 0x1200, unwind: 2 }).is_ok());
        assert_eq!(d.lookup(0x1000), Some(0));
        assert_eq!(d.lookup(0x10FF), Some(0));
        assert_eq!(d.lookup(0x1100), Some(1));
        assert_eq!(d.lookup(0x1200), None);
        // 回插样本既乱序又重叠：乱序守卫（先判）优先命中。
        assert_eq!(d.load(RtFunc { begin: 0x1000, end: 0x1050, unwind: 3 }), Err(PD_UNSORTED));
    }

    #[test]
    fn unx12007_checks_green() {
        let s = run_unx12007_checks();
        assert!(s.all_passed(), "UNX-F12007 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12007_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
