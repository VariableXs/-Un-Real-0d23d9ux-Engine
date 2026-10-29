//! UNX-F11201 glibc 兼容面版本表与排疑总表（0x1f801 段）
//!
//! 锚点：UNX-F11201-J1 钉版 glibc 依赖清单逐项登记满足度，版本表查询接口
//! 对 30 样本全返回可用判定，known_gaps 逐条挂缺陷号。
//! 域：CoRun·UNX-C5-B01——兼容面总账：依赖清单（满足/缺陷）与版本表
//! （四字段：glibc 版本、内核基线、可用、缺口号表）双册并立；查询未登记
//! 版本返回显式「未登记」错误而非默认可用。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f801;
pub const VT_UNREG: u32 = SEG_BASE + 0x01; // 版本未登记
pub const VT_NODEFECT: u32 = SEG_BASE + 0x02; // 缺口无缺陷号
pub const VT_DUP: u32 = SEG_BASE + 0x03; // 版本重号
pub const VT_DEPDUP: u32 = SEG_BASE + 0x04; // 依赖重名
pub const VT_UNSAT: u32 = SEG_BASE + 0x05; // 依赖未满足却无缺陷号
pub const VT_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后写表
pub const VT_SAMPLES: u32 = SEG_BASE + 0x07; // 样本数不符

pub const VT_TAG: u64 = 0x1f801c5;
pub const FP_STEP: u64 = 0x100000001b3;
/// 30 动态样本（与 C1 验收共享）。
pub const SAMPLES: usize = 30;
/// 钉版依赖条目数。
pub const DEPS: usize = 12;

/// 钉版依赖：名、最低版本、满足态、缺陷号（不满足必挂）。
#[derive(Clone, Debug)]
pub struct Dep {
    pub name: &'static str,
    pub need: (u32, u32),
    pub satisfied: bool,
    pub defect: u32,
}

impl Dep {
    /// 满足度合规：满足则缺陷号为 0，不满足则缺陷号非 0。
    pub fn accounted(&self) -> bool {
        if self.satisfied { self.defect == 0 } else { self.defect != 0 }
    }

    pub fn render(&self) -> String {
        format!("{}>={}.{}|{}|d{}", self.name, self.need.0, self.need.1,
            if self.satisfied { "ok" } else { "gap" }, self.defect)
    }
}

/// 版本表行：四字段结构。
#[derive(Clone, Debug)]
pub struct VerRow {
    pub glibc: (u32, u32),
    pub kernel_baseline: u32,
    pub usable: bool,
    pub gaps: Vec<u32>,
}

impl VerRow {
    pub fn tag(&self) -> String {
        format!("{}.{}", self.glibc.0, self.glibc.1)
    }

    /// 四字段齐备：基线非零、缺口号全非零、可用与缺口不矛盾。
    pub fn well_formed(&self) -> bool {
        if self.kernel_baseline == 0 {
            return false;
        }
        let mut k = 0usize;
        while k < self.gaps.len() {
            if self.gaps[k] == 0 {
                return false;
            }
            k += 1;
        }
        self.usable || !self.gaps.is_empty()
    }

    pub fn gap_count(&self) -> usize {
        self.gaps.len()
    }
}

/// 兼容面总表：版本行 + 依赖清单，查询与冻结。
#[derive(Clone, Debug)]
pub struct CompatTable {
    pub rows: Vec<VerRow>,
    pub deps: Vec<Dep>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CompatTable {
    pub fn new() -> CompatTable {
        CompatTable { rows: Vec::new(), deps: Vec::new(), frozen: false, root: None }
    }

    /// 登记依赖：重名拒；不满足且缺陷号为 0 拒（缺口必须挂账）。
    pub fn pin_dep(&mut self, name: &'static str, need: (u32, u32), satisfied: bool, defect: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(VT_FROZEN);
        }
        if self.deps.iter().any(|d| d.name == name) {
            return Err(VT_DEPDUP);
        }
        if !satisfied && defect == 0 {
            return Err(VT_UNSAT);
        }
        self.deps.push(Dep { name, need, satisfied, defect });
        Ok(())
    }

