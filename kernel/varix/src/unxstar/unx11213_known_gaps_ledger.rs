//! UNX-F11213 known_gaps 缺陷账与回流协议（0x1f813 段）
//!
//! 锚点：UNX-F11213-J1 缺口条目结构（缺口描述/归属域/缺陷号/状态四列）
//! 落账，回流 C1/C2 的缺陷号 100% 挂账。
//! 域：CoRun·UNX-C5-B01——缺口只能以「有缺陷号」的形态入账，回流清单按
//! 缺陷号逐条比对；状态只允许 开->修/不修 单向推进，回退一律拒。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f813;
pub const GP_NODEFECT: u32 = SEG_BASE + 0x01; // 缺陷号零值
pub const GP_EMPTY: u32 = SEG_BASE + 0x02; // 缺口描述空
pub const GP_DUP: u32 = SEG_BASE + 0x03; // 缺陷号重录
pub const GP_DOMAIN: u32 = SEG_BASE + 0x04; // 归属域越界
pub const GP_STATUS: u32 = SEG_BASE + 0x05; // 状态非法/回退
pub const GP_UNKNOWN: u32 = SEG_BASE + 0x06; // 缺陷号未入账
pub const GP_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后入账

/// 归属域：C1 内核面 / C2 信号面 / C5 兼容面。
pub const DOM_C1: u32 = 1;
pub const DOM_C2: u32 = 2;
pub const DOM_C5: u32 = 5;
pub const DOMAINS: [u32; 3] = [DOM_C1, DOM_C2, DOM_C5];
/// 状态：开 / 已修 / 不修。
pub const ST_OPEN: u32 = 0;
pub const ST_FIXED: u32 = 1;
pub const ST_WONTFIX: u32 = 2;
pub const GP_TAG: u64 = 0x1f813c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 四列缺口条目。
#[derive(Clone, Debug)]
pub struct Gap {
    pub desc: &'static str,
    pub domain: u32,
    pub defect: u32,
    pub status: u32,
}

impl Gap {
    pub fn closed(&self) -> bool {
        self.status != ST_OPEN
    }

    pub fn four_columns(&self) -> bool {
        !self.desc.is_empty() && DOMAINS.contains(&self.domain) && self.defect != 0 && self.status <= ST_WONTFIX
    }

    pub fn render(&self) -> String {
        format!("{}|d{}|s{}", self.desc, self.defect, self.status)
    }
}

/// 缺口账。
#[derive(Clone, Debug)]
pub struct GapBook {
    pub gaps: Vec<Gap>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl GapBook {
    pub fn new() -> GapBook {
        GapBook { gaps: Vec::new(), frozen: false, root: None }
    }

    /// 入账：冻结/空描述/域越界/零缺陷号/重号五路拒。
    pub fn file(&mut self, desc: &'static str, domain: u32, defect: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(GP_FROZEN);
        }
        if desc.is_empty() {
            return Err(GP_EMPTY);
        }
        if !DOMAINS.contains(&domain) {
            return Err(GP_DOMAIN);
        }
        if defect == 0 {
            return Err(GP_NODEFECT);
        }
        if self.gaps.iter().any(|g| g.defect == defect) {
            return Err(GP_DUP);
        }
        self.gaps.push(Gap { desc, domain, defect, status: ST_OPEN });
        Ok(())
    }

