//! UNX-F11035 五族错误路径挂点对账（错误矩阵联签）（0x1e460 段）
//!
//! 锚点：UNX-F11035-J1 错误挂点逐族对平（10/10），错误路径核销零残留（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·错误矩阵）。
//!
//! 结构：错误矩阵账（族 × 错误码逐格命中账）+ 探针器（合法码命中记账、
//! 非法码必拒）+ 核销器（逐格清账、清不齐留残留）+ 零残留断言（逐族逐格
//! 核销齐平）+ fold 与冻结 + 注入核（非法码/重复核销/越族核销/冻结后写
//! 四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e460 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e460;
pub const FE_BADCODE: u32 = SEG_BASE + 0x01; // 非法错误码
pub const FE_NOFAM: u32 = SEG_BASE + 0x02; // 越族核销
pub const FE_DUPCLEAR: u32 = SEG_BASE + 0x03; // 重复核销
pub const FE_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const FE_UNSET: u32 = SEG_BASE + 0x05; // 格未命中
pub const FE_DRAIN: u32 = SEG_BASE + 0x06; // 核销失平

/// 五族错误码法定域（各族三枚样例码）。
pub const FE_CODES: [u32; 15] = [
    11, 12, 22, // sysv
    17, 2, 13, // posix
    25, 5, 6, // pty
    32, 104, 107, // sock
    9, 14, 16, // evfd
];
/// 族数。
pub const FE_FAMS: usize = 5;
/// 每族码数。
pub const FE_PER: usize = 3;
/// 全格数（10/10 对平面：五族 × 两面）。
pub const FE_FACES: usize = 10;
/// 错误矩阵账域标（fold 种子）。
pub const FERR_TAG: u64 = 0x1e46_35;

/// 错误矩阵账：命中格账 + 核销格账。
pub struct FerrBook {
    pub hits: [bool; 15],
    pub cleared: [bool; 15],
    pub probes: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl FerrBook {
    pub fn new() -> FerrBook {
        FerrBook {
            hits: [false; 15],
            cleared: [false; 15],
            probes: 0,
            frozen: false,
            root: None,
        }
    }

    /// 码定位：非法码必拒。
    pub fn locate(&self, code: u32) -> Result<usize, u32> {
        let mut k = 0usize;
        while k < 15 {
            if FE_CODES[k] == code {
                return Ok(k);
            }
            k += 1;
        }
        Err(FE_BADCODE)
    }

    /// 探针挂点：合法码命中记账；重复探针照记（幂等命中）。
    pub fn probe(&mut self, code: u32) -> Result<usize, u32> {
        if self.frozen {
            return Err(FE_FROZEN);
        }
        let k = self.locate(code)?;
        self.hits[k] = true;
        self.probes = self.probes.wrapping_add(1);
        Ok(k)
    }

    /// 核销挂点：未命中格核销必拒；已核销格重复核销必拒。
    pub fn clear(&mut self, code: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(FE_FROZEN);
        }
        let k = self.locate(code)?;
        if !self.hits[k] {
            return Err(FE_UNSET);
        }
        if self.cleared[k] {
            return Err(FE_DUPCLEAR);
        }
        self.cleared[k] = true;
        Ok(())
    }