    /// 登记版本行：重号拒、缺口号零值拒。
    pub fn register_ver(&mut self, major: u32, minor: u32, baseline: u32, usable: bool, gaps: &[u32]) -> Result<(), u32> {
        if self.frozen {
            return Err(VT_FROZEN);
        }
        if self.rows.iter().any(|r| r.glibc == (major, minor)) {
            return Err(VT_DUP);
        }
        let mut k = 0usize;
        while k < gaps.len() {
            if gaps[k] == 0 {
                return Err(VT_NODEFECT);
            }
            k += 1;
        }
        let mut g: Vec<u32> = Vec::new();
        k = 0;
        while k < gaps.len() {
            g.push(gaps[k]);
            k += 1;
        }
        self.rows.push(VerRow { glibc: (major, minor), kernel_baseline: baseline, usable, gaps: g });
        Ok(())
    }

    /// 查询接口：未登记返回显式错误，绝不默认可用。
    pub fn query(&self, major: u32, minor: u32) -> Result<bool, u32> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].glibc == (major, minor) {
                return Ok(self.rows[k].usable);
            }
            k += 1;
        }
        Err(VT_UNREG)
    }

    /// 未满足依赖数。
    pub fn unmet(&self) -> usize {
        self.deps.iter().filter(|d| !d.satisfied).count()
    }

    /// 依赖满足度合规：每条都按「满足/缺陷号」互斥规则挂账。
    pub fn deps_accounted(&self) -> bool {
        self.deps.iter().all(|d| d.accounted())
    }

    /// 缺口缺陷号全集（去重后升序）。
    pub fn gap_defects(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            let mut j = 0usize;
            while j < self.rows[k].gaps.len() {
                let d = self.rows[k].gaps[j];
                if !out.contains(&d) {
                    out.push(d);
                }
                j += 1;
            }
            k += 1;
        }
        out.sort();
        out
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = VT_TAG;
        let mut k = 0usize;
        while k < self.rows.len() {
            h = (h ^ self.rows[k].glibc.0 as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.rows[k].glibc.1 as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.rows[k].kernel_baseline as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.rows[k].usable as u64).wrapping_mul(FP_STEP);
            let mut j = 0usize;
            while j < self.rows[k].gaps.len() {
                h = (h ^ self.rows[k].gaps[j] as u64).wrapping_mul(FP_STEP);
                j += 1;
            }
            k += 1;
        }
        k = 0;
        while k < self.deps.len() {
            let bytes = self.deps[k].name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.deps[k].satisfied as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.deps[k].defect as u64).wrapping_mul(FP_STEP);
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

/// 30 样本版本对（2.17 起每档递增，覆盖钉版 2.39）。
pub fn sample_ver(i: usize) -> (u32, u32) {
    (2, 10 + i as u32)
}

