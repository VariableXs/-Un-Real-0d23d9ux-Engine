//! UNX-F11036 五族回收挂点总扫（close/unlink/RMID 核销对账）（0x1e480 段）
//!
//! 锚点：UNX-F11036-J1 回收挂点逐族核销（10/10），三路回收零残留（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·回收总扫）。
//!
//! 结构：三路回收账（close/fd 路、unlink/名路、rmid/键路各一路子账：应
//! 收 + 已收 + 残留）+ 回收挂点（逐路核销、超额核销必拒）+ 总扫器（三路
//! 齐平方封账）+ fold 与冻结 + 注入核（超额核销/空路核销/路重号/冻结后
//! 写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e480 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e480;
pub const RC_OVER: u32 = SEG_BASE + 0x01; // 超额核销
pub const RC_NOLANE: u32 = SEG_BASE + 0x02; // 空路核销
pub const RC_DUPLANE: u32 = SEG_BASE + 0x03; // 路重号
pub const RC_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const RC_ZERO: u32 = SEG_BASE + 0x05; // 零应收立账
pub const RC_CAP: u32 = SEG_BASE + 0x06; // 路账超容

/// 路账定容（须容下十路总扫）。
pub const LANE_CAP: usize = 16;
/// 回收对平数（10/10）。
pub const LANES_NEEDED: usize = 10;
/// 回收账域标（fold 种子）。
pub const RECY_TAG: u64 = 0x1e48_36;

/// 单回收路：路名 + 应收 + 已收。
#[derive(Clone, Debug)]
pub struct RecLane {
    pub name: String,
    pub owed: u32,
    pub done: u32,
}

impl RecLane {
    /// 残留。
    pub fn resid(&self) -> u32 {
        self.owed.saturating_sub(self.done)
    }

    /// 核销齐平。
    pub fn even(&self) -> bool {
        self.owed == self.done && self.owed > 0
    }

    pub fn render(&self) -> String {
        format!("lane|{}|o{}|d{}", self.name, self.owed, self.done)
    }
}

/// 三路回收总扫账：逐路立账 → 逐路核销 → 零残留封账。
pub struct RecyBook {
    pub lanes: Vec<RecLane>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl RecyBook {
    pub fn new() -> RecyBook {
        RecyBook { lanes: Vec::new(), frozen: false, root: None }
    }

    /// 立账：冻结、零应收、重号、超容必拒。
    pub fn enroll(&mut self, name: &str, owed: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        if name.is_empty() {
            return Err(RC_NOLANE);
        }
        if owed == 0 {
            return Err(RC_ZERO);
        }
        if self.lanes.iter().any(|x| x.name == name) {
            return Err(RC_DUPLANE);
        }
        if self.lanes.len() >= LANE_CAP {
            return Err(RC_CAP);
        }
        self.lanes.push(RecLane { name: name.to_string(), owed, done: 0 });
        Ok(())
    }

    /// 回收挂点：不得超应收；逐路核销。
    pub fn reclaim(&mut self, name: &str, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        let mut k = 0usize;
        while k < self.lanes.len() {
            if self.lanes[k].name == name {
                if n > self.lanes[k].owed - self.lanes[k].done {
                    return Err(RC_OVER);
                }
                self.lanes[k].done += n;
                return Ok(self.lanes[k].done);
            }
            k += 1;
        }
        Err(RC_NOLANE)
    }

    /// 残留面：未齐平路清单。
    pub fn residues(&self) -> Vec<String> {
        let mut g: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.lanes.len() {
            if !self.lanes[k].even() {
                g.push(self.lanes[k].name.to_string());
            }
            k += 1;
        }
        g
    }

    /// 残留总量。
    pub fn resid_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.lanes.len() {
            n = n.wrapping_add(self.lanes[k].resid());
            k += 1;
        }
        n
    }

    /// 零残留断言：十路齐核销。
    pub fn closed(&self) -> bool {
        self.lanes.len() == LANES_NEEDED
            && self.resid_total() == 0
            && self.residues().is_empty()
    }

    /// 三路归类：close/unlink/rmid 三路名齐在册。
    pub fn three_lanes(&self, a: &str, b: &str, c: &str) -> bool {
        self.find(a).is_some() && self.find(b).is_some() && self.find(c).is_some()
    }

