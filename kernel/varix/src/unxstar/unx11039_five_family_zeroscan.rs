//! UNX-F11039 挂号对账总扫（五族零残留断言）（0x1e4e0 段）
//!
//! 锚点：UNX-F11039-J1 五族全账核销对平（10/10），零残留断言成立（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·总扫收口）。
//!
//! 结构：五族总扫账（族名 + 应核 + 已核 + 残留）+ 总扫器（逐族核销、超
//! 额必拒）+ 双面断言器（应核面与已核面逐族齐平）+ 零残留封账 + fold 与
//! 冻结 + 注入核（超额核销/空族核销/零应核立账/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e4e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e4e0;
pub const ZS_OVER: u32 = SEG_BASE + 0x01; // 超额核销
pub const ZS_NOFAM: u32 = SEG_BASE + 0x02; // 空族核销
pub const ZS_ZERO: u32 = SEG_BASE + 0x03; // 零应核立账
pub const ZS_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const ZS_DUP: u32 = SEG_BASE + 0x05; // 族重号
pub const ZS_CAP: u32 = SEG_BASE + 0x06; // 总扫账超容

/// 总扫账定容。
pub const ZS_CAP_N: usize = 8;
/// 总扫对平数（10/10：五族 × 双面）。
pub const ZS_FAMS: usize = 5;
/// 五族总扫域标（fold 种子）。
pub const ZSCAN_TAG: u64 = 0x1e4e_39;

/// 单族总扫项：族名 + 应核 + 已核。
#[derive(Clone, Debug)]
pub struct ZsFam {
    pub name: String,
    pub owed: u32,
    pub done: u32,
}

impl ZsFam {
    /// 残留。
    pub fn resid(&self) -> u32 {
        self.owed.saturating_sub(self.done)
    }

    /// 齐平。
    pub fn even(&self) -> bool {
        self.owed == self.done && self.owed > 0
    }

    pub fn render(&self) -> String {
        format!("zs|{}|o{}|d{}", self.name, self.owed, self.done)
    }
}

/// 五族总扫账：逐族立账 → 逐族核销 → 零残留封账。
pub struct ZsBook {
    pub fams: Vec<ZsFam>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ZsBook {
    pub fn new() -> ZsBook {
        ZsBook { fams: Vec::new(), frozen: false, root: None }
    }

    /// 立账：冻结、零应核、重号、超容必拒。
    pub fn enroll(&mut self, name: &str, owed: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(ZS_FROZEN);
        }
        if name.is_empty() {
            return Err(ZS_NOFAM);
        }
        if owed == 0 {
            return Err(ZS_ZERO);
        }
        if self.fams.iter().any(|x| x.name == name) {
            return Err(ZS_DUP);
        }
        if self.fams.len() >= ZS_CAP_N {
            return Err(ZS_CAP);
        }
        self.fams.push(ZsFam { name: name.to_string(), owed, done: 0 });
        Ok(())
    }

    /// 核销挂点：不得超应核。
    pub fn sweep(&mut self, name: &str, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(ZS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.fams.len() {
            if self.fams[k].name == name {
                if n > self.fams[k].owed - self.fams[k].done {
                    return Err(ZS_OVER);
                }
                self.fams[k].done += n;
                return Ok(self.fams[k].done);
            }
            k += 1;
        }
        Err(ZS_NOFAM)
    }

    /// 残留面。
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

    /// 残留总量。
    pub fn resid_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.fams.len() {
            n = n.wrapping_add(self.fams[k].resid());
            k += 1;
        }
        n
    }

    /// 零残留断言：五族齐核销。
    pub fn closed(&self) -> bool {
        self.fams.len() == ZS_FAMS
            && self.resid_total() == 0
            && self.residues().is_empty()
    }

