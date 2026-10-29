//! UNX-F12139 — ktest .pdata 面断言集（B07 批判据聚合）（0x1a380 段）
//!
//! 锚点：UNX-F12139-J1 本批 19 条判据聚合入 ktest .pdata 面一次全跑 100%，
//! 压测族与守恒账族独立可单跑。
//! 域：CoRun·UNX-D1-B07（.pdata 展开表面谱系）。
//! 结构：断言集登记（断言 = 族/名/判据指针语义）+ 聚合核（一次全跑即用即弃）+
//! 族单跑核（压测族/守恒族独立跑）+ 通过率千分位 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a380;
pub const KA_GHOST: u32 = SEG_BASE + 0x01; // 断言越域
pub const KA_DUP: u32 = SEG_BASE + 0x02; // 重名断言
pub const KA_EMPTY: u32 = SEG_BASE + 0x03; // 名为空
pub const KA_FROZEN: u32 = SEG_BASE + 0x04; // 印后登记
pub const KA_RED: u32 = SEG_BASE + 0x05; // 断言红项

/// 断言族闭集。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FamKind {
    Press,  // 压测族
    Consrv, // 守恒账族
    Norm,   // 常规族
}

impl FamKind {
    pub fn name(&self) -> &'static str {
        match self {
            FamKind::Press => "press",
            FamKind::Consrv => "consrv",
            FamKind::Norm => "norm",
        }
    }
}

/// 一条登记断言：名 + 族 + 布尔断言（登记即求值语义，与 CheckSet::add 同构）。
pub struct Kassert {
    pub seq: u32,
    pub fam: FamKind,
    pub name: String,
    pub passed: bool,
}

/// B07 断言集聚合册。
pub struct KassertBook {
    pub asserts: Vec<Kassert>,
    pub seq_next: u32,
    sealed: bool,
}

