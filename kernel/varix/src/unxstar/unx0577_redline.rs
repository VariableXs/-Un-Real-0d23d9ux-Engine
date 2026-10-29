//! UNX-F0577 · 电源红线总账（禁令族汇总）· 诊断段 0x77A0（本单独占）
//!
//! 判据原文摘录：UNX-F0577-J1 红线汇总账齐（禁令 5 类——PM 写/入眠/CMOS 写/
//! 电源 MSR 写/C 态进入），逐类拦截计数复算一致；③ 完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 禁令五类闭集：PM 写（0）/入眠（1）/CMOS 写（2）/电源 MSR 写（3）/C 态
//!     进入（4），类号越界一律拒收。
//!  3. 拦截计数复算一致：每类触发账（记一次）与拦截账（拦一次）逐类相等，
//!     总账 = 五类之和，双向对冲。
//!  4. 红线语义：五类操作在内核运行态一律拒绝，白名单（镜像/采集器）除外。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x77A0;
/// 禁令类号越闭集（0..=4 之外）
pub const E_CLASS_RANGE: u16 = SEG_BASE + 1;
/// 汇总账已封板，重封拦截
pub const E_SEALED: u16 = SEG_BASE + 2;
/// 触发账与拦截账逐类不等
pub const E_MIRROR_DIFF: u16 = SEG_BASE + 3;
/// 总账与五类之和不守恒
pub const E_TOTAL_DIFF: u16 = SEG_BASE + 4;
/// 逐类计数复算不一致
pub const E_RECOUNT_DIFF: u16 = SEG_BASE + 5;
/// 白名单外放行（红线穿透）
pub const E_PASS_THROUGH: u16 = SEG_BASE + 6;
/// 汇总摘要为空
pub const E_SUMMARY_EMPTY: u16 = SEG_BASE + 7;

/// 禁令五类闭集。
pub const N_CLASSES: usize = 5;
/// 类名表（下标即类号）。
pub const CLASS_NAMES: [&str; 5] =
    ["PM写", "入眠", "CMOS写", "电源MSR写", "C态进入"];

/// 触发源闭集（谁想越线）。
pub const SRC_KERNEL: u8 = 0;
pub const SRC_DRIVER: u8 = 1;
pub const SRC_USER: u8 = 2;
pub const SRC_MIRROR: u8 = 3; // 镜像通道（白名单）

/// 单类红线账：触发计数与拦截计数双向对冲。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassLedger {
    /// 类号（0..=4 闭集）
    pub class: u8,
    /// 触发计数（试图越线次数）
    pub fired: u32,
    /// 拦截计数（实际拦下次数）
    pub blocked: u32,
    /// 白名单放行计数（镜像通道）
    pub passed: u32,
}

impl ClassLedger {
    pub const fn new(class: u8) -> Self {
        ClassLedger { class, fired: 0, blocked: 0, passed: 0 }
    }
    /// 守恒恒等式：fired == blocked + passed（每次触发要么拦要么白名单放行）。
    pub fn conserved(&self) -> bool {
        self.fired == self.blocked.wrapping_add(self.passed)
    }
    /// 复算一致：fired 与 blocked 同步增长（白名单除外）。
    pub fn recount_ok(&self) -> bool {
        self.blocked.wrapping_add(self.passed) == self.fired
    }
}

/// 红线汇总总账：五类账 + 封板位 + 摘要。
pub struct RedlineBook {
    /// 五类账（定容，类号即下标）
    pub rows: [ClassLedger; N_CLASSES],
    /// 封板位（true 后拒收一切新触发）
    pub sealed: bool,
    /// 最近一次摘要（封板时固化）
    pub summary: Option<String>,
}

impl RedlineBook {
    pub fn new() -> Self {
        RedlineBook {
            rows: [
                ClassLedger::new(0),
                ClassLedger::new(1),
                ClassLedger::new(2),
                ClassLedger::new(3),
                ClassLedger::new(4),
            ],
            sealed: false,
            summary: None,
        }
    }

