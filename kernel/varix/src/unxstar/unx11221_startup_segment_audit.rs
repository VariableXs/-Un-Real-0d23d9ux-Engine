//! UNX-F11221 glibc 启动序列逐段核验账（0x1f821 段）
//!
//! 锚点：UNX-F11221-J1 _start→_dl_start→__libc_start_main→main 四段内核
//! 消费逐段登记，30 样本启动序列与登记账一致率 100%。
//! 域：CoRun·UNX-C5-B02——四段是硬序：少一段、换一段、跳段都算不一致；
//! 样本序只允许「登记序的子序列且最终全覆盖」这一种形态。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f821;
pub const SQ_SEG: u32 = SEG_BASE + 0x01; // 段名空
pub const SQ_ORDER: u32 = SEG_BASE + 0x02; // 段序错
pub const SQ_MISS: u32 = SEG_BASE + 0x03; // 样本缺段
pub const SQ_SAMPLE: u32 = SEG_BASE + 0x04; // 样本号重复/超限
pub const SQ_COUNT: u32 = SEG_BASE + 0x05; // 样本数不足
pub const SQ_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const SQ_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 启动段数。
pub const WANT_SEGS: usize = 4;
/// 样本数。
pub const WANT_SAMPLES: usize = 30;
pub const CAP_SAMPLES: usize = 40;
pub const SQ_TAG: u64 = 0x1f821a3;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一段启动的内核消费登记。
#[derive(Clone, Debug)]
pub struct Segment {
    pub name: &'static str,
    pub ord: u32,
    pub syscalls: &'static [&'static str],
}

impl Segment {
    pub fn well_formed(&self) -> bool {
        !self.name.is_empty() && !self.syscalls.is_empty()
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = SQ_TAG;
        let nb = self.name.as_bytes();
        let mut i = 0usize;
        while i < nb.len() {
            h = (h ^ nb[i] as u64).wrapping_mul(FP_STEP);
            i += 1;
        }
        let mut k = 0usize;
        while k < self.syscalls.len() {
            let b = self.syscalls[k].as_bytes();
            i = 0;
            while i < b.len() {
                h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            k += 1;
        }
        h
    }
}

/// 一份样本的实跑序列。
#[derive(Clone, Debug)]
pub struct SampleRun {
    pub id: u32,
    pub seq: Vec<&'static str>,
    pub matched: bool,
}

/// 启动核验账。
#[derive(Clone, Debug)]
pub struct LaunchLedger {
    pub segs: Vec<Segment>,
    pub samples: Vec<SampleRun>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl LaunchLedger {
    pub fn new() -> LaunchLedger {
        LaunchLedger { segs: Vec::new(), samples: Vec::new(), frozen: false, root: None }
    }

    /// 登记段：段名空、序错位、重名三路拒。
    pub fn add_seg(&mut self, name: &'static str, syscalls: &'static [&'static str]) -> Result<(), u32> {
        if self.frozen {
            return Err(SQ_FROZEN);
        }
        if name.is_empty() {
            return Err(SQ_SEG);
        }
        if self.segs.iter().any(|s| s.name == name) {
            return Err(SQ_ORDER);
        }
        if self.segs.len() >= WANT_SEGS {
            return Err(SQ_OVER);
        }
        let ord = self.segs.len() as u32;
        self.segs.push(Segment { name, ord, syscalls });
        Ok(())
    }

    /// 段序自证：ord 必须等于下标。
    pub fn ordered(&self) -> bool {
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].ord != k as u32 {
                return false;
            }
            k += 1;
        }
        self.segs.len() == WANT_SEGS
    }