    /// 双面对平：应核总量 = 已核总量。
    pub fn faces_even(&self) -> bool {
        let mut o = 0u32;
        let mut d = 0u32;
        let mut k = 0usize;
        while k < self.fams.len() {
            o = o.wrapping_add(self.fams[k].owed);
            d = d.wrapping_add(self.fams[k].done);
            k += 1;
        }
        o == d && o > 0
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

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = ZSCAN_TAG;
        let mut k = 0usize;
        while k < self.fams.len() {
            let x = &self.fams[k];
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

/// 族名确定性文本。
pub fn zs_fam(no: usize) -> String {
    match no % 5 {
        0 => "sysv".to_string(),
        1 => "posix".to_string(),
        2 => "pty".to_string(),
        3 => "sock".to_string(),
        _ => "evfd".to_string(),
    }
}

/// 应核确定性样本。
pub fn zs_owed(no: usize) -> u32 {
    (no % 6 + 4) as u32
}

/// 五族总扫全链：逐族立账并逐族核销齐平。
pub fn build_zsbook() -> (ZsBook, bool) {
    let mut b = ZsBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < ZS_FAMS {
        if b.enroll(&zs_fam(i), zs_owed(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < ZS_FAMS {
        if b.sweep(&zs_fam(i), zs_owed(i)).is_err() {
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
    let mut a = ZsBook::new();
    let _ = a.enroll("sysv", 5);
    let _ = a.sweep("sysv", 3);
    got[0] = a.sweep("sysv", 3).err() == Some(ZS_OVER);
    // 2) 空族核销必拒
    let mut b = ZsBook::new();
    got[1] = b.sweep("ghost", 1).err() == Some(ZS_NOFAM);
    // 3) 零应核与重号必拒
    let mut c = ZsBook::new();
    let z = c.enroll("posix", 0).err() == Some(ZS_ZERO);
    let _ = c.enroll("posix", 4);
    got[2] = z && c.enroll("posix", 4).err() == Some(ZS_DUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_zsbook();
    d.freeze();
    got[3] = d.sweep(&zs_fam(0), 1).err() == Some(ZS_FROZEN)
        && d.enroll("late", 1).err() == Some(ZS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11039_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11039");

    // J1 五族全账核销对平：逐族齐平且双面合计
    let (mut book, green) = build_zsbook();
    s.add("J1-全账核销对平", green && book.faces_even(), "五族逐族核销齐平双面合计");

    // J2 零残留断言成立：残留面清空账封闭
    s.add("J2-零残留断言", book.resid_total() == 0 && book.closed(), "五族残留清零账封闭");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.sweep(&zs_fam(0), 1).err() == Some(ZS_FROZEN)
        && book.enroll("late", 1).err() == Some(ZS_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后核销立账均必拒");

    // J5 注入检出：超额核销 + 空族核销
    let inj = inject_four();
    s.add("J5-超空双抓", inj[0] && inj[1], "超额核销与空族核销两路必抓");

    // J6 注入检出：零应核重号 + 冻结后写
    s.add("J6-零账冻结检出", inj[2] && inj[3], "零应核重号与冻结后写必抓");

    // J7 分次核销：分段至齐平等价
    let j7 = {
        let mut c = ZsBook::new();
        let _ = c.enroll("pty", 12);
        let _ = c.sweep("pty", 7);
        let _ = c.sweep("pty", 5);
        c.resid_total() == 0
    };
    s.add("J7-分次核销", j7, "分段核销与一次到位等价");

    // J8 部分残留：漏核一残留恰一
    let j8 = {
        let mut c = ZsBook::new();
        let mut i = 0usize;
        while i < ZS_FAMS {
            let _ = c.enroll(&zs_fam(i), zs_owed(i));
            i += 1;
        }
        let mut i = 1usize;
        while i < ZS_FAMS {
            let _ = c.sweep(&zs_fam(i), zs_owed(i));
            i += 1;
        }
        c.residues().len() == 1 && !c.closed()
    };
    s.add("J8-部分残留", j8, "漏核一族残留恰一断言为假");

    // J9 渲染面：族渲染含名应核已核
    let j9 = {
        let x = ZsFam { name: "sock".to_string(), owed: 8, done: 8 };
        let r = x.render();
        r.contains("sock") && r.contains("o8") && r.contains("d8")
    };
    s.add("J9-渲染明细", j9, "族渲染含名应核已核");

    // J10 族名确定性
    let j10 = zs_fam(1) == "posix" && zs_fam(4) == "evfd" && zs_fam(5) == "sysv";
    s.add("J10-族名确定", j10, "族名样本同入同出");

    // J11 应核样本确定性
    let o2a = zs_owed(2);
    let o2b = zs_owed(2);
    s.add("J11-样本确定", o2a == o2b, "应核样本同入同出");

    // J12 超容拒：第 ZS_CAP_N+1 族必拒
    let j12 = {
        let mut c = ZsBook::new();
        let mut filled = true;
        let mut n = 0usize;
        while n < ZS_CAP_N {
            let nm = format!("fam{}", n);
            if c.enroll(&nm, 4).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll("overflow", 4).err() == Some(ZS_CAP)
    };
    s.add("J12-超容边界", j12, "超出定容必拒且原账不损");

    // J13 空账零面：未核账指纹为种子
    let j13 = ZsBook::new().fold() == ZSCAN_TAG && ZsBook::new().resid_total() == 0;
    s.add("J13-空账零面", j13, "空账零核指纹成立");

    // J14 双面失平注入：漏核后双面合计不等
    let j14 = {
        let mut c = ZsBook::new();
        let _ = c.enroll("evfd", 10);
        let _ = c.sweep("evfd", 6);
        !c.faces_even() && c.resid_total() == 4
    };
    s.add("J14-双面失平", j14, "漏核后双面合计不等");

    // J15 动态明细：零残留与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-总扫明细",
        fp == root && book.closed(),
        &format!("root={:#x} fams=5", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11039_zs_flow() {
        let (mut b, built) = build_zsbook();
        assert!(built);
        assert!(b.faces_even());
        // 冻结面必须封账后才成立：初版把 sweep 的冻结断言放在 freeze 之前，
        // 未封账时它会正常扫（或撞 ZS_OVER），永远到不了 ZS_FROZEN。
        b.freeze();
        assert!(b.sweep(&zs_fam(0), 1).err() == Some(ZS_FROZEN));
        assert!(b.enroll("late", 1).err() == Some(ZS_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11039_checks_green() {
        let s = run_unx11039_checks();
        assert!(s.all_passed(), "UNX-F11039 判据未全绿");
    }
}
