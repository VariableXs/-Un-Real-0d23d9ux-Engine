//! UNX-F9140 ktest syscall 面 B17 批断言集（0x63140 段）
//!
//! 锚点：UNX-F9140-J1 断言账（B17 批 syscall 断言集——断言号 × 断言
//! 类别 × 目标单 × 通过态逐条落账）；J2 断言语义（类别闭集 + 目标单
//! 存在 + 全通过三性质）；J3 断言指纹（断言账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B17（ktest syscall 断言面）。
//!
//! 结构：断言类别闭集 + 断言账本 + 断言核（类别闸 + 目标闸 + 显式闸）
//! + 断言指纹 + 注入核（未知类别/目标缺失/未过未显式/重断言号/印后
//! 断言必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63140 独占 0x20）。
pub const SEG_BASE: u32 = 0x63140;
pub const KT_UNK_KIND: u32 = SEG_BASE + 0x01; // 未知断言类别
pub const KT_TARGET_MISS: u32 = SEG_BASE + 0x02; // 目标缺失
pub const KT_INEXPLICIT: u32 = SEG_BASE + 0x03; // 未过未显式
pub const KT_DUP_NO: u32 = SEG_BASE + 0x04; // 重断言号
pub const KT_FROZEN: u32 = SEG_BASE + 0x05; // 印后断言

/// 断言类别闭集。
pub const K_SIGNAL: usize = 0; // 信号族断言
pub const K_TIMER: usize = 1; // 定时器族断言
pub const K_FILE: usize = 2; // 文件族断言
pub const K_PROC: usize = 3; // 进程族断言
pub const K_COUNT: usize = 4;

pub const KIND_NAMES: [&str; K_COUNT] = ["signal", "timer", "file", "proc"];

/// B17 批目标单闭集（前置批卡号段）。
pub const TARGETS: [&str; 6] = ["B17-SIG", "B17-TMR", "B17-FILE", "B17-PROC", "B17-E2E", "B17-REG"];

/// 单条断言。
#[derive(Clone)]
pub struct KtAssert {
    pub no: u32,
    pub kind: usize,
    pub target: String,
    pub passed: bool,
    pub note: String,
}

impl KtAssert {
    /// 断言合法：类别闭集 + 目标在闭集 + 未过必显式记 note。
    pub fn ok(&self) -> bool {
        self.kind < K_COUNT
            && TARGETS.contains(&self.target.as_str())
            && (self.passed || !self.note.is_empty())
    }

    fn render_line(&self) -> String {
        format!(
            "A{}|{}@{}|{}",
            self.no,
            KIND_NAMES[self.kind],
            self.target,
            if self.passed { "PASS" } else { "FAIL" }
        )
    }
}

/// 断言账本。
pub struct KtLedger {
    asserts: Vec<KtAssert>,
    sealed: bool,
}

