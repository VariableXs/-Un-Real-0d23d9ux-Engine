//! UNX-F11215 glibc 30 动态样本集管理与判据登记（0x1f815 段）
//!
//! 锚点：UNX-F11215-J1 与 C1 验收共享的 30 动态样本集编号/来源/依赖特征
//! 三元组落账，样本集清单引用编号零缺失。
//! 域：CoRun·UNX-C5-B01——样本编号固定 1..30，来源与依赖位掩码三列登记；
//! 判据清单只以编号引用样本，引用前必查在册，缺号显性列出。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f815;
pub const SM_NO: u32 = SEG_BASE + 0x01; // 编号越界
pub const SM_DUP: u32 = SEG_BASE + 0x02; // 编号重录
pub const SM_SRC: u32 = SEG_BASE + 0x03; // 来源空
pub const SM_DEP: u32 = SEG_BASE + 0x04; // 依赖特征零
pub const SM_MISS: u32 = SEG_BASE + 0x05; // 引用缺号
pub const SM_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后写册
pub const SM_FULL: u32 = SEG_BASE + 0x07; // 样本集未满

/// 样本集定额。
pub const SAMPLES: usize = 30;
/// 依赖特征位数（动态装载/线程/TLS/信号/时钟）。
pub const DEP_BITS: usize = 5;
pub const SM_TAG: u64 = 0x1f815c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 三元组样本：编号、来源、依赖特征位。
#[derive(Clone, Debug)]
pub struct Sample {
    pub no: u32,
    pub source: &'static str,
    pub dep: u32,
}

impl Sample {
    pub fn has_dep(&self, bit: u32) -> bool {
        bit < DEP_BITS as u32 && self.dep & (1u32 << bit) != 0
    }

    pub fn render(&self) -> String {
        format!("S{:02}|{}|{:#07b}", self.no, self.source, self.dep)
    }
}

/// 样本册 + 判据引用清单。
#[derive(Clone, Debug)]
pub struct Manifest {
    pub samples: Vec<Sample>,
    pub refs: Vec<u32>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Manifest {
    pub fn new() -> Manifest {
        Manifest { samples: Vec::new(), refs: Vec::new(), frozen: false, root: None }
    }

    /// 登记样本：冻结/编号越界/来源空/依赖零/重号五路拒。
    pub fn add(&mut self, no: u32, source: &'static str, dep: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        if no == 0 || no as usize > SAMPLES {
            return Err(SM_NO);
        }
        if source.is_empty() {
            return Err(SM_SRC);
        }
        if dep == 0 {
            return Err(SM_DEP);
        }
        if self.samples.iter().any(|s| s.no == no) {
            return Err(SM_DUP);
        }
        self.samples.push(Sample { no, source, dep });
        Ok(())
    }

    /// 判据引用样本：不在册即拒。
    pub fn reference(&mut self, no: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        if !self.samples.iter().any(|s| s.no == no) {
            return Err(SM_MISS);
        }
        if !self.refs.contains(&no) {
            self.refs.push(no);
        }
        Ok(())
    }

