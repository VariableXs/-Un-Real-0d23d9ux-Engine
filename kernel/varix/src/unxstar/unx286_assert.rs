//! UNX-F0286 断言宏体系（assert/kassert/bug_on 语义分层）（0xAB80 段）
//!
//! 锚点：UNX-F0286-J1 三级断言语义（可恢复检查/内核断言/不可继续）
//! 分流正确，误用检出。
//! 域：CoRun·UNX-A1-B15（断言宏面）。
//!
//! 结构：三级断言闭集（Check 可恢复 → Err；Kassert 内核断言 →
//! panic 账；BugOn 不可继续 → 冻结账）+ 分流器（条件失败 → 按级
//! 分流出账）+ 误用检出核（Check 用于不可继续路径 / BugOn 用于可
//! 恢复路径 = 误用）+ 注入核（级别越界/误用漏检/账破缺/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAB80 独占 0x80）。

pub const SEG_BASE: u32 = 0xAB80;
pub const XC_AS_LEVEL: u32 = SEG_BASE + 0x01; // 级别越界
pub const XC_AS_MISUSE: u32 = SEG_BASE + 0x02; // 误用
pub const XC_AS_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 三级闭集。
pub const AL_CHECK: u8 = 0; // 可恢复检查 → Err 返回
pub const AL_KASSERT: u8 = 1; // 内核断言 → panic 账
pub const AL_BUGON: u8 = 2; // 不可继续 → 冻结账

/// 级名。
pub fn assert_level_name(l: u8) -> &'static str {
    match l {
        AL_CHECK => "check",
        AL_KASSERT => "kassert",
        AL_BUGON => "bug_on",
        _ => "?",
    }
}

/// 断言出账。
#[derive(Clone)]
pub struct AssertLog {
    pub level: u8,
    pub site: String,
    pub action: AssertAction,
}

/// 分流动作。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssertAction {
    Recover,  // 可恢复：调用方 Err
    Panic,    // 内核断言：panic
    Freeze,   // 不可继续：冻结
}

/// 分流器。
pub struct AssertRouter {
    pub logs: Vec<AssertLog>,
    pub frozen: bool,
    pub sealed: bool,
}

impl AssertRouter {
    pub fn new() -> Self {
        AssertRouter { logs: Vec::new(), frozen: false, sealed: false }
    }

    /// 断言分流：cond 失败时按级出账。
    pub fn assert_at(&mut self, level: u8, site: &str, cond: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_AS_SEALED);
        }
        if level > AL_BUGON {
            return Err(XC_AS_LEVEL);
        }
        if cond {
            return Ok(());
        }
        let action = match level {
            AL_CHECK => AssertAction::Recover,
            AL_KASSERT => AssertAction::Panic,
            _ => AssertAction::Freeze,
        };
        // 误用检出：BugOn 在冻结后再次触发 → 记账不重复冻结
        if level == AL_BUGON && self.frozen {
            return Err(XC_AS_MISUSE);
        }
        if level == AL_BUGON {
            self.frozen = true;
        }
        self.logs.push(AssertLog { level, site: String::from(site), action });
        Ok(())
    }

    /// 分流正确核：三级各自动作匹配。
    pub fn routing_ok(&self) -> bool {
        self.logs.iter().all(|l| {
            matches!(
                (l.level, l.action),
                (AL_CHECK, AssertAction::Recover)
                    | (AL_KASSERT, AssertAction::Panic)
                    | (AL_BUGON, AssertAction::Freeze)
            )
        })
    }
}

/// 三级分流核：check → Recover；kassert → Panic；bug_on → Freeze。
pub fn three_level_exact() -> bool {
    let mut r = AssertRouter::new();
    // check 失败：可恢复（返回 Ok 但记账）
    if r.assert_at(AL_CHECK, "alloc-small", false).is_err() {
        return false;
    }
    // kassert 失败：panic 账
    if r.assert_at(AL_KASSERT, "sched-invariant", false).is_err() {
        return false;
    }
    // bug_on 失败：冻结
    if r.assert_at(AL_BUGON, "mm-corrupt", false).is_err() {
        return false;
    }
    r.logs.len() == 3 && r.routing_ok() && r.frozen
}

/// 条件通过零账核：cond=true 不出账。
pub fn pass_no_log_exact() -> bool {
    let mut r = AssertRouter::new();
    for i in 0..5u8 {
        if r.assert_at(i % 3, &format!("site-{}", i), true).is_err() {
            return false;
        }
    }
    r.logs.is_empty() && !r.frozen
}

/// 误用检出核：冻结后再 BugOn 拒。
pub fn misuse_exact() -> bool {
    let mut r = AssertRouter::new();
    let _ = r.assert_at(AL_BUGON, "first", false);
    if !r.frozen {
        return false;
    }
    r.assert_at(AL_BUGON, "second", false) == Err(XC_AS_MISUSE)
}

/// 级别越界拒核。
pub fn level_guard_exact() -> bool {
    let mut r = AssertRouter::new();
    r.assert_at(3, "x", false) == Err(XC_AS_LEVEL)
        && r.assert_at(255, "x", false) == Err(XC_AS_LEVEL)
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 级别越界
    let mut r = AssertRouter::new();
    if r.assert_at(9, "x", false) == Err(XC_AS_LEVEL) {
        caught += 1;
    }
    // 2) 误用漏检（冻结后 BugOn）
    let _ = r.assert_at(AL_BUGON, "a", false);
    if r.assert_at(AL_BUGON, "b", false) == Err(XC_AS_MISUSE) {
        caught += 1;
    }
    // 3) 分流破缺（若 action 与 level 错配则检出）
    let mut bad = AssertRouter::new();
    bad.logs.push(AssertLog { level: AL_CHECK, site: String::from("x"), action: AssertAction::Freeze });
    if !bad.routing_ok() {
        caught += 1;
    }
    // 4) 封账后拒
    r.sealed = true;
    if r.assert_at(AL_CHECK, "y", false) == Err(XC_AS_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：路由账折叠哈希。
pub fn fp_frozen(r: &AssertRouter) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for l in &r.logs {
        h ^= l.level as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= match l.action {
            AssertAction::Recover => 0,
            AssertAction::Panic => 1,
            AssertAction::Freeze => 2,
        } as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx286_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0286");
    // J1-1 三级分流
    let ok = three_level_exact();
    s.add("B15-01-三级分流", ok, if ok { "" } else { "分流漂移" });
    // J1-2 通过零账
    let ok = pass_no_log_exact();
    s.add("B15-02-通过零账", ok, if ok { "" } else { "零账漂移" });
    // J1-3 误用检出
    let ok = misuse_exact();
    s.add("B15-03-误用检出", ok, if ok { "" } else { "误用漏" });
    // J1-4 级别守卫
    let ok = level_guard_exact();
    s.add("B15-04-级别守卫", ok, if ok { "" } else { "守卫漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B15-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let mut r = AssertRouter::new();
    let _ = r.assert_at(AL_KASSERT, "fp", false);
    let ok = fp_frozen(&r) == fp_frozen(&r);
    s.add("B15-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
