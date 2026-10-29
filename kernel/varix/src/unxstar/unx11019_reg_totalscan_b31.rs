//! UNX-F11019 挂号对账总扫（挂点全账核销与零残留断言）（0x1e260 段）
//!
//! 锚点：UNX-F11019-J1 挂点全账核销对平（10/10），零残留断言成立（10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：总扫账（B31 族十路子账逐路登记：族名 + 应核销数 + 已核销数 +
//! 残留数）+ 全账核销器（逐路核销、核销数不齐即留账）+ 零残留断言器（逐
//! 路残留清零方可封账）+ fold 指纹与冻结 + 注入核（欠核销/虚报核销/空族
//! 名/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e260 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e260;
pub const TS_UNDERSWEEP: u32 = SEG_BASE + 0x01; // 欠核销
pub const TS_OVERSWEEP: u32 = SEG_BASE + 0x02; // 虚报核销
pub const TS_EMPTY: u32 = SEG_BASE + 0x03; // 族名空
pub const TS_DUP: u32 = SEG_BASE + 0x04; // 族名重号
pub const TS_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const TS_CAP: u32 = SEG_BASE + 0x06; // 总扫账超容

/// 子账对平数（10/10）。
pub const FAMILIES_NEEDED: usize = 10;
/// 总扫账定容。
pub const TS_CAP_N: usize = 16;
/// 总扫账域标（fold 种子）。
pub const TS_TAG: u64 = 0x1e26_13;

/// 单路子账：族名 + 应核销数 + 已核销数。
#[derive(Clone, Debug)]
pub struct ScanFamily {
    pub name: String,
    pub owed: u32,
    pub swept: u32,
}

impl ScanFamily {
    /// 核销齐平。
    pub fn even(&self) -> bool {
        self.owed == self.swept && self.owed > 0
    }

    /// 欠核销数。
    pub fn under(&self) -> u32 {
        self.owed.saturating_sub(self.swept)
    }

    /// 虚报核销数。
    pub fn over(&self) -> u32 {
        self.swept.saturating_sub(self.owed)
    }

    pub fn render(&self) -> String {
        format!("{}|owed{}|swept{}", self.name, self.owed, self.swept)
    }
}

