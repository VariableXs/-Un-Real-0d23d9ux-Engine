//! UNX-F11206 版本表数据结构与查询接口（0x1f806 段）
//!
//! 锚点：UNX-F11206-J1 `{glibc_ver, kernel_baseline, usable, known_gaps[]}`
//! 四字段结构落账，查询接口对未知版本返回显式「未登记」而非默认可用。
//! 域：CoRun·UNX-C5-B01——版本号打包为单字（主<<16|次），记录按包值升序
//! 存放，查询走二分；未命中返回 Unregistered 枚举分支，调用方无法误读。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f806;
pub const VQ_UNREG: u32 = SEG_BASE + 0x01; // 未登记版本
pub const VQ_DUP: u32 = SEG_BASE + 0x02; // 同版本重录
pub const VQ_GAP: u32 = SEG_BASE + 0x03; // 缺口号零值
pub const VQ_BASE: u32 = SEG_BASE + 0x04; // 内核基线零值
pub const VQ_ORDER: u32 = SEG_BASE + 0x05; // 版本号倒序
pub const VQ_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后写表
pub const VQ_OVER: u32 = SEG_BASE + 0x07; // 超容

pub const MAX_ROWS: usize = 64;
pub const VQ_TAG: u64 = 0x1f806c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 版本打包。
pub fn pack(major: u32, minor: u32) -> u32 {
    (major << 16) | (minor & 0xffff)
}

pub fn unpack(v: u32) -> (u32, u32) {
    (v >> 16, v & 0xffff)
}

/// 四字段记录。
#[derive(Clone, Debug)]
pub struct VerRec {
    pub ver: u32,
    pub kernel_baseline: u32,
    pub usable: bool,
    pub gaps: Vec<u32>,
}

impl VerRec {
    pub fn label(&self) -> String {
        let (a, b) = unpack(self.ver);
        format!("{}.{}", a, b)
    }

    pub fn has_gaps(&self) -> bool {
        !self.gaps.is_empty()
    }

    pub fn gap_sum(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.gaps.len() {
            t = t.saturating_add(self.gaps[k]);
            k += 1;
        }
        t
    }
}

/// 查询结果：显式两分支，未登记不落到「默认可用」。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lookup {
    Hit(usize),
    Unregistered,
}

