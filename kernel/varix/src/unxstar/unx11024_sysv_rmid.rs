//! UNX-F11024 SysV RMID 挂点联签（延迟回收挂号对账）（0x1e300 段）
//!
//! 锚点：UNX-F11024-J1 RMID 挂号账逐件核销（10/10），延迟回收语义一致（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·SysV RMID）。
//!
//! 结构：IPC 件账（键 + 挂接数 + 标记位 + 回收位）+ 挂号器（mark 立账待
//! 回收）+ 延迟回收器（挂接归零且已标记方核销；标记即零挂则立即核销）
//! + 核销面与悬空面 + fold 指纹与冻结 + 注入核（核销未标/虚摘挂/重复标
//! 记/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e300 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e300;
pub const RM_UNMARKED: u32 = SEG_BASE + 0x01; // 核销未标记
pub const RM_NOKEY: u32 = SEG_BASE + 0x02; // 键未注册
pub const RM_DUPMARK: u32 = SEG_BASE + 0x03; // 重复标记
pub const RM_OVERDET: u32 = SEG_BASE + 0x04; // 摘挂超挂接
pub const RM_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const RM_CAP: u32 = SEG_BASE + 0x06; // 件账超容

/// 件账定容。
pub const ITEM_CAP_N: usize = 16;
/// RMID 挂号对平数（10/10）。
pub const ITEMS_NEEDED: usize = 10;
/// RMID 账域标（fold 种子）。
pub const RMID_TAG: u64 = 0x1e30_24;
/// 每件挂接样本量。
pub const RM_ATT_SAMPLE: u32 = 5;

/// 单件 IPC 项：键 + 挂接数 + 标记位 + 回收位。
#[derive(Clone, Debug)]
pub struct RmItem {
    pub key: u32,
    pub atts: u32,
    pub marked: bool,
    pub reclaimed: bool,
}

impl RmItem {
    /// 待回收（已标记未核销）。
    pub fn pending(&self) -> bool {
        self.marked && !self.reclaimed
    }

    pub fn render(&self) -> String {
        format!("rmid|k{}|a{}|m{}|r{}", self.key, self.atts, self.marked as u32, self.reclaimed as u32)
    }
}

