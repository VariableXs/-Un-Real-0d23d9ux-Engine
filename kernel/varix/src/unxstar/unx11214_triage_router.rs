//! UNX-F11214 排疑路径归域引擎（0x1f814 段）
//!
//! 锚点：UNX-F11214-J1 "程序报 GLIBC 版本"类问题按排疑总表归域：符号版本
//! 缺失归本域、内核行为缺口回流 C1/C2，归域判定对排疑手册部 C 第一问案例
//! 集 100% 正确。
//! 域：CoRun·UNX-C5-B01——关键词规则按优先级排序，首个命中即定域；无命中
//! 落 UNKNOWN 而不猜；案例集逐条比对，正确率必须满分。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f814;
pub const TR_NORULE: u32 = SEG_BASE + 0x01; // 规则关键词空
pub const TR_DUP: u32 = SEG_BASE + 0x02; // 规则重录
pub const TR_DOM: u32 = SEG_BASE + 0x03; // 归属域越界
pub const TR_PRIO: u32 = SEG_BASE + 0x04; // 优先级零值
pub const TR_CASE: u32 = SEG_BASE + 0x05; // 案例期望非法
pub const TR_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后加规则
pub const TR_LOW: u32 = SEG_BASE + 0x07; // 正确率未满

/// 归域：C1 内核 / C2 信号 / C5 兼容 / 未知。
pub const DOM_C1: u32 = 1;
pub const DOM_C2: u32 = 2;
pub const DOM_C5: u32 = 5;
pub const DOM_UNKNOWN: u32 = 0;
pub const DOMAINS: [u32; 3] = [DOM_C1, DOM_C2, DOM_C5];
pub const TR_TAG: u64 = 0x1f814c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 关键词规则：词、归属域、优先级（小者先）。
#[derive(Clone, Debug)]
pub struct Rule {
    pub kw: &'static str,
    pub domain: u32,
    pub prio: u32,
}

/// 路由引擎 + 案例集。
#[derive(Clone, Debug)]
pub struct Router {
    pub rules: Vec<Rule>,
    pub cases: Vec<(&'static str, u32)>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Router {
    pub fn new() -> Router {
        Router { rules: Vec::new(), cases: Vec::new(), frozen: false, root: None }
    }

    pub fn add_rule(&mut self, kw: &'static str, domain: u32, prio: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TR_FROZEN);
        }
        if kw.is_empty() {
            return Err(TR_NORULE);
        }
        if !DOMAINS.contains(&domain) {
            return Err(TR_DOM);
        }
        if prio == 0 {
            return Err(TR_PRIO);
        }
        if self.rules.iter().any(|r| r.kw == kw) {
            return Err(TR_DUP);
        }
        self.rules.push(Rule { kw, domain, prio });
        Ok(())
    }

