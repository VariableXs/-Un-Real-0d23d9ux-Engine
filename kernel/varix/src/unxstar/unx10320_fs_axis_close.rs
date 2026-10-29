//! UNX-F10320 号面 fs 联签轴闭账（十九项对平+哈希冻结）（0x1b280 段）
//!
//! 锚点：UNX-F10320-J1 fs 轴闭账——按 VFS/路径/文件三面配额（7/6/6）
//! 逐项对平且轴根哈希冻结，配额漂移或值偏离即闭不了账。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：轴闭账册（轴项：项号+面号+双值，面配额独立核）+ 对平核
//! （逐项双值相等且对平位翻齐）+ 轴根冻结器（面号入链的 u64 根，
//! 冻结后写轴即拒）+ 注入核（坏面号/值偏离/配额漂移/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b280 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b280;
pub const AX_DUP: u32 = SEG_BASE + 0x01; // 项号重号
pub const AX_BADPLANE: u32 = SEG_BASE + 0x02; // 面号非法
pub const AX_CAP: u32 = SEG_BASE + 0x03; // 册超容
pub const AX_NONO: u32 = SEG_BASE + 0x04; // 未知项对平
pub const AX_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写轴
pub const AX_QUOTA: u32 = SEG_BASE + 0x06; // 面配额漂移
pub const AX_VALUE: u32 = SEG_BASE + 0x07; // 双值偏离

/// 面号：1=VFS 2=路径 3=文件。
pub const PL_VFS: u8 = 1;
pub const PL_PATH: u8 = 2;
pub const PL_FILE: u8 = 3;

/// 面配额：VFS 7 / 路径 6 / 文件 6 = 19。
pub const QUOTA_VFS: usize = 7;
pub const QUOTA_PATH: usize = 6;
pub const QUOTA_FILE: usize = 6;
/// 轴项总数锚。
pub const ITEMS_NEEDED: usize = QUOTA_VFS + QUOTA_PATH + QUOTA_FILE;
/// 轴册定容。
pub const CAP: usize = 24;
/// 轴根种子。
pub const AXIS_TAG: u64 = 0x1b280c3;

/// 单条轴项。
#[derive(Clone)]
pub struct AxisItem {
    pub no: u32,
    pub plane: u8,
    pub va: u32,
    pub vb: u32,
    pub settled: bool,
}

impl AxisItem {
    pub fn agree(&self) -> bool {
        self.va == self.vb
    }
}

/// fs 联签轴闭账册。
pub struct AxisClose {
    pub items: Vec<AxisItem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl AxisClose {
    pub fn new() -> AxisClose {
        AxisClose { items: Vec::new(), frozen: false, root: None }
    }

    fn plane_ok(plane: u8) -> bool {
        plane == PL_VFS || plane == PL_PATH || plane == PL_FILE
    }

    /// 登记轴项：冻结、坏面、零号、重号、超容均拒。
    pub fn enroll(&mut self, no: u32, plane: u8, va: u32, vb: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(AX_FROZEN);
        }
        if !AxisClose::plane_ok(plane) {
            return Err(AX_BADPLANE);
        }
        if no == 0 || self.items.iter().any(|i| i.no == no) {
            return Err(AX_DUP);
        }
        if self.items.len() >= CAP {
            return Err(AX_CAP);
        }
        self.items.push(AxisItem { no, plane, va, vb, settled: false });
        Ok(())
    }

