//! UNX-F10216 失败现场快照（FAIL 现场栈与寄存器摘要落盘，无快照拦截）（0x10e20 段）
//!
//! 锚点：UNX-F10216-J1 快照达成（FAIL 现场快照含栈与寄存器摘要
//! 落盘 10/10 次），无快照 10/10 次拦截。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：快照仓（栈摘要 + 寄存器摘要 + 帧链）+ 拍摄核（FAIL 即拍）
//! + 完整核（栈+寄存器两件齐备方为有效快照）+ 折叠指纹 +
//! 注入核（缺栈/缺寄存器/空帧/重复拍摄/封仓拍摄）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10e20;
/// 诊断码：栈摘要缺失
pub const ERR_NO_STACK: u32 = SEG_BASE + 0x01;
/// 诊断码：寄存器摘要缺失
pub const ERR_NO_REGS: u32 = SEG_BASE + 0x02;
/// 诊断码：用例为空
pub const ERR_EMPTY_CASE: u32 = SEG_BASE + 0x03;
/// 诊断码：重复拍摄
pub const ERR_DUP_SNAP: u32 = SEG_BASE + 0x04;
/// 诊断码：封仓后拍摄
pub const ERR_SEALED_SNAP: u32 = SEG_BASE + 0x05;

/// 寄存器摘要（内核面常见的八只）
#[derive(Clone, PartialEq, Eq)]
pub struct RegDigest {
    /// rax 摘要
    pub rax: u32,
    /// rbx 摘要
    pub rbx: u32,
    /// rcx 摘要
    pub rcx: u32,
    /// rdx 摘要
    pub rdx: u32,
    /// rsp 摘要
    pub rsp: u32,
    /// rbp 摘要
    pub rbp: u32,
    /// rsi 摘要
    pub rsi: u32,
    /// rdi 摘要
    pub rdi: u32,
}

impl RegDigest {
    /// 全零摘要（视为缺失）
    pub fn is_zero(&self) -> bool {
        self.rax == 0 && self.rbx == 0 && self.rcx == 0 && self.rdx == 0
            && self.rsp == 0 && self.rbp == 0 && self.rsi == 0 && self.rdi == 0
    }
    /// 摘要折叠
    pub fn fold(&self) -> u32 {
        let mut acc = 0x10e20u32;
        for v in [self.rax, self.rbx, self.rcx, self.rdx, self.rsp, self.rbp, self.rsi, self.rdi] {
            acc = acc.wrapping_mul(73856093).wrapping_add(v);
        }
        acc
    }
}

/// 现场快照
#[derive(Clone, PartialEq, Eq)]
pub struct CrashSnapshot {
    /// 用例标识
    pub case: String,
    /// 栈摘要（非空）
    pub stack_digest: String,
    /// 寄存器摘要
    pub regs: RegDigest,
    /// 帧链（由深到浅）
    pub frames: Vec<String>,
}

impl CrashSnapshot {
    /// 快照完整：栈摘要非空、寄存器非全零、帧链非空
    pub fn complete(&self) -> bool {
        !self.case.is_empty()
            && !self.stack_digest.is_empty()
            && !self.regs.is_zero()
            && !self.frames.is_empty()
    }
    /// 快照折叠
    pub fn fold(&self) -> u32 {
        let mut acc = self.regs.fold();
        for b in self.stack_digest.as_bytes() {
            acc = acc ^ (*b as u32);
            acc = acc.wrapping_mul(16777619);
        }
        for (i, f) in self.frames.iter().enumerate() {
            acc = acc.wrapping_mul(2654435761).wrapping_add((f.len() as u32) + i as u32);
        }
        acc
    }
}

/// 快照仓
pub struct SnapStore {
    /// 已落盘快照
    pub snaps: Vec<CrashSnapshot>,
    /// 封仓标记
    pub sealed: bool,
}

impl SnapStore {
    /// 开仓
    pub fn new() -> Self {
        SnapStore { snaps: Vec::new(), sealed: false }
    }

    /// 拍摄：FAIL 用例现场
    pub fn capture(&mut self, case: &str, stack: &str, regs: RegDigest, frames: &[&str]) -> Result<(), u32> {
        if self.sealed {
            return Err(ERR_SEALED_SNAP);
        }
        if case.is_empty() {
            return Err(ERR_EMPTY_CASE);
        }
        if stack.is_empty() {
            return Err(ERR_NO_STACK);
        }
        if regs.is_zero() {
            return Err(ERR_NO_REGS);
        }
        if self.snaps.iter().any(|x| x.case == case) {
            return Err(ERR_DUP_SNAP);
        }
        self.snaps.push(CrashSnapshot {
            case: case.to_string(),
            stack_digest: stack.to_string(),
            regs,
            frames: frames.iter().map(|f| f.to_string()).collect(),
        });
        Ok(())
    }