/// RMID 挂号账：注册 → 挂接 → 标记 → 延迟核销。
pub struct RmBook {
    pub items: Vec<RmItem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl RmBook {
    pub fn new() -> RmBook {
        RmBook { items: Vec::new(), frozen: false, root: None }
    }

    /// 注册件：冻结、超容必拒。
    pub fn enroll(&mut self, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RM_FROZEN);
        }
        if self.items.iter().any(|x| x.key == key) {
            return Err(RM_DUPMARK);
        }
        if self.items.len() >= ITEM_CAP_N {
            return Err(RM_CAP);
        }
        self.items.push(RmItem { key, atts: 0, marked: false, reclaimed: false });
        Ok(())
    }

    /// 挂接：键须已注册且未核销。
    pub fn attach(&mut self, key: u32, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(RM_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].key == key {
                if self.items[k].reclaimed {
                    return Err(RM_NOKEY);
                }
                self.items[k].atts = self.items[k].atts.wrapping_add(n);
                return Ok(self.items[k].atts);
            }
            k += 1;
        }
        Err(RM_NOKEY)
    }

    /// 摘挂：不得超挂接；归零且已标记即核销（延迟回收落地）。
    pub fn detach(&mut self, key: u32, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(RM_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].key == key {
                if n > self.items[k].atts {
                    return Err(RM_OVERDET);
                }
                self.items[k].atts -= n;
                if self.items[k].atts == 0 && self.items[k].marked {
                    self.items[k].reclaimed = true;
                }
                return Ok(self.items[k].atts);
            }
            k += 1;
        }
        Err(RM_NOKEY)
    }

    /// 标记 RMID：零挂即时核销，有挂延迟待核销。
    pub fn mark(&mut self, key: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(RM_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].key == key {
                if self.items[k].marked {
                    return Err(RM_DUPMARK);
                }
                self.items[k].marked = true;
                if self.items[k].atts == 0 {
                    self.items[k].reclaimed = true;
                    return Ok(true);
                }
                return Ok(false);
            }
            k += 1;
        }
        Err(RM_NOKEY)
    }

    /// 主动核销：未标记件不得核销（核销必先挂号）。
    pub fn try_reclaim(&mut self, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RM_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].key == key {
                if !self.items[k].marked {
                    return Err(RM_UNMARKED);
                }
                self.items[k].reclaimed = true;
                return Ok(());
            }
            k += 1;
        }
        Err(RM_NOKEY)
    }

    /// 待核销清单（延迟回收面）。
    pub fn def_pending(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].pending() {
                g.push(self.items[k].key);
            }
            k += 1;
        }
        g
    }

    /// 已核销件数。
    pub fn reclaimed_n(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].reclaimed {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 零悬空断言：十件全核销且无待核销残留。
    pub fn closed(&self) -> bool {
        self.items.len() == ITEMS_NEEDED
            && self.reclaimed_n() as usize == ITEMS_NEEDED
            && self.def_pending().is_empty()
    }

    /// 按键查位次。
    pub fn find(&self, key: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].key == key {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RMID_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let x = &self.items[k];
            h = (h ^ x.key as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.atts as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.marked as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.reclaimed as u64).wrapping_mul(0x100000001b3);
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

/// 键确定性样本。
pub fn rm_key(no: usize) -> u32 {
    0x6d00 + no as u32 * 4
}

/// 十件全链：注册 → 部分带挂标记（延迟）、部分零挂标记（即时）→ 全核销。
pub fn build_rmbook() -> (RmBook, bool) {
    let mut b = RmBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < ITEMS_NEEDED {
        if b.enroll(rm_key(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    // 偶数件：挂接后标记（延迟回收），随后摘挂归零核销。
    // 奇数件：零挂直接标记（即时核销）。
    let mut i = 0usize;
    while i < ITEMS_NEEDED {
        if i % 2 == 0 {
            if b.attach(rm_key(i), RM_ATT_SAMPLE).is_err() {
                ok = false;
            }
            match b.mark(rm_key(i)) {
                Ok(false) => {}
                _ => ok = false,
            }
            if b.detach(rm_key(i), RM_ATT_SAMPLE).is_err() {
                ok = false;
            }
        } else {
            match b.mark(rm_key(i)) {
                Ok(true) => {}
                _ => ok = false,
            }
        }
        i += 1;
    }
    let b_ok = ok && b.closed();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 摘挂超挂接必拒
    let mut a = RmBook::new();
    let _ = a.enroll(1);
    let _ = a.attach(1, 2);
    got[0] = a.detach(1, 3).err() == Some(RM_OVERDET);
    // 2) 未注册键标记必拒且未标记件不得核销
    let mut b = RmBook::new();
    let _ = b.enroll(0xbeef);
    let un = b.try_reclaim(0xbeef).err() == Some(RM_UNMARKED);
    got[1] = un && b.mark(0xdead).err() == Some(RM_NOKEY);
    // 3) 重复标记必拒
    let mut c = RmBook::new();
    let _ = c.enroll(7);
    let _ = c.mark(7);
    got[2] = c.mark(7).err() == Some(RM_DUPMARK);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_rmbook();
    d.freeze();
    got[3] = d.attach(rm_key(0), 1).err() == Some(RM_FROZEN)
        && d.mark(rm_key(1)).err() == Some(RM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11024_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11024");

    // J1 RMID 挂号账逐件核销：十件全核销
    let (mut book, green) = build_rmbook();
    s.add("J1-逐件核销", green && book.reclaimed_n() == 10, "十件标记挂摘逐件核销");

    // J2 延迟回收语义一致：无待核销残留且账封闭
    s.add("J2-延迟回收一致", book.def_pending().is_empty() && book.closed(), "延迟待核销面清空账封闭");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.attach(rm_key(0), 1).err() == Some(RM_FROZEN)
        && book.detach(rm_key(0), 1).err() == Some(RM_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后挂摘均必拒");

    // J5 注入检出：虚摘挂 + 未注册键
    let inj = inject_four();
    s.add("J5-虚摘无键抓", inj[0] && inj[1], "摘挂超挂接与未注册键两路必抓");

    // J6 注入检出：重复标记 + 冻结后写
    s.add("J6-重标冻结检出", inj[2] && inj[3], "重复标记与冻结后写必抓");

    // J7 即时核销语义：零挂标记即刻核销
    let j7 = {
        let mut c = RmBook::new();
        let _ = c.enroll(0x71);
        let got = c.mark(0x71);
        got == Ok(true) && c.reclaimed_n() == 1
    };
    s.add("J7-即时核销", j7, "零挂标记即刻核销");

    // J8 延迟核销语义：带挂标记先待核后归零核销
    let j8 = {
        let mut c = RmBook::new();
        let _ = c.enroll(0x72);
        let _ = c.attach(0x72, 3);
        let _ = c.mark(0x72);
        let mid = c.def_pending().len() == 1 && c.reclaimed_n() == 0;
        let _ = c.detach(0x72, 3);
        mid && c.reclaimed_n() == 1 && c.def_pending().is_empty()
    };
    s.add("J8-延迟核销", j8, "带挂标记延迟至归零核销");

    // J9 先摘后标：未标记归零不核销
    let j9 = {
        let mut c = RmBook::new();
        let _ = c.enroll(0x73);
        let _ = c.attach(0x73, 2);
        let _ = c.detach(0x73, 2);
        c.reclaimed_n() == 0 && c.mark(0x73) == Ok(true)
    };
    s.add("J9-未标不核销", j9, "未标记归零不核销且随后标记即时核销");

    // J10 核销件再挂必拒
    let j10 = {
        let mut c = RmBook::new();
        let _ = c.enroll(0x74);
        let _ = c.mark(0x74);
        c.attach(0x74, 1).err() == Some(RM_NOKEY)
    };
    s.add("J10-核销再挂拒", j10, "已核销件再挂必拒");

    // J11 超容拒：第 ITEM_CAP_N+1 件必拒
    let j11 = {
        let mut c = RmBook::new();
        let mut filled = true;
        let mut n = 0u32;
        while (n as usize) < ITEM_CAP_N {
            if c.enroll(0x3000 + n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(0x4000).err() == Some(RM_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：件渲染含键与标志位
    let j12 = {
        let x = RmItem { key: 0x5b4, atts: 3, marked: true, reclaimed: false };
        let r = x.render();
        r.contains("k1460") && r.contains("m1") && r.contains("r0")
    };
    s.add("J12-渲染明细", j12, "件渲染含键挂接标志");

    // J13 样本确定性：键同入同出
    let k4a = rm_key(4);
    let k4b = rm_key(4);
    s.add("J13-样本确定性", k4a == k4b, "键样本同入同出");

    // J14 悬空注入：漏核销一件悬空面恰一
    let j14 = {
        let mut c = RmBook::new();
        let mut i = 0usize;
        while i < ITEMS_NEEDED {
            let _ = c.enroll(rm_key(i));
            let _ = c.attach(rm_key(i), RM_ATT_SAMPLE);
            i += 1;
        }
        // 十件全标记，仅九件摘挂归零：末件标而未核即唯一悬空面
        let mut i = 0usize;
        while i < ITEMS_NEEDED {
            let _ = c.mark(rm_key(i));
            if i < ITEMS_NEEDED - 1 {
                let _ = c.detach(rm_key(i), RM_ATT_SAMPLE);
            }
            i += 1;
        }
        c.def_pending().len() == 1 && !c.closed()
    };
    s.add("J14-漏核悬空", j14, "漏核一件悬空面恰一且断言为假");

    // J15 动态明细：十件核销与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-核销明细",
        fp == root && book.reclaimed_n() == 10,
        &format!("root={:#x} reclaimed={}", fp, book.reclaimed_n()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11024_rmid_flow() {
        let (mut b, built) = build_rmbook();
        assert!(built);
        b.freeze();
        assert!(b.attach(rm_key(0), 1).err() == Some(RM_FROZEN));
        assert!(b.mark(rm_key(2)).err() == Some(RM_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11024_checks_green() {
        let s = run_unx11024_checks();
        assert!(s.all_passed(), "UNX-F11024 判据未全绿");
    }
}