/// 版本表：升序记录 + 二分查询。
#[derive(Clone, Debug)]
pub struct VerTable {
    pub recs: Vec<VerRec>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl VerTable {
    pub fn new() -> VerTable {
        VerTable { recs: Vec::new(), frozen: false, root: None }
    }

    /// 录入：冻结/基线零/缺口零/重号/超容/倒序六路拒。
    pub fn insert(&mut self, major: u32, minor: u32, baseline: u32, usable: bool, gaps: &[u32]) -> Result<(), u32> {
        if self.frozen {
            return Err(VQ_FROZEN);
        }
        if baseline == 0 {
            return Err(VQ_BASE);
        }
        let v = pack(major, minor);
        let mut k = 0usize;
        while k < gaps.len() {
            if gaps[k] == 0 {
                return Err(VQ_GAP);
            }
            k += 1;
        }
        if self.recs.iter().any(|r| r.ver == v) {
            return Err(VQ_DUP);
        }
        if self.recs.len() >= MAX_ROWS {
            return Err(VQ_OVER);
        }
        let mut g: Vec<u32> = Vec::new();
        k = 0;
        while k < gaps.len() {
            g.push(gaps[k]);
            k += 1;
        }
        let rec = VerRec { ver: v, kernel_baseline: baseline, usable, gaps: g };
        let pos = self.lower_bound(v);
        self.recs.insert(pos, rec);
        Ok(())
    }

    fn lower_bound(&self, v: u32) -> usize {
        let mut lo = 0usize;
        let mut hi = self.recs.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.recs[mid].ver < v {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// 二分查询：返回显式分支。
    pub fn find(&self, major: u32, minor: u32) -> Lookup {
        let v = pack(major, minor);
        let mut lo = 0usize;
        let mut hi = self.recs.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.recs[mid].ver == v {
                return Lookup::Hit(mid);
            }
            if self.recs[mid].ver < v {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Lookup::Unregistered
    }

    /// 可用判定：未登记改报错误而非 false/true 默认。
    pub fn usable(&self, major: u32, minor: u32) -> Result<bool, u32> {
        match self.find(major, minor) {
            Lookup::Hit(i) => Ok(self.recs[i].usable),
            Lookup::Unregistered => Err(VQ_UNREG),
        }
    }

    /// 内核基线：未登记同样显式报错。
    pub fn baseline(&self, major: u32, minor: u32) -> Result<u32, u32> {
        match self.find(major, minor) {
            Lookup::Hit(i) => Ok(self.recs[i].kernel_baseline),
            Lookup::Unregistered => Err(VQ_UNREG),
        }
    }

    /// 缺口号：未登记报错，登记无缺口返回空表。
    pub fn gaps(&self, major: u32, minor: u32) -> Result<Vec<u32>, u32> {
        match self.find(major, minor) {
            Lookup::Hit(i) => Ok(self.recs[i].gaps.clone()),
            Lookup::Unregistered => Err(VQ_UNREG),
        }
    }

    /// 升序不变式。
    pub fn sorted(&self) -> bool {
        let mut k = 1usize;
        while k < self.recs.len() {
            if self.recs[k - 1].ver >= self.recs[k].ver {
                return false;
            }
            k += 1;
        }
        true
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = VQ_TAG;
        let mut k = 0usize;
        while k < self.recs.len() {
            h = (h ^ self.recs[k].ver as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.recs[k].kernel_baseline as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.recs[k].usable as u64).wrapping_mul(FP_STEP);
            let mut j = 0usize;
            while j < self.recs[k].gaps.len() {
                h = (h ^ self.recs[k].gaps[j] as u64).wrapping_mul(FP_STEP);
                j += 1;
            }
            k += 1;
        }
        h
    }

    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 建表：乱序录入 6 行（验证升序落位），其中两行带缺口。
pub fn build_table() -> (VerTable, bool) {
    let mut t = VerTable::new();
    let mut ok = true;
    let rows: [(u32, u32, u32, bool, [u32; 2], usize); 6] = [
        (2, 34, 6, true, [0, 0], 0),
        (2, 17, 4, true, [0, 0], 0),
        (2, 39, 7, true, [12013, 0], 1),
        (2, 28, 5, true, [0, 0], 0),
        (2, 31, 6, true, [12014, 0], 1),
        (2, 2, 3, true, [0, 0], 0),
    ];
    let mut k = 0usize;
    while k < rows.len() {
        let r = rows[k];
        let gaps = &r.4[..r.5];
        if t.insert(r.0, r.1, r.2, r.3, gaps).is_err() {
            ok = false;
        }
        k += 1;
    }
    let good = ok && t.sorted() && t.recs.len() == 6;
    (t, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let (t, _) = build_table();
    // 1) 未知版本显式未登记（可用判定与基线查询同报）。
    got[0] = t.find(2, 99) == Lookup::Unregistered
        && t.usable(2, 99).err() == Some(VQ_UNREG)
        && t.baseline(2, 99).err() == Some(VQ_UNREG);
    // 2) 缺口号零值拒。
    let mut a = VerTable::new();
    got[1] = a.insert(2, 40, 6, true, &[0]).err() == Some(VQ_GAP);
    // 3) 基线零值拒。
    got[2] = a.insert(2, 40, 0, true, &[]).err() == Some(VQ_BASE);
    // 4) 重号与冻结两路拒。
    let mut b = VerTable::new();
    let _ = b.insert(2, 17, 4, true, &[]);
    let dup = b.insert(2, 17, 4, false, &[]).err() == Some(VQ_DUP);
    b.seal();
    got[3] = dup && b.insert(2, 18, 4, true, &[]).err() == Some(VQ_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx11206_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11206");
    let (mut t, built) = build_table();

    s.add("J1-四字段结构", built && t.recs.iter().all(|r| r.kernel_baseline > 0),
        "六行记录四字段齐备且基线非零");
    s.add("J2-升序不变式", t.sorted(), "乱序录入后仍严格升序");
    s.add("J3-二分命中", t.find(2, 17) == Lookup::Hit(1) && t.find(2, 39) == Lookup::Hit(5),
        "二分查询首尾命中下标正确");
    s.add("J4-未知未登记", t.find(3, 0) == Lookup::Unregistered, "未知版本返回 Unregistered 分支");
    s.add("J5-可用不默认", t.usable(9, 9).err() == Some(VQ_UNREG), "可用判定不落默认值");
    s.add("J6-基线可查", t.baseline(2, 28) == Ok(5) && t.baseline(2, 2) == Ok(3),
        "内核基线按版本精确回读");

    let g39 = t.gaps(2, 39);
    s.add("J7-缺口回读", g39 == Ok(vec![12013]) && t.gaps(2, 17) == Ok(vec![]),
        "缺口表按版本回读（有缺口与空缺口两态）");
    s.add("J8-缺口合计", t.recs.iter().filter(|r| r.has_gaps()).count() == 2,
        "带缺口行恰两行");
    s.add("J9-重录拒", {
        let mut c = VerTable::new();
        let _ = c.insert(2, 30, 6, true, &[]);
        c.insert(2, 30, 6, true, &[]).err() == Some(VQ_DUP)
    }, "同版本重录必拒");

    let inj = inject_four();
    s.add("J10-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "未登记/零缺口/零基线/重录冻结四路必抓");

    let root = t.seal();
    s.add("J11-封表冻结", t.fold() == root && root != 0 && t.insert(2, 40, 6, true, &[]).err() == Some(VQ_FROZEN),
        "封表后可重算指纹且拒绝再录");

    s.add("J12-打包往返", unpack(pack(2, 39)) == (2, 39) && pack(2, 39) > pack(2, 34),
        "版本打包与解包往返一致且序可比");
    s.add("J13-标签渲染", t.recs[5].label() == "2.39" && t.recs[0].label() == "2.2",
        "版本标签渲染与下标对应");

    let fp = t.fold();
    s.add_dyn("J14-查表明细", t.recs.len() == 6 && fp == root, &format!("rows={} fp={:#x}", t.recs.len(), fp));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11206_table_flow() {
        let (mut t, built) = build_table();
        assert!(built);
        assert_eq!(t.find(2, 34), Lookup::Hit(4));
        assert_eq!(t.find(4, 0), Lookup::Unregistered);
        assert_eq!(t.usable(2, 31), Ok(true));
        assert_eq!(t.gaps(2, 31), Ok(vec![12014]));
        assert_eq!(t.baseline(2, 99), Err(VQ_UNREG));
        let root = t.seal();
        assert_eq!(t.fold(), root);
        assert!(t.insert(2, 50, 8, true, &[]).is_err());
        assert_eq!(pack(2, 2), 0x0002_0002);
    }

    #[test]
    fn unx11206_checks_green() {
        let s = run_unx11206_checks();
        assert!(s.all_passed(), "UNX-F11206 判据未全绿");
    }
}