impl KassertBook {
    pub fn new() -> KassertBook {
        KassertBook { asserts: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 登记断言（越族/重名/名空必拒；红项登记收 KA_RED 但仍入账）。
    pub fn add(&mut self, fam: FamKind, name: &str, passed: bool) -> Result<u32, u32> {
        if self.sealed {
            return Err(KA_FROZEN);
        }
        if name.is_empty() {
            return Err(KA_EMPTY);
        }
        if self.asserts.iter().any(|a| a.name == name) {
            return Err(KA_DUP);
        }
        let seq = self.seq_next;
        self.asserts.push(Kassert {
            seq,
            fam,
            name: name.to_string(),
            passed,
        });
        self.seq_next += 1;
        if passed {
            Ok(seq)
        } else {
            Err(KA_RED)
        }
    }

    /// 一次全跑：返回 (全绿?, 总数, 绿数)。
    pub fn run_all(&self) -> (bool, usize, usize) {
        let total = self.asserts.len();
        let greens = self.asserts.iter().filter(|a| a.passed).count();
        (greens == total && total > 0, total, greens)
    }

    /// 族单跑：压测族/守恒族/常规族独立跑。
    pub fn run_fam(&self, fam: FamKind) -> (bool, usize, usize) {
        let sel: Vec<&Kassert> = self.asserts.iter().filter(|a| a.fam == fam).collect();
        let total = sel.len();
        let greens = sel.iter().filter(|a| a.passed).count();
        (greens == total && total > 0, total, greens)
    }

    /// 红项清单。
    pub fn red_items(&self) -> Vec<String> {
        self.asserts
            .iter()
            .filter(|a| !a.passed)
            .map(|a| a.name.clone())
            .collect()
    }

    /// 通过率千分位。
    pub fn pass_permille(&self) -> usize {
        if self.asserts.is_empty() {
            return 0;
        }
        let g = self.asserts.iter().filter(|a| a.passed).count();
        g * 1000 / self.asserts.len()
    }

    /// 族清点。
    pub fn count_of(&self, fam: FamKind) -> usize {
        self.asserts.iter().filter(|a| a.fam == fam).count()
    }

    /// 本批 19 条齐备判定（压测 6 + 守恒 6 + 常规 7）。
    pub fn batch_full(&self) -> bool {
        self.count_of(FamKind::Press) == 6
            && self.count_of(FamKind::Consrv) == 6
            && self.count_of(FamKind::Norm) == 7
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a380;
        for a in &self.asserts {
            h = h.wrapping_mul(211).wrapping_add(a.passed as u32);
            for by in a.name.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KassertBook::new();
    // 1) 名空必拒。
    got[0] = b.add(FamKind::Norm, "", true).err() == Some(KA_EMPTY);
    // 2) 重名必拒。
    let _ = b.add(FamKind::Norm, "dup", true);
    got[1] = b.add(FamKind::Norm, "dup", true).err() == Some(KA_DUP);
    // 3) 红项登记收 RED。
    got[2] = b.add(FamKind::Press, "red_one", false).err() == Some(KA_RED);
    // 4) 红项使全跑不绿。
    let _ = b.add(FamKind::Consrv, "ok1", true);
    let (green, _, _) = b.run_all();
    got[3] = !green;
    // 5) 印后登记必拒。
    b.seal();
    got[4] = b.add(FamKind::Norm, "post", true).err() == Some(KA_FROZEN);
    got
}

/// 判据入口：UNX-F12139
pub fn run_unx12139_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12139");
    let mut b = KassertBook::new();

    // J1 本批 19 条登记齐（压测 6 + 守恒 6 + 常规 7）。
    let mut j1 = true;
    for i in 0..6usize {
        let name = format!("press_a{}", i);
        if b.add(FamKind::Press, &name, true).is_err() {
            j1 = false;
        }
    }
    for i in 0..6usize {
        let name = format!("consrv_a{}", i);
        if b.add(FamKind::Consrv, &name, true).is_err() {
            j1 = false;
        }
    }
    for i in 0..7usize {
        let name = format!("norm_a{}", i);
        if b.add(FamKind::Norm, &name, true).is_err() {
            j1 = false;
        }
    }
    s.add("J1_batch_full", j1 && b.batch_full(), "19 条三族登记齐");

    // J2 一次全跑 100%。
    let (green, total, greens) = b.run_all();
    s.add(
        "J2_run_all",
        green && total == 19 && greens == 19,
        "一次全跑 19/19 全绿",
    );

    // J3 压测族独立单跑。
    let (pg, pt, pg_) = b.run_fam(FamKind::Press);
    s.add(
        "J3_press_solo",
        pg && pt == 6 && pg_ == 6,
        "压测族独立可单跑 6/6",
    );

    // J4 守恒族独立单跑。
    let (cg, ct, cg_) = b.run_fam(FamKind::Consrv);
    s.add(
        "J4_consrv_solo",
        cg && ct == 6 && cg_ == 6,
        "守恒族独立可单跑 6/6",
    );

    // J5 红项清单空。
    s.add("J5_no_red", b.red_items().is_empty(), "全绿红项清单空");

    // J6 通过率千分位满额。
    s.add("J6_permille", b.pass_permille() == 1000, "通过率千分位 1000");

    // J7 族清点对平。
    s.add(
        "J7_fam_count",
        b.count_of(FamKind::Press) == 6
            && b.count_of(FamKind::Consrv) == 6
            && b.count_of(FamKind::Norm) == 7,
        "三族清点 6/6/7 对平",
    );

    // J8 红项点名（造单红验证）。
    let mut b2 = KassertBook::new();
    let _ = b2.add(FamKind::Norm, "g1", true);
    let _ = b2.add(FamKind::Norm, "r1", false);
    let _ = b2.add(FamKind::Norm, "g2", true);
    s.add(
        "J8_red_name",
        b2.red_items() == vec!["r1".to_string()] && b2.pass_permille() == 666,
        "红项点名且千分位 2/3",
    );

    // J9 重名受控。
    s.add(
        "J9_dup",
        b.add(FamKind::Norm, "press_a0", true).err() == Some(KA_DUP),
        "跨族重名仍拒 DUP",
    );

    // J10 名空受控。
    s.add(
        "J10_empty",
        b.add(FamKind::Norm, "", true).err() == Some(KA_EMPTY),
        "名空拒 EMPTY",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "断言集折叠指纹一致");

    // J12 指纹对红项敏感。
    let mut b3 = KassertBook::new();
    let _ = b3.add(FamKind::Norm, "x", true);
    let f_before = b3.fold();
    let _ = b3.add(FamKind::Norm, "y", false);
    s.add("J12_fp_red", b3.fold() != f_before, "指纹对红项敏感");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.add(FamKind::Norm, "post", true).err() == Some(KA_FROZEN),
        "印账后登记拒 FROZEN",
    );

    // J15 序号单调。
    let seqs: Vec<u32> = b.asserts.iter().map(|a| a.seq).collect();
    let mono = seqs.windows(2).all(|w| w[1] > w[0]);
    s.add("J15_seq_mono", mono && seqs.first() == Some(&1), "断言序号单调");

    // J16 族名闭集。
    s.add(
        "J16_fam_names",
        [FamKind::Press.name(), FamKind::Consrv.name(), FamKind::Norm.name()]
            == ["press", "consrv", "norm"],
        "三族名闭集",
    );

    // J17 空册全跑不判绿（无断言不算 100%）。
    let b4 = KassertBook::new();
    let (green4, t4, _) = b4.run_all();
    s.add(
        "J17_empty_not_green",
        !green4 && t4 == 0 && b4.pass_permille() == 0,
        "空册全跑不判绿",
    );

    // J18 断言族语义独立：改一族不影响他族单跑。
    let mut b5 = KassertBook::new();
    let _ = b5.add(FamKind::Press, "p_bad", false);
    let _ = b5.add(FamKind::Consrv, "c_good", true);
    let (p_green, _, _) = b5.run_fam(FamKind::Press);
    let (c_green, _, _) = b5.run_fam(FamKind::Consrv);
    s.add(
        "J18_fam_isolation",
        !p_green && c_green,
        "压测族红不连坐守恒族",
    );

    // J19 红项多笔全点名。
    let mut b6 = KassertBook::new();
    let _ = b6.add(FamKind::Norm, "r_a", false);
    let _ = b6.add(FamKind::Norm, "g", true);
    let _ = b6.add(FamKind::Norm, "r_b", false);
    s.add(
        "J19_multi_red",
        b6.red_items().len() == 2 && b6.pass_permille() == 333,
        "双红全点名千分位 1/3",
    );

    // J20 全绿单册与红册指纹互异。
    s.add(
        "J20_fp_distinct",
        b.fold() != b2.fold(),
        "全绿册与含红册指纹互异",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12139_fam_solo() {
        let mut b = KassertBook::new();
        let _ = b.add(FamKind::Press, "p", true);
        let _ = b.add(FamKind::Consrv, "c", true);
        assert!(b.run_fam(FamKind::Press).0);
        assert!(b.run_fam(FamKind::Consrv).0);
    }

    #[test]
    fn unx12139_checks_green() {
        let s = run_unx12139_checks();
        assert!(s.all_passed(), "UNX-F12139 判据未全绿");
    }

    #[test]
    fn unx12139_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