    /// 越族核销探针：族号越界必拒（格账按码定位）。
    pub fn clear_fam(&mut self, fam: usize, slot: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(FE_FROZEN);
        }
        if fam >= FE_FAMS || slot >= FE_PER {
            return Err(FE_NOFAM);
        }
        self.clear(FE_CODES[fam * FE_PER + slot])
    }

    /// 残留面：命中未核销格清单。
    pub fn residues(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < 15 {
            if self.hits[k] && !self.cleared[k] {
                g.push(FE_CODES[k]);
            }
            k += 1;
        }
        g
    }

    /// 零残留断言：全格命中全核销。
    pub fn closed(&self) -> bool {
        let mut ok = true;
        let mut k = 0usize;
        while k < 15 {
            if !self.hits[k] || !self.cleared[k] {
                ok = false;
            }
            k += 1;
        }
        ok && self.residues().is_empty()
    }

    /// 核销失平校验：未闭合即报失平。
    pub fn check_closed(&self) -> Result<(), u32> {
        if self.closed() {
            Ok(())
        } else {
            Err(FE_DRAIN)
        }
    }

    /// 十面对平：五族 × 两面（命中面与核销面）。
    pub fn faces_even(&self) -> bool {
        let mut fam_hits = 0usize;
        let mut k = 0usize;
        while k < 15 {
            if self.hits[k] {
                fam_hits += 1;
            }
            k += 1;
        }
        fam_hits == 15 && self.closed()
    }

    /// 链式 fold 指纹（FNV 风格，序敏感；未命中未核销的格与零探针不入链，
    /// 空账即种子）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = FERR_TAG;
        let mut k = 0usize;
        while k < 15 {
            if self.hits[k] || self.cleared[k] {
                h = (h ^ k as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.hits[k] as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.cleared[k] as u64).wrapping_mul(0x100000001b3);
            }
            k += 1;
        }
        if self.probes != 0 {
            h = (h ^ self.probes as u64).wrapping_mul(0x100000001b3);
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
pub fn ferr_fam(no: usize) -> String {
    match no {
        0 => "sysv".to_string(),
        1 => "posix".to_string(),
        2 => "pty".to_string(),
        3 => "sock".to_string(),
        _ => "evfd".to_string(),
    }
}

/// 全矩阵对账全链：逐码探针命中后逐码核销。
pub fn build_ferrbook() -> (FerrBook, bool) {
    let mut b = FerrBook::new();
    let mut ok = true;
    let mut k = 0usize;
    while k < 15 {
        if b.probe(FE_CODES[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    let mut k = 0usize;
    while k < 15 {
        if b.clear(FE_CODES[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    let b_ok = ok && b.closed();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 非法码探针必拒
    let mut a = FerrBook::new();
    got[0] = a.probe(0xdead).err() == Some(FE_BADCODE)
        && a.clear(0xdead).err() == Some(FE_BADCODE);
    // 2) 未命中格核销必拒
    let mut b = FerrBook::new();
    got[1] = b.clear(FE_CODES[0]).err() == Some(FE_UNSET);
    // 3) 重复核销必拒
    let mut c = FerrBook::new();
    let _ = c.probe(FE_CODES[1]);
    let _ = c.clear(FE_CODES[1]);
    got[2] = c.clear(FE_CODES[1]).err() == Some(FE_DUPCLEAR);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_ferrbook();
    d.freeze();
    got[3] = d.probe(FE_CODES[2]).err() == Some(FE_FROZEN)
        && d.clear(FE_CODES[2]).err() == Some(FE_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11035_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11035");

    // J1 错误挂点逐族对平：五族逐码命中核销齐平
    let (mut book, green) = build_ferrbook();
    s.add("J1-逐族对平", green && book.faces_even(), "十五格逐族命中核销齐平");

    // J2 错误路径核销零残留：残留面清空账封闭
    s.add("J2-零残留", book.residues().is_empty() && book.closed() && book.check_closed().is_ok(), "命中格全核销零残留");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.probe(FE_CODES[0]).err() == Some(FE_FROZEN)
        && book.clear(FE_CODES[0]).err() == Some(FE_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后探针核销均必拒");

    // J5 注入检出：非法码 + 未命中核销
    let inj = inject_four();
    s.add("J5-非法未中抓", inj[0] && inj[1], "非法码与未命中核销两路必抓");

    // J6 注入检出：重复核销 + 冻结后写
    s.add("J6-重销冻结检出", inj[2] && inj[3], "重复核销与冻结后写必抓");

    // J7 族定位核销：clear_fam 与按码核销等价
    let j7 = {
        let mut c = FerrBook::new();
        let _ = c.probe(FE_CODES[FE_FAMS * 0 + 1]);
        let g = c.clear_fam(0, 1);
        g.is_ok() && c.residues().is_empty()
    };
    s.add("J7-族定位核销", j7, "族位核销与按码核销等价");

    // J8 越族核销必拒
    let j8 = {
        let mut c = FerrBook::new();
        c.clear_fam(FE_FAMS, 0).err() == Some(FE_NOFAM)
            && c.clear_fam(0, FE_PER).err() == Some(FE_NOFAM)
    };
    s.add("J8-越族拒", j8, "族号越界核销必拒");

    // J9 部分残留：漏核销一格残留恰一
    let j9 = {
        let mut c = FerrBook::new();
        let mut k = 0usize;
        while k < 15 {
            let _ = c.probe(FE_CODES[k]);
            k += 1;
        }
        let mut k = 1usize;
        while k < 15 {
            let _ = c.clear(FE_CODES[k]);
            k += 1;
        }
        c.residues().len() == 1 && !c.closed()
    };
    s.add("J9-漏销残留", j9, "漏核销一格残留恰一断言为假");

    // J10 探针幂等：重复探针同格命中不重复记账面
    let j10 = {
        let mut c = FerrBook::new();
        let _ = c.probe(FE_CODES[3]);
        let f1 = c.fold();
        let _ = c.probe(FE_CODES[3]);
        c.hits[3] && c.fold() != f1 && c.probes == 2
    };
    s.add("J10-探针幂等", j10, "重复探针命中面不变计数可推");

    // J11 码域唯一：十五码无重号
    let j11 = {
        let mut uniq = true;
        let mut a = 0usize;
        while a < 15 {
            let mut b2 = a + 1;
            while b2 < 15 {
                if FE_CODES[a] == FE_CODES[b2] {
                    uniq = false;
                }
                b2 += 1;
            }
            a += 1;
        }
        uniq
    };
    s.add("J11-码域唯一", j11, "十五码互不重号");

    // J12 族名确定性
    let j12 = ferr_fam(0) == "sysv" && ferr_fam(4) == "evfd" && ferr_fam(2) == "pty";
    s.add("J12-族名确定", j12, "族名样本同入同出");

    // J13 失平校验：构造半核销态失平可报
    let j13 = {
        let mut c = FerrBook::new();
        let _ = c.probe(FE_CODES[5]);
        let _ = c.clear(FE_CODES[5]);
        let _ = c.probe(FE_CODES[6]);
        c.residues().len() == 1
    };
    s.add("J13-半销可报", j13, "半核销残留可探测");

    // J14 空账零面：未探针账指纹为种子
    let j14 = FerrBook::new().fold() == FERR_TAG && FerrBook::new().probes == 0;
    s.add("J14-空账零面", j14, "空账零探针指纹成立");

    // J15 动态明细：零残留与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-矩阵明细",
        fp == root && book.faces_even(),
        &format!("root={:#x} probes={}", fp, book.probes),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11035_ferr_flow() {
        let (mut b, built) = build_ferrbook();
        assert!(built);
        assert!(b.probe(0xdead).err() == Some(FE_BADCODE));
        b.freeze();
        assert!(b.clear(FE_CODES[0]).err() == Some(FE_FROZEN));
        assert!(b.residues().is_empty());
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11035_checks_green() {
        let s = run_unx11035_checks();
        assert!(s.all_passed(), "UNX-F11035 判据未全绿");
    }
}
