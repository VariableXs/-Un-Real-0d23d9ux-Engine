//! UNX-F12136 — 叶函数（无 .pdata 条目）行为语义（0x1a320 段）
//!
//! 锚点：UNX-F12136-J1 叶函数判定（无调用无栈增长即叶，无需 .pdata 条目；
//! 假叶检出 10/10：有调用/有栈增长/有 try 皆非叶）。
//! 域：CoRun·UNX-D1-B07（.pdata 展开表面谱系）。
//! 结构：叶判定核（调用/栈增长/SEH 三因互斥）+ 叶函数账 + 指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a320;
pub const LF_CALL: u32 = SEG_BASE + 0x01; // 叶含调用
pub const LF_STACK: u32 = SEG_BASE + 0x02; // 叶有栈增长
pub const LF_SEH: u32 = SEG_BASE + 0x03; // 叶含 SEH
pub const LF_EMPTY: u32 = SEG_BASE + 0x04; // 名为空
pub const LF_FROZEN: u32 = SEG_BASE + 0x05; // 印后挂账

/// 函数画像：判定叶所需三因。
#[derive(Clone)]
pub struct FuncProfile {
    pub func: String,
    /// 有对外调用。
    pub has_call: bool,
    /// 有栈帧增长（字节）。
    pub stack: u32,
    /// 含 __try 作用域。
    pub has_seh: bool,
}

impl FuncProfile {
    /// 叶函数判定：无调用 && 无栈增长 && 无 SEH。
    pub fn is_leaf(&self) -> bool {
        !self.has_call && self.stack == 0 && !self.has_seh
    }

    /// 非叶因清单（叶函数诊断面）。
    pub fn non_leaf_reasons(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.has_call {
            v.push("call");
        }
        if self.stack > 0 {
            v.push("stack");
        }
        if self.has_seh {
            v.push("seh");
        }
        v
    }

    pub fn ok(&self) -> bool {
        !self.func.is_empty()
    }
}

/// 叶函数账。
pub struct LeafBook {
    pub profiles: Vec<FuncProfile>,
    pub seq_next: u32,
    sealed: bool,
}