    /// 引用清单缺号（相对给定全集）。
    pub fn missing_refs(&self, all: &[u32]) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < all.len() {
            if !self.refs.contains(&all[k]) {
                out.push(all[k]);
            }
            k += 1;
        }
        out
    }

    /// 编号连续 1..30 无缺。
    pub fn numbered(&self) -> bool {
        if self.samples.len() != SAMPLES {
            return false;
        }
        let mut n = 1u32;
        while n <= SAMPLES as u32 {
            if !self.samples.iter().any(|s| s.no == n) {
                return false;
            }
            n += 1;
        }
        true
    }

    /// 依赖位直方图。
    pub fn dep_hist(&self) -> [u32; DEP_BITS] {
        let mut h = [0u32; DEP_BITS];
        let mut k = 0usize;
        while k < self.samples.len() {
            let mut b = 0u32;
            while (b as usize) < DEP_BITS {
                if self.samples[k].has_dep(b) {
                    h[b as usize] = h[b as usize].wrapping_add(1);
                }
                b += 1;
            }
            k += 1;
        }
        h
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = SM_TAG;
        let mut k = 0usize;
        while k < self.samples.len() {
            h = (h ^ self.samples[k].no as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.samples[k].dep as u64).wrapping_mul(FP_STEP);
            let bytes = self.samples[k].source.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        k = 0;
        while k < self.refs.len() {
            h = (h ^ self.refs[k] as u64).wrapping_mul(FP_STEP);
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

/// 来源样本（五源轮转）。
pub const SOURCES: [&'static str; 5] = ["c1-hello", "c1-libc", "dyn-sym", "tls-probe", "sig-frame"];

/// 建册：30 样本编号 1..30 全登记并全部被引用。
pub fn build() -> (Manifest, bool) {
    let mut m = Manifest::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < SAMPLES {
        let src = SOURCES[i % SOURCES.len()];
        let dep = 1 + (i as u32 % ((1 << DEP_BITS) - 1));
        if m.add(i as u32 + 1, src, dep).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut n = 1u32;
    while n <= SAMPLES as u32 {
        if m.reference(n).is_err() {
            ok = false;
        }
        n += 1;
    }
    let good = ok && m.numbered() && m.refs.len() == SAMPLES;
    (m, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 编号越界必拒（0 与 31）。
    let mut a = Manifest::new();
    got[0] = a.add(0, "s", 1).err() == Some(SM_NO)
        && a.add(SAMPLES as u32 + 1, "s", 1).err() == Some(SM_NO);
    // 2) 来源空与依赖零必拒。
    got[1] = a.add(1, "", 1).err() == Some(SM_SRC) && a.add(1, "s", 0).err() == Some(SM_DEP);
    // 3) 引用缺号必拒且缺号可列。
    let mut b = Manifest::new();
    let _ = b.add(1, "s", 1);
    let _ = b.reference(1);
    got[2] = b.reference(2).err() == Some(SM_MISS) && b.missing_refs(&[1, 2]) == vec![2];
    // 4) 冻结后写册与引用均拒。
    let (mut c, _) = build();
    c.freeze();
    got[3] = c.add(31, "x", 1).err() == Some(SM_FROZEN) && c.reference(1).err() == Some(SM_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx11215_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11215");
    let (mut m, built) = build();

    s.add("J1-三十样本", built && m.samples.len() == SAMPLES, "30 动态样本逐条登记");
    s.add("J2-编号连续", m.numbered(), "样本编号 1..30 连续无缺");
    s.add("J3-引用零缺", m.missing_refs(&all_ids()).is_empty() && m.refs.len() == SAMPLES,
        "样本集清单引用编号零缺失");
    s.add("J4-三元组齐备", m.samples.iter().all(|x| x.no != 0 && !x.source.is_empty() && x.dep != 0),
        "编号/来源/依赖特征三列齐备");
    s.add("J5-来源轮转", m.samples[0].source == "c1-hello" && m.samples[5].source == "c1-hello",
        "来源按五源轮转落账");
    s.add("J6-依赖直方图", {
        let h = m.dep_hist();
        let sum: u32 = h.iter().sum();
        sum >= SAMPLES as u32 && h.iter().all(|c| *c > 0)
    }, "五个依赖位各有样本消费");

    s.add("J7-重号拒", {
        let mut a = Manifest::new();
        let _ = a.add(7, "s", 1);
        a.add(7, "t", 2).err() == Some(SM_DUP)
    }, "同编号二次登记必拒");
    s.add("J8-缺号可列", {
        let mut a = Manifest::new();
        let _ = a.add(1, "s", 1);
        let _ = a.reference(1);
        a.missing_refs(&[1, 2, 3]) == vec![2, 3]
    }, "缺号按全集序枚举");
    s.add("J9-重复引用幂等", {
        let mut a = Manifest::new();
        let _ = a.add(1, "s", 1);
        let _ = a.reference(1);
        a.reference(1).is_ok() && a.refs.len() == 1
    }, "同一样本重复引用不重复计数");

    let inj = inject_four();
    s.add("J10-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "越界号/空源零依赖/缺号引用/冻结四路必抓");

    let root = m.freeze();
    s.add("J11-冻结根", m.fold() == root && root != 0, "冻结后样本册指纹可重算");
    s.add("J12-冻结拒写", m.add(1, "x", 1).err() == Some(SM_FROZEN), "冻结后登记必拒");

    let h = m.dep_hist();
    let fp = m.fold();
    s.add_dyn("J13-样本明细", m.refs.len() == 30 && fp == root,
        &format!("refs={} dep0={}", m.refs.len(), h[0]));
    s
}

/// 编号全集 1..30。
pub fn all_ids() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut n = 1u32;
    while n <= SAMPLES as u32 {
        v.push(n);
        n += 1;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11215_samples_flow() {
        let (mut m, built) = build();
        assert!(built);
        assert_eq!(m.samples.len(), 30);
        assert!(m.numbered());
        assert!(m.missing_refs(&all_ids()).is_empty());
        assert_eq!(m.reference(1).is_ok(), true);
        assert_eq!(m.add(1, "dup", 1), Err(SM_DUP));
        let root = m.freeze();
        assert_eq!(m.fold(), root);
        assert!(m.reference(2).is_err());
        assert!(m.samples[29].no == 30);
    }

    #[test]
    fn unx11215_checks_green() {
        let s = run_unx11215_checks();
        assert!(s.all_passed(), "UNX-F11215 判据未全绿");
    }
}