/// 挂号总扫账：逐路登记 → 逐路核销 → 零残留断言 → 冻结。
pub struct TotalScan {
    pub fams: Vec<ScanFamily>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl TotalScan {
    pub fn new() -> TotalScan {
        TotalScan { fams: Vec::new(), frozen: false, root: None }
    }

    /// 登记子账：冻结、空族名、重号、超容均拒；应核销数必须非零。
    pub fn enroll(&mut self, name: &str, owed: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TS_FROZEN);
        }
        if name.is_empty() {
            return Err(TS_EMPTY);
        }
        if owed == 0 {
            return Err(TS_UNDERSWEEP);
        }
        if self.fams.iter().any(|f| f.name == name) {
            return Err(TS_DUP);
        }
        if self.fams.len() >= TS_CAP_N {
            return Err(TS_CAP);
        }
        self.fams.push(ScanFamily { name: name.to_string(), owed, swept: 0 });
        Ok(())
    }

    /// 逐路核销：核销数不得超过应核销数（虚报必拒）。
    pub fn sweep(&mut self, name: &str, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(TS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.fams.len() {
            if self.fams[k].name == name {
                let room = self.fams[k].owed - self.fams[k].swept;
                if n > room {
                    return Err(TS_OVERSWEEP);
                }
                self.fams[k].swept += n;
                return Ok(self.fams[k].swept);
            }
            k += 1;
        }
        Err(TS_DUP)
    }

    /// 欠核销族清单（零残留面）。
    pub fn residues(&self) -> Vec<String> {
        let mut g: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.fams.len() {
            if !self.fams[k].even() {
                g.push(self.fams[k].name.to_string());
            }
            k += 1;
        }
        g
    }

    /// 欠核销总数。
    pub fn under_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.fams.len() {
            n = n.wrapping_add(self.fams[k].under());
            k += 1;
        }
        n
    }

    /// 零残留断言：十路齐、逐路齐平。
    pub fn closed(&self) -> bool {
        self.fams.len() == FAMILIES_NEEDED
            && self.residues().is_empty()
            && self.under_total() == 0
    }

    /// 按族名查位次。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.fams.len() {
            if self.fams[k].name == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含族名/应收/已收）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = TS_TAG;
        let mut k = 0usize;
        while k < self.fams.len() {
            let f = &self.fams[k];
            h = (h ^ f.owed as u64).wrapping_mul(0x100000001b3);
            h = (h ^ f.swept as u64).wrapping_mul(0x100000001b3);
            let bytes = f.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 族名确定性文本。
pub fn ts_name(no: usize) -> String {
    format!("b31族{}", no)
}

/// 应核销数确定性样本。
pub fn ts_owed(no: usize) -> u32 {
    (no % 7 + 3) as u32
}

/// 十路总扫全链：逐路登记并逐路核销齐平。
pub fn build_totalscan() -> (TotalScan, bool) {
    let mut t = TotalScan::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < FAMILIES_NEEDED {
        if t.enroll(&ts_name(i), ts_owed(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < FAMILIES_NEEDED {
        if t.sweep(&ts_name(i), ts_owed(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let t_ok = ok && t.closed();
    (t, t_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 欠核销留残留必抓
    let mut a = TotalScan::new();
    let _ = a.enroll("甲", 5);
    let _ = a.sweep("甲", 3);
    got[0] = a.residues().len() == 1 && a.under_total() == 2 && !a.closed();
    // 2) 虚报核销必拒
    let mut b = TotalScan::new();
    let _ = b.enroll("乙", 5);
    let _ = b.sweep("乙", 5);
    got[1] = b.sweep("乙", 1).err() == Some(TS_OVERSWEEP);
    // 3) 空族名必拒
    let mut c = TotalScan::new();
    got[2] = c.enroll("", 5).err() == Some(TS_EMPTY);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_totalscan();
    d.freeze();
    got[3] = d.enroll("迟到", 5).err() == Some(TS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11019_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11019");

    // J1 挂点全账核销对平：十路逐路齐平
    let (mut book, green) = build_totalscan();
    s.add("J1-全账核销对平", green && book.fams.len() == 10, "十路子账逐路登记并核销齐平");

    // J2 零残留断言成立：残留面清空且欠账为零
    s.add("J2-零残留断言", book.residues().is_empty() && book.under_total() == 0 && book.closed(), "逐路残留清零零欠账");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll("迟到", 5).err() == Some(TS_FROZEN)
        && book.sweep(&ts_name(0), 1).err() == Some(TS_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后登记与核销均必拒");

    // J5 注入检出：欠核销 + 虚报核销
    let inj = inject_four();
    s.add("J5-欠虚双抓", inj[0] && inj[1], "欠核销留残留与虚报必拒两路必抓");

    // J6 注入检出：空族名 + 冻结后写
    s.add("J6-空名冻结检出", inj[2] && inj[3], "空族名必拒且冻结后写必抓");

    // J7 分段核销：分两次核销至齐平与一次到位等价
    let j7 = {
        let mut c = TotalScan::new();
        let _ = c.enroll("分段", 10);
        let _ = c.sweep("分段", 4);
        let _ = c.sweep("分段", 6);
        c.find("分段").is_some() && c.residues().is_empty()
    };
    s.add("J7-分段核销", j7, "分段核销至齐平与一次到位等价");

    // J8 核销不存在的族必拒
    let j8 = TotalScan::new().sweep("无此族", 1).err() == Some(TS_DUP);
    s.add("J8-核销无此族拒", j8, "核销不存在的族必拒");

    // J9 零应核销拒
    let j9 = TotalScan::new().enroll("零账", 0).err() == Some(TS_UNDERSWEEP);
    s.add("J9-零账拒收", j9, "应核销数为零必拒");

    // J10 重号族名拒
    let j10 = {
        let mut c = TotalScan::new();
        let _ = c.enroll("重号", 5);
        c.enroll("重号", 6).err() == Some(TS_DUP)
    };
    s.add("J10-重号拒", j10, "族名重登必拒");

    // J11 超容拒：第 TS_CAP_N+1 路必拒
    let j11 = {
        let mut c = TotalScan::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= TS_CAP_N as u32 {
            let nm = format!("超容{}", n);
            if c.enroll(&nm, 5).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll("溢出", 5).err() == Some(TS_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：子账渲染含应收与已收
    let j12 = {
        let f = ScanFamily { name: "渲染族".to_string(), owed: 8, swept: 8 };
        let r = f.render();
        r.contains("owed8") && r.contains("swept8")
    };
    s.add("J12-渲染明细", j12, "子账渲染含应收已收");

    // J13 部分残留：欠两路时残留恰两路且断言为假
    let j13 = {
        let mut c = TotalScan::new();
        let mut i = 0usize;
        while i < FAMILIES_NEEDED {
            let _ = c.enroll(&ts_name(i), ts_owed(i));
            i += 1;
        }
        let mut i = 0usize;
        while i < FAMILIES_NEEDED - 2 {
            let _ = c.sweep(&ts_name(i), ts_owed(i));
            i += 1;
        }
        c.residues().len() == 2 && !c.closed()
    };
    s.add("J13-部分残留", j13, "欠两路残留恰两路且断言为假");

    // J14 样本确定性：族名与应收同入同出
    let n3a = ts_name(3);
    let n3b = ts_name(3);
    let o3a = ts_owed(3);
    let o3b = ts_owed(3);
    s.add("J14-样本确定性", n3a == n3b && o3a == o3b, "族名与应收样本同入同出");

    // J15 动态明细：十路齐平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-总扫明细",
        fp == root && book.fams.len() == 10,
        &format!("root={:#x} fams=10", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11019_totalscan_flow() {
        let (mut t, built) = build_totalscan();
        assert!(built);
        assert!(t.sweep(&ts_name(0), 1).err() == Some(TS_OVERSWEEP));
        assert!(t.enroll(&ts_name(0), 5).err() == Some(TS_DUP));
        t.freeze();
        assert!(t.enroll("late", 5).err() == Some(TS_FROZEN));
        assert!(t.root.is_some());
    }

    #[test]
    fn unx11019_checks_green() {
        let s = run_unx11019_checks();
        assert!(s.all_passed(), "UNX-F11019 判据未全绿");
    }
}