    /// 无快照拦截：查询某失败用例是否缺快照
    pub fn missing_for(&self, case: &str) -> bool {
        !self.snaps.iter().any(|x| x.case == case)
    }

    /// 完整率（基点）
    pub fn completeness_bps(&self) -> u32 {
        if self.snaps.is_empty() {
            return 0;
        }
        let c = self.snaps.iter().filter(|x| x.complete()).count();
        (c * 10000 / self.snaps.len()) as u32
    }

    /// 最深帧链长度
    pub fn deepest_frames(&self) -> usize {
        let mut best = 0;
        for x in &self.snaps {
            if x.frames.len() > best {
                best = x.frames.len();
            }
        }
        best
    }

    /// 封仓
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        let mut acc = SEG_BASE + 0x99;
        for (i, x) in self.snaps.iter().enumerate() {
            acc = acc.wrapping_mul(48611).wrapping_add(x.fold() >> (i % 3));
        }
        acc
    }

    /// 诊断面：帧链含内核关键帧判定
    pub fn chain_touches_kernel(&self, case: &str, kernel_frame: &str) -> bool {
        match self.snaps.iter().find(|x| x.case == case) {
            Some(x) => x.frames.iter().any(|f| f.contains(kernel_frame)),
            None => false,
        }
    }

    /// 诊断面：帧链去重计数
    pub fn distinct_frame_count(&self) -> usize {
        let mut seen: Vec<&String> = Vec::new();
        for x in &self.snaps {
            for f in &x.frames {
                if !seen.contains(&f) {
                    seen.push(f);
                }
            }
        }
        seen.len()
    }

    /// 诊断面：某用例寄存器回读
    pub fn regs_of(&self, case: &str) -> Option<RegDigest> {
        self.snaps.iter().find(|x| x.case == case).map(|x| x.regs.clone())
    }

    /// 诊断面：快照摘要文本
    pub fn snap_summary(&self, case: &str) -> String {
        match self.snaps.iter().find(|x| x.case == case) {
            Some(x) => format!(
                "case={} frames={} fp={:X}",
                x.case,
                x.frames.len(),
                x.fold()
            ),
            None => "missing".to_string(),
        }
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    let regs_ok = RegDigest { rax: 1, rbx: 2, rcx: 3, rdx: 4, rsp: 5, rbp: 6, rsi: 7, rdi: 8 };
    // 1: 缺栈摘要
    let mut s1 = SnapStore::new();
    out[0] = s1.capture("c1", "", regs_ok.clone(), &["f"]) == Err(ERR_NO_STACK);
    // 2: 缺寄存器摘要（全零）
    let mut s2 = SnapStore::new();
    let zero = RegDigest { rax: 0, rbx: 0, rcx: 0, rdx: 0, rsp: 0, rbp: 0, rsi: 0, rdi: 0 };
    out[1] = s2.capture("c2", "stk", zero, &["f"]) == Err(ERR_NO_REGS);
    // 3: 空用例
    let mut s3 = SnapStore::new();
    out[2] = s3.capture("", "stk", regs_ok.clone(), &["f"]) == Err(ERR_EMPTY_CASE);
    // 4: 重复拍摄
    let mut s4 = SnapStore::new();
    let _ = s4.capture("dup", "stk", regs_ok.clone(), &["f"]);
    out[3] = s4.capture("dup", "stk2", regs_ok.clone(), &["f"]) == Err(ERR_DUP_SNAP);
    // 5: 封仓后拍摄
    let mut s5 = SnapStore::new();
    let _ = s5.capture("late", "stk", regs_ok.clone(), &["f"]);
    let _ = s5.seal();
    out[4] = s5.capture("late2", "stk", regs_ok, &["f"]) == Err(ERR_SEALED_SNAP);
    out
}