    pub fn add_case(&mut self, text: &'static str, want: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TR_FROZEN);
        }
        if !DOMAINS.contains(&want) {
            return Err(TR_CASE);
        }
        self.cases.push((text, want));
        Ok(())
    }

    /// 归域：按优先级升序取首个命中规则；无命中落 UNKNOWN。
    pub fn classify(&self, text: &str) -> u32 {
        let mut best: Option<&Rule> = None;
        let mut k = 0usize;
        while k < self.rules.len() {
            if contains(text, self.rules[k].kw) {
                match best {
                    None => best = Some(&self.rules[k]),
                    Some(b) => {
                        if self.rules[k].prio < b.prio {
                            best = Some(&self.rules[k]);
                        }
                    }
                }
            }
            k += 1;
        }
        match best {
            Some(r) => r.domain,
            None => DOM_UNKNOWN,
        }
    }

    /// 案例正确率（分子, 分母）。
    pub fn accuracy(&self) -> (usize, usize) {
        let mut hit = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.classify(self.cases[k].0) == self.cases[k].1 {
                hit += 1;
            }
            k += 1;
        }
        (hit, self.cases.len())
    }

    /// 错分案例。
    pub fn misses(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.classify(self.cases[k].0) != self.cases[k].1 {
                out.push(self.cases[k].0);
            }
            k += 1;
        }
        out
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = TR_TAG;
        let mut k = 0usize;
        while k < self.rules.len() {
            let bytes = self.rules[k].kw.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.rules[k].domain as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.rules[k].prio as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        k = 0;
        while k < self.cases.len() {
            h = (h ^ self.cases[k].1 as u64).wrapping_mul(FP_STEP);
            let bytes = self.cases[k].0.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
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

/// 子串包含（朴素扫描，越界不外溢）。
pub fn contains(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    let mut i = 0usize;
    while i + n.len() <= h.len() {
        let mut j = 0usize;
        let mut same = true;
        while j < n.len() {
            if h[i + j] != n[j] {
                same = false;
                break;
            }
            j += 1;
        }
        if same {
            return true;
        }
        i += 1;
    }
    false
}

/// 建引擎：七条规则 + 部 C 第一问 8 案例。
pub fn build() -> (Router, bool) {
    let mut r = Router::new();
    let mut ok = true;
    let rules: [(&'static str, u32, u32); 7] = [
        ("version `GLIBC", DOM_C5, 1),
        ("version mismatch", DOM_C5, 2),
        ("GLIBC_2", DOM_C5, 3),
        ("rt_sigaction", DOM_C2, 4),
        ("rseq", DOM_C2, 4),
        ("auxv", DOM_C1, 5),
        ("set_robust_list", DOM_C1, 5),
    ];
    let mut k = 0usize;
    while k < rules.len() {
        if r.add_rule(rules[k].0, rules[k].1, rules[k].2).is_err() {
            ok = false;
        }
        k += 1;
    }
    let cases: [(&'static str, u32); 8] = [
        ("a.out: version `GLIBC_2.34' not found", DOM_C5),
        ("b.out: version `GLIBC_2.28' not found (required by b.out)", DOM_C5),
        ("symbol _dl_catch_error version mismatch in libc", DOM_C5),
        ("ld.so reports GLIBC_2.39 requirement", DOM_C5),
        ("runtime: rt_sigaction frame restorer differs", DOM_C2),
        ("thread: rseq registration rejected by kernel", DOM_C2),
        ("loader: auxv AT_HWCAP bits absent", DOM_C1),
        ("thread: set_robust_list returned EINVAL", DOM_C1),
    ];
    k = 0;
    while k < cases.len() {
        if r.add_case(cases[k].0, cases[k].1).is_err() {
            ok = false;
        }
        k += 1;
    }
    let (hit, total) = r.accuracy();
    let good = ok && hit == total && total == 8;
    (r, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 符号版本缺失归本域（C5）。
    let (r, _) = build();
    got[0] = r.classify("x: version `GLIBC_2.30' not found") == DOM_C5;
    // 2) 内核行为缺口回流 C1 / C2。
    got[1] = r.classify("auxv AT_RANDOM missing") == DOM_C1
        && r.classify("rt_sigaction mismatch") == DOM_C2;
    // 3) 无关键词落 UNKNOWN，不猜域。
    got[2] = r.classify("application crashed silently") == DOM_UNKNOWN;
    // 4) 空关键词规则必拒、域越界案例必拒。
    let mut b = Router::new();
    got[3] = b.add_rule("", DOM_C1, 1).err() == Some(TR_NORULE)
        && b.add_case("x", 9).err() == Some(TR_CASE);
    got
}

/// 批内判据。
pub fn run_unx11214_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11214");
    let (mut r, built) = build();

    s.add("J1-案例集满分", built && r.accuracy() == (8, 8) && r.misses().is_empty(),
        "部 C 第一问 8 案例归域 100% 正确");
    s.add("J2-符号版本归本域", r.classify("version `GLIBC_2.34' not found") == DOM_C5,
        "符号版本缺失归本域 C5");
    s.add("J3-内核缺口回流", r.classify("auxv AT_PHDR absent") == DOM_C1
        && r.classify("rseq disabled") == DOM_C2, "内核行为缺口回流 C1/C2");
    s.add("J4-无命中不猜", r.classify("nothing relevant here") == DOM_UNKNOWN,
        "无关键词命中落 UNKNOWN");
    s.add("J5-优先级生效", {
        let mut c = Router::new();
        let _ = c.add_rule("GLIBC", DOM_C5, 9);
        let _ = c.add_rule("version `GLIBC", DOM_C2, 1);
        c.classify("version `GLIBC_2.2'") == DOM_C2
    }, "低优先级号规则先命中");
    s.add("J6-规则在册", r.rules.len() == 7 && r.cases.len() == 8, "七规则八案例在册");

    s.add("J7-空词拒", {
        let mut c = Router::new();
        c.add_rule("", DOM_C1, 1).err() == Some(TR_NORULE)
    }, "空关键词必拒");
    s.add("J8-重规则拒", {
        let mut c = Router::new();
        let _ = c.add_rule("auxv", DOM_C1, 1);
        c.add_rule("auxv", DOM_C2, 2).err() == Some(TR_DUP)
    }, "同关键词二次登记必拒");
    s.add("J9-域越界拒", {
        let mut c = Router::new();
        c.add_rule("k", 9, 1).err() == Some(TR_DOM)
    }, "规则归属域越界必拒");
    s.add("J10-优先级零拒", {
        let mut c = Router::new();
        c.add_rule("k", DOM_C1, 0).err() == Some(TR_PRIO)
    }, "优先级零值必拒");

    let inj = inject_four();
    s.add("J11-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "版本归域/缺口回流/不猜/坏规则四路必抓");

    let root = r.freeze();
    s.add("J12-冻结根", r.fold() == root && root != 0, "冻结后引擎指纹可重算");
    s.add("J13-冻结拒加", r.add_rule("more", DOM_C1, 1).err() == Some(TR_FROZEN),
        "冻结后加规则必拒");

    let (hit, total) = r.accuracy();
    let fp = r.fold();
    s.add_dyn("J14-归域明细", hit == total && fp == root,
        &format!("hit={}/{} fp={:#x}", hit, total, fp));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11214_triage_flow() {
        let (mut r, built) = build();
        assert!(built);
        assert_eq!(r.accuracy(), (8, 8));
        assert_eq!(r.classify("version `GLIBC_2.34' not found"), DOM_C5);
        assert_eq!(r.classify("loader: auxv missing"), DOM_C1);
        assert_eq!(r.classify("plain text"), DOM_UNKNOWN);
        assert!(contains("abc", "bc") && !contains("abc", "cb"));
        let root = r.freeze();
        assert_eq!(r.fold(), root);
        assert!(r.add_case("x", DOM_C1).is_err());
        assert!(r.misses().is_empty());
    }

    #[test]
    fn unx11214_checks_green() {
        let s = run_unx11214_checks();
        assert!(s.all_passed(), "UNX-F11214 判据未全绿");
    }
}