    /// 触发一次越线企图：返回 Ok(true) 表示拦截，Ok(false) 表示白名单放行。
    pub fn fire(&mut self, class: u8, src: u8) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if class as usize >= N_CLASSES {
            return Err(E_CLASS_RANGE);
        }
        let row = &mut self.rows[class as usize];
        row.fired = row.fired.wrapping_add(1);
        if src == SRC_MIRROR {
            // 白名单通道：镜像采集只读不改，放行但留账
            row.passed = row.passed.wrapping_add(1);
            Ok(false)
        } else {
            // 内核/驱动/用户一律拦截
            row.blocked = row.blocked.wrapping_add(1);
            Ok(true)
        }
    }

    /// 逐类复算：五类守恒恒等式全过才准封板。
    pub fn recount_all(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < N_CLASSES {
            if !self.rows[i].conserved() {
                return Err(E_TOTAL_DIFF);
            }
            if !self.rows[i].recount_ok() {
                return Err(E_RECOUNT_DIFF);
            }
            i += 1;
        }
        Ok(())
    }

    /// 总账：五类触发之和。
    pub fn total_fired(&self) -> u32 {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < N_CLASSES {
            s = s.wrapping_add(self.rows[i].fired);
            i += 1;
        }
        s
    }

    /// 总拦截数。
    pub fn total_blocked(&self) -> u32 {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < N_CLASSES {
            s = s.wrapping_add(self.rows[i].blocked);
            i += 1;
        }
        s
    }

    /// 封板：复算全过 + 摘要非空才落板。
    pub fn seal(&mut self) -> Result<&str, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.recount_all()?;
        // 摘要：类名 x 计数 五段拼接
        let mut s = String::new();
        let mut i = 0usize;
        while i < N_CLASSES {
            if i > 0 {
                s.push('|');
            }
            s.push_str(CLASS_NAMES[i]);
            s.push('=');
            let n = self.rows[i].blocked;
            // u32 → 十进制（无 format! 依赖的手工展开）
            if n == 0 {
                s.push('0');
            } else {
                let mut digits: Vec<u8> = Vec::new();
                let mut x = n;
                while x > 0 {
                    digits.push((x % 10) as u8);
                    x /= 10;
                }
                let mut j = digits.len();
                while j > 0 {
                    j -= 1;
                    s.push((b'0' + digits[j]) as char);
                }
            }
            i += 1;
        }
        if s.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        self.summary = Some(s);
        self.sealed = true;
        let idx = self.summary.as_ref();
        match idx {
            Some(v) => Ok(v.as_str()),
            None => Err(E_SUMMARY_EMPTY),
        }
    }

    /// 摘要读取（封板后可用）。
    pub fn summary_str(&self) -> Result<&str, u16> {
        match &self.summary {
            Some(v) => Ok(v.as_str()),
            None => Err(E_SUMMARY_EMPTY),
        }
    }
}

impl Default for RedlineBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 演练剧本：五类各 3 次内核触发（全拦）+ 镜像通道 2 次放行 → 全部对冲。
pub fn rehearsal() -> Result<(u32, u32), u16> {
    let mut book = RedlineBook::new();
    let mut c: u8 = 0;
    while c < N_CLASSES as u8 {
        let mut k = 0u32;
        while k < 3 {
            let blocked = book.fire(c, SRC_KERNEL)?;
            if !blocked {
                return Err(E_PASS_THROUGH);
            }
            k += 1;
        }
        c += 1;
    }
    // 镜像白名单：C 态进入类放行 2 次（只读采集）
    let mut m = 0u32;
    while m < 2 {
        let blocked = book.fire(4, SRC_MIRROR)?;
        if blocked {
            return Err(E_PASS_THROUGH);
        }
        m += 1;
    }
    book.recount_all()?;
    Ok((book.total_fired(), book.total_blocked()))
}

/// 汇总账对冲复核：total_fired == total_blocked + total_passed。
pub fn hedge_ok(book: &RedlineBook) -> bool {
    let mut passed: u32 = 0;
    let mut i = 0usize;
    while i < N_CLASSES {
        passed = passed.wrapping_add(book.rows[i].passed);
        i += 1;
    }
    book.total_fired() == book.total_blocked().wrapping_add(passed)
}

/// 自例程：演练剧本 + 对冲 + 封板 + 摘要非空 → 0。
pub fn drill() -> Result<u32, u16> {
    let (fired, blocked) = rehearsal()?;
    // 5 类 × 3 内核触发 + 2 镜像放行 = 17 触发，拦 15 放 2
    if fired != 17 || blocked != 15 {
        return Err(E_MIRROR_DIFF);
    }
    let mut book = RedlineBook::new();
    let _ = book.fire(0, SRC_DRIVER)?;
    let _ = book.fire(2, SRC_USER)?;
    let _ = book.fire(4, SRC_MIRROR)?;
    if !hedge_ok(&book) {
        return Err(E_TOTAL_DIFF);
    }
    let s = book.seal()?;
    if s.is_empty() {
        return Err(E_SUMMARY_EMPTY);
    }
    // 封板后拒收
    if book.fire(1, SRC_KERNEL) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    // 类号越界拒收
    let mut book2 = RedlineBook::new();
    if book2.fire(5, SRC_KERNEL) != Err(E_CLASS_RANGE) {
        return Err(E_CLASS_RANGE);
    }
    Ok(0)
}
