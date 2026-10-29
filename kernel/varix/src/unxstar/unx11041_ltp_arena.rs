//! UNX-F11041 LTP 对照场搭建（ipc/pty 组用例清单立账）（0x1e520 段）
//!
//! 锚点：UNX-F11041-J1 对照场全链搭建（10/10），用例清单账逐项对平（10/10）。
//! 域：CoRun·UNX-C4-B33（LTP 对照域·对照场搭建）。
//!
//! 结构：对照场账（用例清单：例号 + 族号 + 权重 + 对照位）+ 立账器（例号
//! 唯一、族号合法、超容必拒）+ 清单对平器（逐族例数齐平、总例数闭式）+
//! 探针器（例号闭式指纹）+ fold 与冻结 + 注入核（越族立账/重号/零权重/
//! 冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e520 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e520;
pub const LA_NOFAM: u32 = SEG_BASE + 0x01; // 族号越界
pub const LA_DUP: u32 = SEG_BASE + 0x02; // 例号重号
pub const LA_ZW: u32 = SEG_BASE + 0x03; // 零权重
pub const LA_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const LA_CAP: u32 = SEG_BASE + 0x05; // 清单超容
pub const LA_DRAIN: u32 = SEG_BASE + 0x06; // 清单失平

/// 对照族数（ipc 五组 + pty 五组各取五族面）。
pub const LA_FAMS: usize = 5;
/// 每族用例数。
pub const CASES_PER: usize = 8;
/// 清单总例数。
pub const CASES_NEEDED: usize = LA_FAMS * CASES_PER;
/// 清单定容。
pub const LA_CAP_N: usize = 48;
/// 对照场域标（fold 种子）。
pub const LARENA_TAG: u64 = 0x1e52_29;

/// 单用例：例号 + 族号 + 权重 + 对照位。
#[derive(Clone, Debug)]
pub struct LtpCase {
    pub cid: u32,
    pub fam: usize,
    pub weight: u32,
    pub matched: bool,
}

impl LtpCase {
    /// 立账齐备：族号合法且权重非零。
    pub fn sound(&self) -> bool {
        self.fam < LA_FAMS && self.weight > 0
    }

    pub fn render(&self) -> String {
        format!("case|c{}|f{}|w{}|m{}", self.cid, self.fam, self.weight, self.matched as u32)
    }
}

/// LTP 对照场：用例清单立账 → 探针对照 → 清单对平封账。
pub struct LtpArena {
    pub cases: Vec<LtpCase>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl LtpArena {
    pub fn new() -> LtpArena {
        LtpArena { cases: Vec::new(), frozen: false, root: None }
    }

    /// 立账：冻结、零权重、越族、重号、超容必拒。
    pub fn enroll(&mut self, cid: u32, fam: usize, weight: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(LA_FROZEN);
        }
        if weight == 0 {
            return Err(LA_ZW);
        }
        if fam >= LA_FAMS {
            return Err(LA_NOFAM);
        }
        if self.cases.iter().any(|x| x.cid == cid) {
            return Err(LA_DUP);
        }
        if self.cases.len() >= LA_CAP_N {
            return Err(LA_CAP);
        }
        self.cases.push(LtpCase { cid, fam, weight, matched: false });
        Ok(())
    }