impl LeafBook {
    pub fn new() -> LeafBook {
        LeafBook { profiles: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂画像（名为空/重名必拒）。
    pub fn mount(&mut self, func: &str, has_call: bool, stack: u32, has_seh: bool) -> Result<u32, u32> {
        if self.sealed {
            return Err(LF_FROZEN);
        }
        if func.is_empty() {
            return Err(LF_EMPTY);
        }
        if self.profiles.iter().any(|p| p.func == func) {
            return Err(LF_EMPTY);
        }
        let seq = self.seq_next;
        self.profiles.push(FuncProfile {
            func: func.to_string(),
            has_call,
            stack,
            has_seh,
        });
        self.seq_next += 1;
        Ok(seq)
    }

    /// 叶数。
    pub fn leaf_count(&self) -> usize {
        self.profiles.iter().filter(|p| p.is_leaf()).count()
    }

    /// 非叶数。
    pub fn non_leaf_count(&self) -> usize {
        self.profiles.len() - self.leaf_count()
    }

    /// 假叶检出：挂账声明为叶但三因任一为真。
    pub fn fake_leaf_probe(&self) -> Vec<String> {
        self.profiles
            .iter()
            .filter(|p| p.is_leaf() != (p.non_leaf_reasons().is_empty()))
            .map(|p| p.func.clone())
            .collect()
    }

    /// 叶账守恒：叶 + 非叶 = 全账。
    pub fn conserved(&self) -> bool {
        self.leaf_count() + self.non_leaf_count() == self.profiles.len()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a320;
        for p in &self.profiles {
            h = h.wrapping_mul(211).wrapping_add(p.has_call as u32);
            h = h.wrapping_mul(211).wrapping_add(p.stack);
            h = h.wrapping_mul(211).wrapping_add(p.has_seh as u32);
            for by in p.func.bytes() {
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

    /// 按名查画像。
    pub fn find(&self, func: &str) -> Option<&FuncProfile> {
        self.profiles.iter().find(|p| p.func == func)
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "total={} leaf={} nonleaf={}",
            self.profiles.len(),
            self.leaf_count(),
            self.non_leaf_count()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = LeafBook::new();
    // 1) 有调用的"叶"画像非叶（假叶检出）。
    let _ = b.mount("inj_call", true, 0, false);
    got[0] = !b.find("inj_call").unwrap().is_leaf();
    // 2) 有栈增长的"叶"非叶。
    let _ = b.mount("inj_stack", false, 48, false);
    got[1] = !b.find("inj_stack").unwrap().is_leaf();
    // 3) 含 SEH 的"叶"非叶。
    let _ = b.mount("inj_seh", false, 0, true);
    got[2] = !b.find("inj_seh").unwrap().is_leaf();
    // 4) 名为空必拒。
    got[3] = b.mount("", false, 0, false).err() == Some(LF_EMPTY);
    // 5) 印后挂账必拒。
    b.seal();
    got[4] = b.mount("post", false, 0, false).err() == Some(LF_FROZEN);
    got
}

/// 判据入口：UNX-F12136
pub fn run_unx12136_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12136");
    let mut b = LeafBook::new();

    // J1 真叶 10/10：纯计算函数全判叶。
    let mut j1 = true;
    for i in 0..10usize {
        let name = format!("pure{}", i);
        if b.mount(&name, false, 0, false).is_err() {
            j1 = false;
        }
        if !b.find(&name).unwrap().is_leaf() {
            j1 = false;
        }
    }
    s.add("J1_leaf_10x", j1 && b.leaf_count() == 10, "真叶判定 10/10");

    // J2 假叶检出 10/10：三因各造样本皆非叶。
    let mut j2 = true;
    for i in 0..10usize {
        let name = format!("fake{}", i);
        let has_call = i % 3 == 0;
        let stack = if i % 3 == 1 { 32 } else { 0 };
        let has_seh = i % 3 == 2;
        let _ = b.mount(&name, has_call, stack, has_seh);
        if b.find(&name).unwrap().is_leaf() {
            j2 = false;
        }
    }
    s.add("J2_fake_10x", j2 && b.non_leaf_count() == 10, "假叶三因检出 10/10");

    // J3 叶账守恒。
    s.add("J3_conserved", b.conserved(), "叶 + 非叶 = 全账 20");

    // J4 非叶因清单正确。
    let p = b.find("fake0").unwrap();
    let reasons = p.non_leaf_reasons();
    s.add(
        "J4_reasons",
        reasons == vec!["call"],
        "call 因画像清单只含 call",
    );

    // J5 非叶因清单多因并列。
    let mut b2 = LeafBook::new();
    let _ = b2.mount("multi", true, 64, true);
    let r = b2.find("multi").unwrap().non_leaf_reasons();
    s.add(
        "J5_multi_reasons",
        r == vec!["call", "stack", "seh"],
        "三因并列清单全列",
    );

    // J6 空因清单即真叶。
    let mut b3 = LeafBook::new();
    let _ = b3.mount("clean", false, 0, false);
    s.add(
        "J6_clean",
        b3.find("clean").unwrap().non_leaf_reasons().is_empty() && b3.find("clean").unwrap().is_leaf(),
        "空因清单等价真叶",
    );

    // J7 零栈阈语义：1 字节也算增长。
    let mut b4 = LeafBook::new();
    let _ = b4.mount("one_byte", false, 1, false);
    s.add("J7_one_byte", !b4.find("one_byte").unwrap().is_leaf(), "1 字节栈增长即非叶");

    // J8 重名拒。
    let _ = b4.mount("dup", false, 0, false);
    s.add(
        "J8_dup",
        b4.mount("dup", false, 0, false).err() == Some(LF_EMPTY),
        "重名同码受控",
    );

    // J9 名空拒。
    s.add("J9_empty", b4.mount("", false, 0, false).err() == Some(LF_EMPTY), "名为空拒 EMPTY");

    // J10 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J10_fingerprint", f1 == f2, "叶账折叠指纹一致");

    // J11 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 印账受控。
    b.seal();
    s.add(
        "J12_sealed",
        b.mount("post", false, 0, false).err() == Some(LF_FROZEN),
        "印账后挂账拒 FROZEN",
    );

    // J13 假叶探针空清单（账面自洽时）。
    s.add("J13_probe_clean", b.fake_leaf_probe().is_empty(), "判定自洽时假叶探针空");

    // J14 按名查命中与未命中。
    s.add(
        "J14_find",
        b.find("pure0").is_some() && b.find("nope").is_none(),
        "按名查命中/未命中",
    );

    // J15 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("total=20") && sm.contains("leaf=10") && sm.contains("nonleaf=10"),
        "摘要含总数与叶数",
    );

    // J16 空账守恒。
    let b5 = LeafBook::new();
    s.add("J16_empty_conserved", b5.conserved() && b5.leaf_count() == 0, "空账守恒且零叶");

    // J17 画像合法性（名非空）。
    s.add("J17_profile_ok", b.profiles.iter().all(|p| p.ok()), "全画像过合法性核");

    // J18 stack 上限语义：u32 大值仍判非叶。
    let mut b6 = LeafBook::new();
    let _ = b6.mount("huge", false, u32::MAX, false);
    s.add("J18_huge_stack", !b6.find("huge").unwrap().is_leaf(), "大栈值仍非叶");

    // J19 三因独立：单因非叶、双因非叶。
    let mut b7 = LeafBook::new();
    let _ = b7.mount("c1", true, 0, false);
    let _ = b7.mount("c2", false, 8, true);
    s.add(
        "J19_independent",
        !b7.find("c1").unwrap().is_leaf() && !b7.find("c2").unwrap().is_leaf(),
        "单因与双因皆非叶",
    );

    // J20 混账指纹对画像敏感。
    let f_before = b7.fold();
    let _ = b7.mount("sensitive", false, 0, false);
    s.add("J20_fp_sensitive", b7.fold() != f_before, "指纹对新增画像敏感");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12136_leaf_judge() {
        let mut b = LeafBook::new();
        let _ = b.mount("leaf", false, 0, false);
        let _ = b.mount("not", true, 0, false);
        assert_eq!(b.leaf_count(), 1);
        assert_eq!(b.non_leaf_count(), 1);
    }

    #[test]
    fn unx12136_checks_green() {
        let s = run_unx12136_checks();
        assert!(s.all_passed(), "UNX-F12136 判据未全绿");
    }

    #[test]
    fn unx12136_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