    /// 按路名查位次。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.lanes.len() {
            if self.lanes[k].name == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RECY_TAG;
        let mut k = 0usize;
        while k < self.lanes.len() {
            let x = &self.lanes[k];
            h = (h ^ x.owed as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.done as u64).wrapping_mul(0x100000001b3);
            let bytes = x.name.as_bytes();
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

/// 路名确定性样本（三主路 + 派生路）。
pub fn recy_lane(no: usize) -> String {
    match no % 3 {
        0 => format!("close{}", no),
        1 => format!("unlink{}", no),
        _ => format!("rmid{}", no),
    }
}

/// 应收确定性样本。
pub fn recy_owed(no: usize) -> u32 {
    (no % 5 + 2) as u32
}

/// 十路总扫全链：逐路立账并逐路核销齐平。
pub fn build_recybook() -> (RecyBook, bool) {
    let mut b = RecyBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < LANES_NEEDED {
        if b.enroll(&recy_lane(i), recy_owed(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < LANES_NEEDED {
        if b.reclaim(&recy_lane(i), recy_owed(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let b_ok = ok && b.closed();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 超额核销必拒
    let mut a = RecyBook::new();
    let _ = a.enroll("close0", 4);
    let _ = a.reclaim("close0", 2);
    got[0] = a.reclaim("close0", 3).err() == Some(RC_OVER);
    // 2) 空路核销必拒
    let mut b = RecyBook::new();
    got[1] = b.reclaim("nowhere", 1).err() == Some(RC_NOLANE);
    // 3) 路重号与零应收必拒
    let mut c = RecyBook::new();
    let _ = c.enroll("unlink0", 3);
    let z = c.enroll("rmid0", 0).err() == Some(RC_ZERO);
    got[2] = z && c.enroll("unlink0", 3).err() == Some(RC_DUPLANE);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_recybook();
    d.freeze();
    got[3] = d.reclaim(&recy_lane(0), 1).err() == Some(RC_FROZEN)
        && d.enroll("late", 1).err() == Some(RC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11036_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11036");

    // J1 回收挂点逐族核销：十路逐路齐平
    let (mut book, green) = build_recybook();
    s.add("J1-逐族核销", green && book.lanes.len() == 10, "十路回收逐路立账核销齐平");

    // J2 三路回收零残留：残留面清空账封闭
    s.add("J2-零残留", book.resid_total() == 0 && book.closed(), "三路残留清零账封闭");

    // J3 三主路齐在册：close/unlink/rmid 均有立账
    let j3 = book.three_lanes("close0", "unlink1", "rmid2");
    s.add("J3-三主路在册", j3, "close/unlink/rmid 三路齐在册");

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：超额核销 + 空路核销
    let inj = inject_four();
    s.add("J5-超空双抓", inj[0] && inj[1], "超额核销与空路核销两路必抓");

    // J6 注入检出：路重号零应收 + 冻结后写
    s.add("J6-重号冻结检出", inj[2] && inj[3], "重号零应收与冻结后写必抓");

    // J7 分次核销：分两次与一次到位等价
    let j7 = {
        let mut c = RecyBook::new();
        let _ = c.enroll("close9", 10);
        let _ = c.reclaim("close9", 4);
        let _ = c.reclaim("close9", 6);
        c.resid_total() == 0
    };
    s.add("J7-分次核销", j7, "分次核销与一次到位等价");

    // J8 核销齐平后再核销必拒
    let j8 = {
        let mut c = RecyBook::new();
        let _ = c.enroll("rmid9", 3);
        let _ = c.reclaim("rmid9", 3);
        c.reclaim("rmid9", 1).err() == Some(RC_OVER)
    };
    s.add("J8-齐平再销拒", j8, "核销齐平后超额必拒");

    // J9 渲染面：路渲染含名应收已收
    let j9 = {
        let x = RecLane { name: "unlink7".to_string(), owed: 9, done: 9 };
        let r = x.render();
        r.contains("unlink7") && r.contains("o9") && r.contains("d9")
    };
    s.add("J9-渲染明细", j9, "路渲染含名应收已收");

    // J10 路名归类确定性：三路归类公式精确
    let j10 = recy_lane(0) == "close0" && recy_lane(1) == "unlink1" && recy_lane(2) == "rmid2";
    s.add("J10-路名归类", j10, "路名按三路公式归类");

    // J11 超容拒：第 LANE_CAP+1 路必拒
    let j11 = {
        let mut c = RecyBook::new();
        let mut filled = true;
        let mut n = 0usize;
        while n < LANE_CAP {
            let nm = format!("lane{}", n);
            if c.enroll(&nm, 3).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll("overflow", 3).err() == Some(RC_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 样本确定性：路名与应收同入同出
    let n5a = recy_lane(5);
    let n5b = recy_lane(5);
    let o6a = recy_owed(6);
    let o6b = recy_owed(6);
    s.add("J12-样本确定性", n5a == n5b && o6a == o6b, "路名应收样本同入同出");

    // J13 残留注入：漏核销一路残留恰一
    let j13 = {
        let mut c = RecyBook::new();
        let mut i = 0usize;
        while i < LANES_NEEDED {
            let _ = c.enroll(&recy_lane(i), recy_owed(i));
            i += 1;
        }
        let mut i = 1usize;
        while i < LANES_NEEDED {
            let _ = c.reclaim(&recy_lane(i), recy_owed(i));
            i += 1;
        }
        c.residues().len() == 1 && !c.closed()
    };
    s.add("J13-漏销残留", j13, "漏核销一路残留恰一断言为假");

    // J14 应收非零面：齐平路 owe>0 才算齐平
    let j14 = {
        let x = RecLane { name: "close3".to_string(), owed: 0, done: 0 };
        !x.even() && x.resid() == 0
    };
    s.add("J14-零账不算齐", j14, "零应收不算齐平");

    // J15 动态明细：十路齐平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-总扫明细",
        fp == root && book.closed(),
        &format!("root={:#x} lanes=10", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11036_recy_flow() {
        let (mut b, built) = build_recybook();
        assert!(built);
        b.freeze();
        assert!(b.reclaim(&recy_lane(0), 1).err() == Some(RC_FROZEN));
        assert!(b.enroll("late", 1).err() == Some(RC_FROZEN));
        assert!(b.three_lanes("close0", "unlink1", "rmid2"));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11036_checks_green() {
        let s = run_unx11036_checks();
        assert!(s.all_passed(), "UNX-F11036 判据未全绿");
    }
}