impl KtLedger {
    pub fn new() -> Self {
        KtLedger {
            asserts: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条断言：合法性闸 + 重号拒 + 印后拒。
    pub fn add(&mut self, a: KtAssert) -> Result<(), u32> {
        if self.sealed {
            return Err(KT_FROZEN);
        }
        if a.kind >= K_COUNT {
            return Err(KT_UNK_KIND);
        }
        if !TARGETS.contains(&a.target.as_str()) {
            return Err(KT_TARGET_MISS);
        }
        if !a.passed && a.note.is_empty() {
            return Err(KT_INEXPLICIT);
        }
        if self.asserts.iter().any(|x| x.no == a.no) {
            return Err(KT_DUP_NO);
        }
        self.asserts.push(a);
        Ok(())
    }

    pub fn entry(&self, no: u32) -> Option<&KtAssert> {
        self.asserts.iter().find(|x| x.no == no)
    }

    pub fn len(&self) -> usize {
        self.asserts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.asserts.is_empty()
    }

    /// 未过清单。
    pub fn failed(&self) -> Vec<u32> {
        self.asserts.iter().filter(|x| !x.passed).map(|x| x.no).collect()
    }

    /// 全通过核。
    pub fn all_pass(&self) -> bool {
        !self.asserts.is_empty() && self.failed().is_empty()
    }

    /// 各类别计数。
    pub fn kind_counts(&self) -> [usize; K_COUNT] {
        let mut c = [0usize; K_COUNT];
        for x in &self.asserts {
            if x.kind < K_COUNT {
                c[x.kind] += 1;
            }
        }
        c
    }

    /// 断言指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.asserts {
            h = h.wrapping_mul(0x0100_0193) ^ x.no;
            h = h.wrapping_mul(0x0100_0193) ^ x.kind as u32;
            for b in x.target.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            h ^= x.passed as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.all_pass() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for x in &self.asserts {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("B17KT-SEAL\n");
        }
        out
    }
}

impl Default for KtLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：八条断言（四类别各二，全过）。
fn standard_ledger() -> KtLedger {
    let mut l = KtLedger::new();
    let spec: [(u32, usize, &str); 8] = [
        (1, K_SIGNAL, "B17-SIG"),
        (2, K_SIGNAL, "B17-REG"),
        (3, K_TIMER, "B17-TMR"),
        (4, K_TIMER, "B17-REG"),
        (5, K_FILE, "B17-FILE"),
        (6, K_FILE, "B17-REG"),
        (7, K_PROC, "B17-PROC"),
        (8, K_PROC, "B17-E2E"),
    ];
    for (no, kind, target) in spec {
        let _ = l.add(KtAssert {
            no,
            kind,
            target: target.to_string(),
            passed: true,
            note: String::new(),
        });
    }
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    let mk = |no: u32, kind: usize, target: &str, passed: bool| KtAssert {
        no,
        kind,
        target: target.to_string(),
        passed,
        note: String::new(),
    };
    // 注 1：未知类别
    let mut l = KtLedger::new();
    if l.add(mk(1, 9, "B17-SIG", true)) == Err(KT_UNK_KIND) {
        caught += 1;
    }
    // 注 2：目标缺失
    let mut l = KtLedger::new();
    if l.add(mk(1, K_SIGNAL, "B17-NOPE", true)) == Err(KT_TARGET_MISS) {
        caught += 1;
    }
    // 注 3：未过未显式
    let mut l = KtLedger::new();
    if l.add(mk(1, K_SIGNAL, "B17-SIG", false)) == Err(KT_INEXPLICIT) {
        caught += 1;
    }
    // 注 4：重断言号
    let mut l = standard_ledger();
    if l.add(mk(1, K_SIGNAL, "B17-SIG", true)) == Err(KT_DUP_NO) {
        caught += 1;
    }
    // 注 5：印后断言
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.add(mk(9, K_SIGNAL, "B17-SIG", true)) == Err(KT_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 断言账；J2 断言语义；J3 断言指纹。
pub fn run_unx9140_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9140");
    // J1-1 八条全落账
    let l = standard_ledger();
    let ok = l.len() == 8 && (1..=8).all(|n| l.entry(n).is_some());
    s.add("9140-J1-八条全落账", ok, if ok { "" } else { "断言账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("A1|signal@B17-SIG|PASS") && text.contains("A8|proc@B17-E2E|PASS") && text.contains("B17KT-SEAL");
    s.add("9140-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 合法性（全部）
    let ok = (1..=8).all(|n| l.entry(n).map(|x| x.ok()).unwrap_or(false));
    s.add("9140-J2-断言合法性", ok, if ok { "" } else { "合法性漂移" });
    // J2-2 全通过
    let ok = l.all_pass() && l.failed().is_empty();
    s.add("9140-J2-全通过", ok, if ok { "" } else { "有未过项" });
    // J2-3 类别计数（各 2）
    let kc = l.kind_counts();
    let ok = kc == [2, 2, 2, 2];
    let detail = if ok { String::new() } else { format!("{},{},{},{}", kc[0], kc[1], kc[2], kc[3]) };
    s.add_dyn("9140-J2-类别计数", kc == [2, 2, 2, 2], &detail);
    // J2-4 目标覆盖（六目标全被引用）
    let mut seen: Vec<&str> = Vec::new();
    for t in TARGETS.iter() {
        if l.asserts.iter().any(|x| x.target == *t) {
            seen.push(t);
        }
    }
    let ok = seen.len() == TARGETS.len();
    s.add("9140-J2-目标覆盖", ok, if ok { "" } else { "目标缺引用" });
    // J2-5 未过显式闸
    let mut l3 = KtLedger::new();
    let ok = l3
        .add(KtAssert {
            no: 1,
            kind: K_TIMER,
            target: "B17-TMR".to_string(),
            passed: false,
            note: "timer-drift".to_string(),
        })
        .is_ok();
    s.add("9140-J2-未过显式", ok, if ok { "" } else { "显式闸失效" });
    // J3-1 断言指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9140-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 断言变签变
    let mut l4 = standard_ledger();
    let fp0 = l4.ledger_fp();
    if let Some(x) = l4.asserts.first_mut() {
        x.passed = false;
        x.note = "flip".to_string();
    }
    let ok = l4.ledger_fp() != fp0;
    s.add("9140-J3-断言变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l5 = standard_ledger();
    let ok = l5.seal() && l5.sealed();
    s.add("9140-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒断言
    let ok = l5.add(mk_helper(9)) == Err(KT_FROZEN);
    s.add("9140-J3-印后拒断言", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9140-注入核必抓", ok, &detail);
    s
}

fn mk_helper(no: u32) -> KtAssert {
    KtAssert {
        no,
        kind: K_SIGNAL,
        target: "B17-SIG".to_string(),
        passed: true,
        note: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9140_all_checks_green() {
        let set = run_unx9140_checks();
        assert!(set.all_passed(), "UNX-F9140 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9140_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