/// 判据函数
pub fn run_unx10216_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10216");
    let mut st = SnapStore::new();
    let regs_a = RegDigest { rax: 0xA1, rbx: 0xB2, rcx: 0xC3, rdx: 0xD4, rsp: 0x7F00, rbp: 0x7E00, rsi: 0x11, rdi: 0x22 };
    let regs_b = RegDigest { rax: 0x10, rbx: 0x20, rcx: 0x30, rdx: 0x40, rsp: 0x7C00, rbp: 0x7B00, rsi: 0x33, rdi: 0x44 };
    let c1 = st.capture("ut-fail-a", "stack: kmain+0x40", regs_a.clone(), &["kentry", "kmain", "case_a"]);
    let c2 = st.capture("ut-fail-b", "stack: kirq+0x08", regs_b.clone(), &["kirq", "handler"]);
    s.add("双快照落盘", c1.is_ok() && c2.is_ok() && st.snaps.len() == 2, "两份现场快照落盘");
    s.add("快照完整", st.snaps.iter().all(|x| x.complete()), "栈+寄存器+帧链齐备");
    s.add("完整率满格", st.completeness_bps() == 10000, "完整率 10000 基点");
    s.add("最深帧链三条", st.deepest_frames() == 3, "最深快照含三帧");
    s.add("缺快照拦截", st.missing_for("ut-fail-c") && !st.missing_for("ut-fail-a"), "未拍用例命中缺件清单");
    s.add("寄存器折叠差异", regs_a.fold() != regs_b.fold(), "不同寄存器摘要折叠不同");
    s.add("零摘要判缺", RegDigest { rax: 0, rbx: 0, rcx: 0, rdx: 0, rsp: 0, rbp: 0, rsi: 0, rdi: 0 }.is_zero(), "全零摘要判为缺失");
    s.add("折叠指纹非零", st.snaps.iter().all(|x| x.fold() != 0), "各快照折叠非零");
    s.add("快照可克隆", st.snaps[0].clone() == st.snaps[0], "快照克隆自等");
    s.add("关键帧命中", st.chain_touches_kernel("ut-fail-a", "kmain") && !st.chain_touches_kernel("ut-fail-b", "kmain"), "A 链含 kmain、B 链不含");
    s.add("帧去重计数", st.distinct_frame_count() == 5, "五帧全异无重复");
    s.add("寄存器回读", st.regs_of("ut-fail-a") == Some(regs_a.clone()) && st.regs_of("none").is_none(), "在册可回读寄存器、缺席为 None");
    s.add("摘要文本", st.snap_summary("ut-fail-a").contains("frames=3"), "摘要文本含帧数");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let seal_v = st.seal();
    s.add("封仓后拍摄拒绝", st.capture("late", "stk", regs_a, &["f"]) == Err(ERR_SEALED_SNAP), "封仓后拍摄被拒");
    s.add("封仓指纹非零", seal_v != 0, "封仓折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regs(seed: u32) -> RegDigest {
        RegDigest {
            rax: seed, rbx: seed + 1, rcx: seed + 2, rdx: seed + 3,
            rsp: 0x7F00, rbp: 0x7E00, rsi: seed + 4, rdi: seed + 5,
        }
    }

    #[test]
    fn capture_and_missing() {
        let mut st = SnapStore::new();
        let _ = st.capture("x", "stk", regs(1), &["f1"]);
        assert!(!st.missing_for("x"));
        assert!(st.missing_for("y"));
        assert_eq!(st.deepest_frames(), 1);
    }

    #[test]
    fn fold_differs_by_case() {
        let a = CrashSnapshot {
            case: "a".to_string(),
            stack_digest: "s".to_string(),
            regs: regs(1),
            frames: vec!["f".to_string()],
        };
        let b = CrashSnapshot { case: "b".to_string(), ..a.clone() };
        assert_ne!(a.fold(), b.fold());
        assert!(a.complete());
    }

    #[test]
    fn kernel_frame_and_regs_roundtrip() {
        let mut st = SnapStore::new();
        let _ = st.capture("k", "stk", regs(9), &["kentry", "kmain"]);
        assert!(st.chain_touches_kernel("k", "kentry"));
        assert!(!st.chain_touches_kernel("k", "nosuch"));
        assert_eq!(st.regs_of("k").unwrap().rax, 9);
        assert!(st.snap_summary("k").contains("frames=2"));
    }

    #[test]
    fn zero_regs_rejected_on_capture() {
        let mut st = SnapStore::new();
        let z = RegDigest { rax: 0, rbx: 0, rcx: 0, rdx: 0, rsp: 0, rbp: 0, rsi: 0, rdi: 0 };
        assert_eq!(st.capture("z", "stk", z, &["f"]), Err(ERR_NO_REGS));
        assert!(st.snaps.is_empty());
    }
}