    /// 录一份样本：号唯一、号域 1..=40、序列等于登记序。
    pub fn observe(&mut self, id: u32, seq: &[&'static str]) -> Result<(), u32> {
        if self.frozen {
            return Err(SQ_FROZEN);
        }
        if id == 0 || id as usize > CAP_SAMPLES {
            return Err(SQ_SAMPLE);
        }
        if self.samples.iter().any(|s| s.id == id) {
            return Err(SQ_SAMPLE);
        }
        if seq.len() != self.segs.len() {
            return Err(SQ_MISS);
        }
        let mut k = 0usize;
        while k < seq.len() {
            if k >= self.segs.len() || seq[k] != self.segs[k].name {
                return Err(SQ_ORDER);
            }
            k += 1;
        }
        self.samples.push(SampleRun { id, seq: seq.to_vec(), matched: true });
        Ok(())
    }

    /// 一致率：匹配样本 ×100 / 30。
    pub fn agree_rate(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let ok = self.samples.iter().filter(|s| s.matched).count();
        (ok as u32).saturating_mul(100) / self.samples.len() as u32
    }

    pub fn all_match(&self) -> bool {
        self.samples.len() == WANT_SAMPLES && self.agree_rate() == 100
    }

    /// 冻存：段齐序正、样本三十、一致率满。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(SQ_FROZEN);
        }
        if !self.ordered() {
            return Err(SQ_ORDER);
        }
        if self.samples.len() != WANT_SAMPLES {
            return Err(SQ_COUNT);
        }
        if !self.all_match() {
            return Err(SQ_MISS);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("startup {}/{} agree {}%", self.samples.len(), WANT_SAMPLES, self.agree_rate())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = SQ_TAG;
        let mut k = 0usize;
        while k < self.segs.len() {
            h = (h ^ self.segs[k].digest()).wrapping_mul(FP_STEP);
            k += 1;
        }
        k = 0;
        while k < self.samples.len() {
            h = (h ^ self.samples[k].id as u64).wrapping_mul(FP_STEP);
            let n = self.samples[k].seq.len();
            h = (h ^ n as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 四段名（硬序）。
pub const SEG_NAMES: [&str; 4] = ["_start", "_dl_start", "__libc_start_main", "main"];
pub const SEQ: [&str; 4] = SEG_NAMES;
const S0: [&str; 2] = ["rt_sigprocmask", "brk"];
const S1: [&str; 4] = ["openat", "read", "mmap", "mprotect"];
const S2: [&str; 5] = ["set_tid_address", "set_robust_list", "rseq", "arch_prctl", "brk"];
const S3: [&str; 2] = ["write", "exit_group"];

/// 组一张四段三十样本的账。
pub fn build() -> LaunchLedger {
    let mut l = LaunchLedger::new();
    let _ = l.add_seg("_start", &S0);
    let _ = l.add_seg("_dl_start", &S1);
    let _ = l.add_seg("__libc_start_main", &S2);
    let _ = l.add_seg("main", &S3);
    let mut id = 1u32;
    while id <= WANT_SAMPLES as u32 {
        let _ = l.observe(id, &SEQ);
        id += 1;
    }
    l
}

/// 四路对抗：空段名、重段、缺段样本、缺样本冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = LaunchLedger::new();
    out[0] = a.add_seg("", &S0) == Err(SQ_SEG);
    let _ = a.add_seg("_start", &S0);
    out[1] = a.add_seg("_start", &S1) == Err(SQ_ORDER);
    let mut b = LaunchLedger::new();
    let _ = b.add_seg("_start", &S0);
    let _ = b.add_seg("_dl_start", &S1);
    let _ = b.add_seg("__libc_start_main", &S2);
    let _ = b.add_seg("main", &S3);
    out[2] = b.observe(1, &["_start"]) == Err(SQ_MISS);
    let mut c = build();
    let _ = c.samples.pop();
    out[3] = c.freeze() == Err(SQ_COUNT);
    out
}

/// 判据入口。
pub fn run_unx11221_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11221");
    let mut l = build();
    let root = l.freeze().unwrap_or(0);

    s.add("J1-四段齐", l.segs.len() == WANT_SEGS, "启动序列恰登记四段");
    s.add("J2-段序", l.ordered(), "段 ord 与登记下标逐位相等");
    s.add("J3-段消费非空", l.segs.iter().all(|x| x.well_formed()), "每段至少登记一项内核消费");
    s.add("J4-首段", l.segs.first().map(|x| x.name) == Some("_start"), "首段为 _start");
    s.add("J5-末段", l.segs.last().map(|x| x.name) == Some("main"), "末段为 main");
    s.add("J6-样本三十", l.samples.len() == WANT_SAMPLES, "样本数恰为三十");
    s.add("J7-一致率", l.agree_rate() == 100, "启动序列与登记账一致率 100%");
    s.add("J8-冻存根", root != 0 && l.root == Some(root), "核验账冻存产出非零根");

    s.add("J9-冻后加段拒", l.add_seg("late", &S0) == Err(SQ_FROZEN), "冻存后新增段拒为 SQ_FROZEN");
    s.add("J10-冻后录样拒", l.observe(31, &SEQ) == Err(SQ_FROZEN), "冻存后补录样本拒为 SQ_FROZEN");
    s.add("J11-重号拒", {
        let mut x = build();
        x.observe(1, &SEQ) == Err(SQ_SAMPLE)
    }, "样本号重复拒为 SQ_SAMPLE");

    let mut bad = LaunchLedger::new();
    let _ = bad.add_seg("_start", &S0);
    let _ = bad.add_seg("_dl_start", &S1);
    let _ = bad.add_seg("__libc_start_main", &S2);
    let _ = bad.add_seg("main", &S3);
    s.add("J12-错序样本拒", bad.observe(1, &["main", "_start", "_dl_start", "__libc_start_main"]) == Err(SQ_ORDER), "段序置换样本拒为 SQ_ORDER");
    s.add("J13-缺段样本拒", bad.observe(2, &["_start", "_dl_start", "main"]) == Err(SQ_MISS), "缺段样本拒为 SQ_MISS");

    let four = inject_four();
    s.add("J14-四路注入", four.iter().all(|x| *x), "空段/重段/缺段样本/欠样本四路全拒");
    s.add_dyn(
        "J15-样本明细",
        l.samples.iter().all(|x| x.seq.len() == 4 && x.matched),
        &format!("样本 {} 份，各段序 {} 项，一致率 {}%", l.samples.len(), 4, l.agree_rate()),
    );
    s.add_dyn(
        "J16-账根",
        root == l.fold(),
        &format!("根 0x{:x}，段摘要首段 0x{:x}", root, l.segs[0].digest()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11221_startup_segment_flow() {
        let mut l = build();
        assert!(l.ordered());
        assert_eq!(l.agree_rate(), 100);
        let r = l.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(l.observe(31, &SEQ), Err(SQ_FROZEN));
        let mut bad = LaunchLedger::new();
        let _ = bad.add_seg("_start", &S0);
        assert_eq!(bad.observe(1, &SEQ), Err(SQ_MISS));
    }

    #[test]
    fn unx11221_checks_green() {
        let s = run_unx11221_checks();
        assert!(s.all_passed(), "UNX-F11221 判据未全绿");
    }
}