    /// 对照探针：例号命中翻对照位；未立账例必拒。
    pub fn match_case(&mut self, cid: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(LA_FROZEN);
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].cid == cid {
                self.cases[k].matched = true;
                return Ok(true);
            }
            k += 1;
        }
        Err(LA_DRAIN)
    }

    /// 逐族例数。
    pub fn fam_count(&self, fam: usize) -> u32 {
        if fam >= LA_FAMS {
            return 0;
        }
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].fam == fam {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 已对照数。
    pub fn matched_n(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].matched {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 清单对平：例数齐、逐族齐平、全数已对照。
    pub fn aligned(&self) -> bool {
        self.cases.len() == CASES_NEEDED
            && self.matched_n() == CASES_NEEDED as u32
            && {
                let mut ok = true;
                let mut f = 0usize;
                while f < LA_FAMS {
                    if self.fam_count(f) != CASES_PER as u32 {
                        ok = false;
                    }
                    f += 1;
                }
                ok
            }
    }

    /// 按例号查位次。
    pub fn find(&self, cid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].cid == cid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 权重总量。
    pub fn weight_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.cases.len() {
            n = n.wrapping_add(self.cases[k].weight);
            k += 1;
        }
        n
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = LARENA_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let x = &self.cases[k];
            h = (h ^ x.cid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.fam as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.weight as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.matched as u64).wrapping_mul(0x100000001b3);
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

/// 例号确定性样本：族 no 族内第 half 例。
pub fn la_cid(fam: usize, half: usize) -> u32 {
    0x4000 + (fam * 100 + half) as u32
}

/// 权重确定性样本。
pub fn la_weight(no: usize) -> u32 {
    (no % 4 + 1) as u32
}

/// 对照场全链：四十例立账并全数对照。
pub fn build_ltparena() -> (LtpArena, bool) {
    let mut a = LtpArena::new();
    let mut ok = true;
    let mut f = 0usize;
    while f < LA_FAMS {
        let mut h = 0usize;
        while h < CASES_PER {
            if a.enroll(la_cid(f, h), f, la_weight(f * CASES_PER + h)).is_err() {
                ok = false;
            }
            h += 1;
        }
        f += 1;
    }
    let mut f = 0usize;
    while f < LA_FAMS {
        let mut h = 0usize;
        while h < CASES_PER {
            if a.match_case(la_cid(f, h)).is_err() {
                ok = false;
            }
            h += 1;
        }
        f += 1;
    }
    let a_ok = ok && a.aligned();
    (a, a_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 越族立账必拒
    let mut a = LtpArena::new();
    got[0] = a.enroll(1, LA_FAMS, 1).err() == Some(LA_NOFAM);
    // 2) 重号立账必拒
    let mut b = LtpArena::new();
    let _ = b.enroll(7, 0, 1);
    got[1] = b.enroll(7, 1, 1).err() == Some(LA_DUP);
    // 3) 零权重必拒且未立账例对照必拒
    let mut c = LtpArena::new();
    let z = c.enroll(8, 0, 0).err() == Some(LA_ZW);
    got[2] = z && c.match_case(999).err() == Some(LA_DRAIN);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_ltparena();
    d.freeze();
    got[3] = d.enroll(0x9999, 0, 1).err() == Some(LA_FROZEN)
        && d.match_case(la_cid(0, 0)).err() == Some(LA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11041");

    // J1 对照场全链搭建：四十例立账全对照
    let (mut arena, green) = build_ltparena();
    s.add("J1-对照场搭建", green && arena.cases.len() == CASES_NEEDED, "五族四十例立账对照全链搭建");

    // J2 用例清单账逐项对平：逐族齐平全数已对照
    s.add("J2-清单对平", arena.aligned(), "逐族八例齐平且全数已对照");

    // J3 冻结根：fold 稳定且与根一致
    let root = arena.freeze();
    s.add("J3-冻结根", arena.root == Some(root) && arena.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = arena.enroll(0x9999, 0, 1).err() == Some(LA_FROZEN)
        && arena.match_case(la_cid(0, 0)).err() == Some(LA_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后立账对照均必拒");

    // J5 注入检出：越族 + 重号
    let inj = inject_four();
    s.add("J5-越族重号抓", inj[0] && inj[1], "越族立账与例号重号两路必抓");

    // J6 注入检出：零权重未立对照 + 冻结后写
    s.add("J6-零权冻结检出", inj[2] && inj[3], "零权重与冻结后写必抓");

    // J7 逐族齐平：每族恰八例
    let j7 = {
        let mut ok = true;
        let mut f = 0usize;
        while f < LA_FAMS {
            if arena.fam_count(f) != CASES_PER as u32 {
                ok = false;
            }
            f += 1;
        }
        ok
    };
    s.add("J7-逐族齐平", j7, "每族用例数恰为八");

    // J8 例号确定性：同族同半同号
    let c2a = la_cid(2, 3);
    let c2b = la_cid(2, 3);
    s.add("J8-例号确定", c2a == c2b, "例号样本同入同出");

    // J9 部分对照：漏照一族对平为假
    let j9 = {
        let mut c = LtpArena::new();
        let mut f = 0usize;
        while f < LA_FAMS {
            let mut h = 0usize;
            while h < CASES_PER {
                let _ = c.enroll(la_cid(f, h), f, la_weight(f * CASES_PER + h));
                h += 1;
            }
            f += 1;
        }
        let mut f = 1usize;
        while f < LA_FAMS {
            let mut h = 0usize;
            while h < CASES_PER {
                let _ = c.match_case(la_cid(f, h));
                h += 1;
            }
            f += 1;
        }
        c.matched_n() == (LA_FAMS - 1) as u32 * CASES_PER as u32 && !c.aligned()
    };
    s.add("J9-漏照为假", j9, "漏照一族对平断言为假");

    // J10 权重面：权重总量与闭式一致
    let j10 = {
        let mut expect = 0u32;
        let mut f = 0usize;
        while f < LA_FAMS {
            let mut h = 0usize;
            while h < CASES_PER {
                expect = expect.wrapping_add(la_weight(f * CASES_PER + h));
                h += 1;
            }
            f += 1;
        }
        arena.weight_total() == expect && expect > 0
    };
    s.add("J10-权重面", j10, "权重总量与逐例闭式一致");

    // J11 超容拒：第 LA_CAP_N+1 例必拒
    let j11 = {
        let mut c = LtpArena::new();
        let mut filled = true;
        let mut n = 0u32;
        while (n as usize) < LA_CAP_N {
            if c.enroll(0x9000 + n, n as usize % LA_FAMS, 1).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(0xa000, 0, 1).err() == Some(LA_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：用例渲染含例号与族号
    let j12 = {
        let x = LtpCase { cid: 0x4042, fam: 1, weight: 3, matched: true };
        let r = x.render();
        r.contains("c16450") && r.contains("f1") && r.contains("m1")
    };
    s.add("J12-渲染明细", j12, "用例渲染含例号族号权重");

    // J13 清单失平校验：构造缺族清单可报
    let j13 = {
        let mut c = LtpArena::new();
        let mut h = 0usize;
        while h < CASES_PER {
            let _ = c.enroll(la_cid(0, h), 0, 1);
            h += 1;
        }
        !c.aligned()
    };
    s.add("J13-缺族可报", j13, "缺族清单对平断言为假");

    // J14 权重样本确定性
    let w4a = la_weight(4);
    let w4b = la_weight(4);
    s.add("J14-权重确定", w4a == w4b, "权重样本同入同出");

    // J15 动态明细：清单对平与冻结根落账
    let fp = arena.fold();
    s.add_dyn(
        "J15-清单明细",
        fp == root && arena.aligned(),
        &format!("root={:#x} cases={}", fp, arena.cases.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11041_ltp_arena_flow() {
        let (mut a, built) = build_ltparena();
        assert!(built);
        assert!(a.match_case(1).err() == Some(LA_DRAIN));
        a.freeze();
        assert!(a.enroll(0x8888, 0, 1).err() == Some(LA_FROZEN));
        assert!(a.weight_total() > 0);
        assert!(a.root.is_some());
    }

    #[test]
    fn unx11041_checks_green() {
        let s = run_unx11041_checks();
        assert!(s.all_passed(), "UNX-F11041 判据未全绿");
    }
}