    /// 对平单项：翻位并回报双值是否一致。
    pub fn settle(&mut self, no: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(AX_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].no == no {
                self.items[k].settled = true;
                return Ok(self.items[k].agree());
            }
            k += 1;
        }
        Err(AX_NONO)
    }

    /// 面内项数。
    pub fn plane_count(&self, plane: u8) -> usize {
        self.items.iter().filter(|i| i.plane == plane).count()
    }

    /// 面配额核：三面配额恰等。
    pub fn quotas_ok(&self) -> bool {
        self.plane_count(PL_VFS) == QUOTA_VFS
            && self.plane_count(PL_PATH) == QUOTA_PATH
            && self.plane_count(PL_FILE) == QUOTA_FILE
    }

    /// 值偏离项清单（已对平但不等）。
    pub fn deviated(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].settled && !self.items[k].agree() {
                v.push(self.items[k].no);
            }
            k += 1;
        }
        v
    }

    /// 未对平项清单。
    pub fn unsettled(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].settled {
                v.push(self.items[k].no);
            }
            k += 1;
        }
        v
    }

    /// 闭账裁断：配额齐、全对平、零偏离。
    pub fn closed(&self) -> bool {
        self.items.len() == ITEMS_NEEDED
            && self.quotas_ok()
            && self.unsettled().is_empty()
            && self.deviated().is_empty()
    }

    /// 轴根哈希（面号入链 FNV 风格）。
    pub fn axis_root(&self) -> u64 {
        let mut h: u64 = AXIS_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = (h ^ it.no as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.plane as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.va as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.vb as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.settled as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 哈希冻结：翻冻结位并落轴根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.axis_root();
        self.root = Some(r);
        r
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut a = AxisClose::new();
    // 1) 坏面号必拒
    got[0] = a.enroll(1, 9, 1, 1).err() == Some(AX_BADPLANE);
    // 2) 值偏离必被对平判定抓出
    let _ = a.enroll(1, PL_VFS, 10, 11);
    got[1] = a.settle(1) == Ok(false) && a.deviated() == [1u32];
    // 3) 配额漂移（VFS 少一条）必判闭账失败
    let mut q = AxisClose::new();
    let mut n = 1u32;
    while n < QUOTA_VFS as u32 {
        let _ = q.enroll(n, PL_VFS, 5, 5);
        let _ = q.settle(n);
        n += 1;
    }
    let mut m = 1u32;
    while m <= QUOTA_PATH as u32 {
        let no = 100 + m;
        let _ = q.enroll(no, PL_PATH, 5, 5);
        let _ = q.settle(no);
        m += 1;
    }
    let mut p = 1u32;
    while p <= QUOTA_FILE as u32 {
        let no = 200 + p;
        let _ = q.enroll(no, PL_FILE, 5, 5);
        let _ = q.settle(no);
        p += 1;
    }
    got[2] = !q.closed() && q.plane_count(PL_VFS) == QUOTA_VFS - 1;
    // 4) 冻结后写轴必抓
    a.freeze();
    got[3] = a.enroll(2, PL_PATH, 1, 1).err() == Some(AX_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx10320_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10320");

    // J1 正路径：三面配额 7/6/6 逐项登记对平闭账
    let mut a = AxisClose::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= QUOTA_VFS as u32 {
        if a.enroll(n, PL_VFS, n * 10, n * 10).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1u32;
    while n <= QUOTA_PATH as u32 {
        let no = 100 + n;
        if a.enroll(no, PL_PATH, n * 10, n * 10).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1u32;
    while n <= QUOTA_FILE as u32 {
        let no = 200 + n;
        if a.enroll(no, PL_FILE, n * 10, n * 10).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1u32;
    while n <= (ITEMS_NEEDED * 3) as u32 {
        // 对平段只按已登记项号重放：1..=7、101..=106、201..=206
        let in_vfs = n >= 1 && n <= QUOTA_VFS as u32;
        let in_path = n >= 101 && n <= 100 + QUOTA_PATH as u32;
        let in_file = n >= 201 && n <= 200 + QUOTA_FILE as u32;
        if (in_vfs || in_path || in_file) && a.settle(n) != Ok(true) {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-三面配额闭账", ok && a.closed(), "7/6/6 配额逐项对平且闭账成立");

    // J2 轴根冻结：根有值且与 axis_root 一致、二次稳定
    let root = a.freeze();
    let j2 = a.root == Some(root) && a.axis_root() == root && root != 0;
    s.add("J2-轴根冻结", j2, "轴根落账且指纹稳定");

    // J3 冻结后写轴必抓（正册复现）
    let j3 = a.enroll(999, PL_VFS, 1, 1).err() == Some(AX_FROZEN)
        && a.settle(1).err() == Some(AX_FROZEN);
    s.add("J3-冻结拒写", j3, "冻结后登记与对平均拒");

    // J4 注入检出：坏面 + 值偏离
    let inj = inject_four();
    s.add("J4-坏面偏离检出", inj[0] && inj[1], "坏面号必拒且值偏离必抓");

    // J5 注入检出：配额漂移 + 冻结后写
    s.add("J5-配额冻结检出", inj[2] && inj[3], "配额漂移闭不了账且冻结后写必抓");

    // J6 未对平清单可枚举
    let j6 = {
        let mut c = AxisClose::new();
        let _ = c.enroll(1, PL_VFS, 1, 1);
        let _ = c.enroll(2, PL_VFS, 1, 1);
        let _ = c.settle(1);
        c.unsettled() == [2u32]
    };
    s.add("J6-未对平枚举", j6, "未对平项按登记序枚举");

    // J7 未知项对平拒
    let j7 = AxisClose::new().settle(9).err() == Some(AX_NONO);
    s.add("J7-未知项拒", j7, "对平不存在的项必拒");

    // J8 零号/重号拒
    let j8 = {
        let mut c = AxisClose::new();
        c.enroll(0, PL_VFS, 1, 1).err() == Some(AX_DUP)
            && {
                let _ = c.enroll(1, PL_VFS, 1, 1);
                c.enroll(1, PL_PATH, 1, 1).err() == Some(AX_DUP)
            }
    };
    s.add("J8-零号重号拒", j8, "零号与重号均按重号面拒");

    // J9 容量边界
    let j9 = {
        let mut c = AxisClose::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            if c.enroll(n, PL_VFS, n, n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(9999, PL_VFS, 1, 1).err() == Some(AX_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原册不损");

    // J10 面计数自洽：三面计数和恰为十九
    let j10 = a.plane_count(PL_VFS) == QUOTA_VFS
        && a.plane_count(PL_PATH) == QUOTA_PATH
        && a.plane_count(PL_FILE) == QUOTA_FILE
        && a.items.len() == ITEMS_NEEDED;
    s.add("J10-面计数自洽", j10, "7+6+6=19 计数对平");

    // J11 未冻结前闭账不齐不算成（对平位缺一）
    let j11 = {
        let mut c = AxisClose::new();
        let mut n = 1u32;
        while n <= QUOTA_VFS as u32 {
            let _ = c.enroll(n, PL_VFS, n, n);
            let _ = c.settle(n);
            n += 1;
        }
        !c.closed()
    };
    s.add("J11-缺面不成闭", j11, "只闭一面即闭账失败");

    // J12 动态明细：轴根十六进制落账
    let fp = a.axis_root();
    s.add_dyn("J12-轴根明细", fp != 0, &format!("root={:#x}", fp));

    // J13 动态明细：双零清单（未对平+偏离）
    let u = a.unsettled().len();
    let d = a.deviated().len();
    s.add_dyn("J13-双零清单", u == 0 && d == 0, &format!("unsettled={} deviated={}", u, d));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10320_axis_flow() {
        let mut a = AxisClose::new();
        assert!(a.enroll(1, 9, 1, 1).err() == Some(AX_BADPLANE));
        assert!(a.enroll(1, PL_VFS, 5, 5).is_ok());
        assert!(a.enroll(1, PL_PATH, 5, 5).err() == Some(AX_DUP));
        assert!(a.enroll(0, PL_VFS, 1, 1).err() == Some(AX_DUP));
        assert!(a.settle(9).err() == Some(AX_NONO));
        assert_eq!(a.settle(1), Ok(true));
        assert!(a.enroll(2, PL_VFS, 5, 6).is_ok());
        assert_eq!(a.settle(2), Ok(false));
        assert_eq!(a.deviated(), [2u32]);
        a.freeze();
        assert!(a.enroll(3, PL_PATH, 1, 1).err() == Some(AX_FROZEN));
        assert!(a.settle(1).err() == Some(AX_FROZEN));
        assert!(a.root.is_some());
    }

    #[test]
    fn unx10320_checks_green() {
        let s = run_unx10320_checks();
        assert!(s.all_passed(), "UNX-F10320 判据未全绿");
    }
}