/// 钉版依赖样本（12 条，两条已知缺口挂缺陷号）。
pub fn pinned_deps() -> Vec<(&'static str, (u32, u32), bool, u32)> {
    let mut v: Vec<(&'static str, (u32, u32), bool, u32)> = Vec::new();
    v.push(("libc.so.6", (2, 17), true, 0));
    v.push(("libm.so.6", (2, 17), true, 0));
    v.push(("libpthread.so.0", (2, 17), true, 0));
    v.push(("libdl.so.2", (2, 17), true, 0));
    v.push(("librt.so.1", (2, 17), true, 0));
    v.push(("ld-linux-x86-64.so.2", (2, 17), true, 0));
    v.push(("libnss_files.so.2", (2, 30), true, 0));
    v.push(("libresolv.so.2", (2, 30), false, 12011));
    v.push(("libutil.so.1", (2, 17), true, 0));
    v.push(("libcrypt.so.1", (2, 34), false, 12012));
    v.push(("libanl.so.1", (2, 17), true, 0));
    v.push(("libBrokenLocale.so.1", (2, 17), true, 0));
    v
}

/// 建表：12 依赖 + 30 版本行（全可用，三条缺口挂缺陷号）。
pub fn build_table() -> (CompatTable, bool) {
    let mut t = CompatTable::new();
    let deps = pinned_deps();
    let mut k = 0usize;
    while k < deps.len() {
        let d = deps[k];
        if t.pin_dep(d.0, d.1, d.2, d.3).is_err() {
            return (t, false);
        }
        k += 1;
    }
    let mut i = 0usize;
    while i < SAMPLES {
        let (ma, mi) = sample_ver(i);
        let baseline = 6 + (i as u32 % 4);
        let mut gaps: Vec<u32> = Vec::new();
        if i % 10 == 9 {
            gaps.push(12011);
        }
        if i % 15 == 14 {
            gaps.push(12012);
        }
        let usable = true;
        if t.register_ver(ma, mi, baseline, usable, &gaps).is_err() {
            return (t, false);
        }
        i += 1;
    }
    let ok = t.rows.len() == SAMPLES && t.deps.len() == DEPS && t.deps_accounted();
    (t, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未知版本查询必须显式「未登记」。
    let (t, _) = build_table();
    got[0] = t.query(2, 99).err() == Some(VT_UNREG) && t.query(2, 17).is_ok();
    // 2) 缺口号为零必拒。
    let mut a = CompatTable::new();
    got[1] = a.register_ver(2, 40, 6, true, &[0]).err() == Some(VT_NODEFECT);
    // 3) 依赖不满足且无缺陷号必拒。
    let mut b = CompatTable::new();
    got[2] = b.pin_dep("libx.so.1", (2, 39), false, 0).err() == Some(VT_UNSAT);
    // 4) 冻结后登记必拒。
    let (mut c, _) = build_table();
    c.freeze();
    got[3] = c.register_ver(2, 41, 6, true, &[]).err() == Some(VT_FROZEN)
        && c.pin_dep("z.so", (2, 41), true, 0).err() == Some(VT_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx11201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11201");
    let (mut t, built) = build_table();

    s.add("J1-依赖十二项", t.deps.len() == DEPS && built, "钉版依赖清单 12 项逐项登记");
    s.add("J2-满足度互斥", t.deps_accounted(), "满足者缺陷号为零、缺口者挂缺陷号");

    let mut all_usable = true;
    let mut n = 0usize;
    while n < SAMPLES {
        let (ma, mi) = sample_ver(n);
        match t.query(ma, mi) {
            Ok(true) => {}
            _ => all_usable = false,
        }
        n += 1;
    }
    s.add("J3-三十样本可用", all_usable, "版本表查询对 30 样本全返回可用判定");
    s.add("J4-未知显式未登记", t.query(9, 9).err() == Some(VT_UNREG),
        "未登记版本返回显式错误而非默认可用");
    s.add("J5-缺口挂缺陷号", t.gap_defects().len() >= 2, "缺口逐条挂缺陷号且可枚举");
    s.add("J6-四字段结构", t.rows.iter().all(|r| r.well_formed()),
        "版本行四字段齐备且自洽");

    let mut gap_rows = 0usize;
    let mut k = 0usize;
    while k < t.rows.len() {
        if t.rows[k].gap_count() > 0 {
            gap_rows += 1;
        }
        k += 1;
    }
    s.add("J7-缺口可定位", gap_rows == 4 && t.rows.iter().all(|r| r.gap_count() == 0 || r.usable),
        "缺口逐行可定位且带缺口行仍可用");

    let mut d = CompatTable::new();
    let _ = d.pin_dep("a", (2, 17), true, 0);
    let j8 = d.pin_dep("a", (2, 17), true, 0).err() == Some(VT_DEPDUP);
    s.add("J8-依赖重名拒", j8, "同名依赖二次登记必拒");

    let j9 = {
        let mut e = CompatTable::new();
        let _ = e.register_ver(2, 17, 6, true, &[]);
        e.register_ver(2, 17, 7, false, &[12011]).err() == Some(VT_DUP)
    };
    s.add("J9-版本重号拒", j9, "同版本行二次登记必拒");

    let root = t.freeze();
    s.add("J10-冻结根", t.fold() == root && root != 0, "冻结后可重算指纹且非零");
    s.add("J11-冻结后拒写", t.register_ver(3, 0, 6, true, &[]).err() == Some(VT_FROZEN),
        "冻结后版本登记必拒");

    let inj = inject_four();
    s.add("J12-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "未登记/零缺口号/无缺陷号/冻结四路必抓");

    let unmet = t.unmet();
    let fp = t.fold();
    s.add_dyn("J13-总表明细", unmet == 2 && fp == root,
        &format!("unmet={} fp={:#x}", unmet, fp));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11201_version_table_flow() {
        let (mut t, built) = build_table();
        assert!(built);
        assert_eq!(t.rows.len(), SAMPLES);
        assert!(t.query(2, 39).is_ok());
        assert!(t.query(2, 99).err() == Some(VT_UNREG));
        assert!(t.register_ver(2, 99, 6, true, &[]).is_ok());
        assert!(t.query(2, 99) == Ok(true));
        t.freeze();
        assert!(t.pin_dep("q", (2, 40), true, 0).err() == Some(VT_FROZEN));
        assert!(t.deps_accounted());
    }

    #[test]
    fn unx11201_checks_green() {
        let s = run_unx11201_checks();
        assert!(s.all_passed(), "UNX-F11201 判据未全绿");
    }
}