    /// 推进状态：只允许从 开 推进一次。
    pub fn advance(&mut self, defect: u32, status: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(GP_FROZEN);
        }
        if status != ST_FIXED && status != ST_WONTFIX {
            return Err(GP_STATUS);
        }
        let mut k = 0usize;
        while k < self.gaps.len() {
            if self.gaps[k].defect == defect {
                if self.gaps[k].status != ST_OPEN {
                    return Err(GP_STATUS);
                }
                self.gaps[k].status = status;
                return Ok(());
            }
            k += 1;
        }
        Err(GP_UNKNOWN)
    }

    /// 回流对账：给定缺陷号清单，返回未入账的号。
    pub fn missing_reflow(&self, defects: &[u32]) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < defects.len() {
            if !self.gaps.iter().any(|g| g.defect == defects[k]) {
                out.push(defects[k]);
            }
            k += 1;
        }
        out
    }

    pub fn open_count(&self) -> usize {
        self.gaps.iter().filter(|g| !g.closed()).count()
    }

    pub fn by_domain(&self, domain: u32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.gaps.len() {
            if self.gaps[k].domain == domain {
                out.push(self.gaps[k].defect);
            }
            k += 1;
        }
        out
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = GP_TAG;
        let mut k = 0usize;
        while k < self.gaps.len() {
            let bytes = self.gaps[k].desc.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.gaps[k].domain as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.gaps[k].defect as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.gaps[k].status as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 回流缺陷号清单（C1/C2/C5 三域共五条）。
pub const REFLOW: [u32; 5] = [12011, 12012, 12021, 12022, 12031];

/// 建账：五条缺口全部入账，两条推进为已修/不修。
pub fn build() -> (GapBook, bool) {
    let mut b = GapBook::new();
    let mut ok = true;
    let rows: [(&'static str, u32, u32); 5] = [
        ("ptrace 单步标志位缺注册", DOM_C1, 12011),
        ("信号帧 restorer 对齐偏差", DOM_C2, 12012),
        ("auxv AT_EXECFN 截断", DOM_C1, 12021),
        ("futex WAKE_OP 计数口径", DOM_C5, 12022),
        ("vdso 时钟精度档位缺一档", DOM_C5, 12031),
    ];
    let mut k = 0usize;
    while k < rows.len() {
        if b.file(rows[k].0, rows[k].1, rows[k].2).is_err() {
            ok = false;
        }
        k += 1;
    }
    if b.advance(12011, ST_FIXED).is_err() {
        ok = false;
    }
    if b.advance(12022, ST_WONTFIX).is_err() {
        ok = false;
    }
    let good = ok && b.missing_reflow(&REFLOW).is_empty() && b.open_count() == 3;
    (b, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 零缺陷号必拒。
    let mut a = GapBook::new();
    got[0] = a.file("无号缺口", DOM_C1, 0).err() == Some(GP_NODEFECT);
    // 2) 域越界必拒。
    let mut b = GapBook::new();
    got[1] = b.file("域错", 7, 12041).err() == Some(GP_DOMAIN);
    // 3) 状态只可单向推进一次。
    let mut c = GapBook::new();
    let _ = c.file("单向", DOM_C2, 12042);
    let _ = c.advance(12042, ST_FIXED);
    got[2] = c.advance(12042, ST_WONTFIX).err() == Some(GP_STATUS);
    // 4) 未入账缺陷号回流必列缺。
    let (d, _) = build();
    got[3] = d.missing_reflow(&[12011, 99999]) == vec![99999];
    got
}

/// 批内判据。
pub fn run_unx11213_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11213");
    let (mut b, built) = build();

    s.add("J1-五条入账", built && b.gaps.len() == REFLOW.len(), "五条缺口逐条入账");
    s.add("J2-回流零缺", b.missing_reflow(&REFLOW).is_empty(), "回流 C1/C2 缺陷号 100% 挂账");
    s.add("J3-四列齐全", b.gaps.iter().all(|g| g.four_columns()),
        "每条缺口描述/归属域/缺陷号/状态四列齐全");
    s.add("J4-状态推进", b.gaps[0].status == ST_FIXED && b.gaps[3].status == ST_WONTFIX,
        "两条缺口状态已单向推进");
    s.add("J5-未结数", b.open_count() == 3, "未结缺口恰三条");
    s.add("J6-按域枚举", b.by_domain(DOM_C1) == vec![12011, 12021]
        && b.by_domain(DOM_C5) == vec![12022, 12031], "按归属域枚举缺陷号正确");

    s.add("J7-空描述拒", {
        let mut c = GapBook::new();
        c.file("", DOM_C1, 1).err() == Some(GP_EMPTY)
    }, "空缺口描述必拒");
    s.add("J8-重号拒", {
        let mut c = GapBook::new();
        let _ = c.file("甲", DOM_C1, 12051);
        c.file("乙", DOM_C2, 12051).err() == Some(GP_DUP)
    }, "同缺陷号二次入账必拒");
    s.add("J9-未知推进拒", {
        let mut c = GapBook::new();
        c.advance(12061, ST_FIXED).err() == Some(GP_UNKNOWN)
    }, "未入账缺陷号推进必拒");
    s.add("J10-非法状态拒", {
        let mut c = GapBook::new();
        let _ = c.file("丙", DOM_C5, 12062);
        c.advance(12062, 9).err() == Some(GP_STATUS)
    }, "非法状态值必拒");

    let inj = inject_four();
    s.add("J11-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "零号/越域/状态回退/回流缺号四路必抓");

    let root = b.freeze();
    s.add("J12-冻结根", b.fold() == root && root != 0, "冻结后缺口账指纹可重算");
    s.add("J13-冻结拒入", b.file("丁", DOM_C1, 12071).err() == Some(GP_FROZEN),
        "冻结后新缺口入账必拒");

    let fp = b.fold();
    s.add_dyn("J14-缺口明细", b.open_count() == 3 && fp == root,
        &format!("gaps={} open={}", b.gaps.len(), b.open_count()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11213_gaps_flow() {
        let (mut b, built) = build();
        assert!(built);
        assert!(b.missing_reflow(&REFLOW).is_empty());
        assert_eq!(b.open_count(), 3);
        assert_eq!(b.file("x", DOM_C1, 12011), Err(GP_DUP));
        assert_eq!(b.advance(12011, ST_WONTFIX), Err(GP_STATUS));
        let root = b.freeze();
        assert_eq!(b.fold(), root);
        assert!(b.advance(12031, ST_FIXED).is_err());
        assert_eq!(b.by_domain(DOM_C2), vec![12012]);
    }

    #[test]
    fn unx11213_checks_green() {
        let s = run_unx11213_checks();
        assert!(s.all_passed(), "UNX-F11213 判据未全绿");
    }
}
