//! UNX-F9142 waitpid/wait3 兼容面——WNOHANG/WUNTRACED/WCONTINUED（0x63180 段）
//!
//! 锚点：UNX-F9142-J1 兼容账（waitpid/wait3 选项组合兼容面——等待号 ×
//! 选项集 × 匹配态 × 返回档逐次落账）；J2 兼容语义（选项位闭集 + 匹
//! 配态推导 + 返回档闭集三性质）；J3 兼容指纹（兼容账折叠指纹）三判
//! 据全绿。
//! 域：CoRun·UNX-C2-B19（wait 族选项兼容面）。
//!
//! 结构：选项位闭集 + 匹配核（事件 × 选项 → 返回档）+ 兼容账本 + 匹
//! 配指纹 + 注入核（未知选项/零选项挂等/状态字簇过滤/重等待号/印后等
//! 待必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63180 独占 0x20）。
pub const SEG_BASE: u32 = 0x63180;
pub const WP_UNK_OPT: u32 = SEG_BASE + 0x01; // 未知选项
pub const WP_ZERO_OPT: u32 = SEG_BASE + 0x02; // 零选项挂等
pub const WP_FILTER: u32 = SEG_BASE + 0x03; // 状态字簇过滤
pub const WP_DUP_WAIT: u32 = SEG_BASE + 0x04; // 重等待号
pub const WP_FROZEN: u32 = SEG_BASE + 0x05; // 印后等待

/// 选项位闭集。
pub const O_WNOHANG: u32 = 0x01; // 非阻塞
pub const O_WUNTRACED: u32 = 0x02; // 报告停止事件
pub const O_WCONTINUED: u32 = 0x04; // 报告继续事件
pub const O_ALL: u32 = O_WNOHANG | O_WUNTRACED | O_WCONTINUED;

/// 子进程事件簇（与 F9141 对齐）。
pub const EV_EXITED: usize = 0;
pub const EV_KILLED: usize = 1;
pub const EV_STOPPED: usize = 2;
pub const EV_CONTINUED: usize = 3;
pub const EV_COUNT: usize = 4;

pub const EV_NAMES: [&str; EV_COUNT] = ["exited", "killed", "stopped", "continued"];

/// 返回档闭集。
pub const R_REPORT: usize = 0; // 报告（返回 pid+状态字）
pub const R_ZERO: usize = 1; // WNOHANG 下无匹配 → 0
pub const R_BLOCK: usize = 2; // 无选项匹配可报事件 → 挂等
pub const R_COUNT: usize = 3;

pub const RET_NAMES: [&str; R_COUNT] = ["report", "zero", "block"];

/// 匹配核：事件簇 × 选项位 → 返回档。
/// 规则：exited/killed 恒可报；stopped 需 WUNTRACED；continued 需
/// WCONTINUED；不可报事件在 WNOHANG 下返回 zero，否则 block。
pub fn match_ret(ev: usize, opts: u32) -> Result<usize, u32> {
    if ev >= EV_COUNT {
        return Err(WP_FILTER);
    }
    if opts & !O_ALL != 0 {
        return Err(WP_UNK_OPT);
    }
    let reportable = match ev {
        EV_EXITED | EV_KILLED => true,
        EV_STOPPED => opts & O_WUNTRACED != 0,
        EV_CONTINUED => opts & O_WCONTINUED != 0,
        _ => false,
    };
    if reportable {
        Ok(R_REPORT)
    } else if opts & O_WNOHANG != 0 {
        Ok(R_ZERO)
    } else {
        Ok(R_BLOCK)
    }
}

/// 单条兼容账。
#[derive(Clone)]
pub struct WaitPidCall {
    pub wait_no: u32,
    pub ev: usize,
    pub opts: u32,
    pub ret: usize,
}

impl WaitPidCall {
    fn render_line(&self) -> String {
        format!(
            "WP{}|{}|opts={:#04x}|{}",
            self.wait_no, EV_NAMES[self.ev], self.opts, RET_NAMES[self.ret]
        )
    }
}

/// 兼容账本。
pub struct WaitPidLedger {
    calls: Vec<WaitPidCall>,
    sealed: bool,
}

impl WaitPidLedger {
    pub fn new() -> Self {
        WaitPidLedger {
            calls: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条等待：选项闸 + 匹配闸 + 重号拒 + 印后拒。
    pub fn wait(&mut self, no: u32, ev: usize, opts: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(WP_FROZEN);
        }
        if opts & !O_ALL != 0 {
            return Err(WP_UNK_OPT);
        }
        if opts == 0 {
            return Err(WP_ZERO_OPT);
        }
        let r = match_ret(ev, opts)?;
        if self.calls.iter().any(|x| x.wait_no == no) {
            return Err(WP_DUP_WAIT);
        }
        self.calls.push(WaitPidCall {
            wait_no: no,
            ev,
            opts,
            ret: r,
        });
        Ok(r)
    }

    pub fn call(&self, no: u32) -> Option<&WaitPidCall> {
        self.calls.iter().find(|x| x.wait_no == no)
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// 各返回档计数。
    pub fn ret_counts(&self) -> [usize; R_COUNT] {
        let mut c = [0usize; R_COUNT];
        for x in &self.calls {
            if x.ret < R_COUNT {
                c[x.ret] += 1;
            }
        }
        c
    }

    /// 兼容指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.calls {
            h = h.wrapping_mul(0x0100_0193) ^ x.wait_no;
            h = h.wrapping_mul(0x0100_0193) ^ x.ev as u32;
            h = h.wrapping_mul(0x0100_0193) ^ x.opts;
            h ^= x.ret as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.calls.is_empty() {
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
        for x in &self.calls {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("WAITPID-SEAL\n");
        }
        out
    }
}

impl Default for WaitPidLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：五组合（stopped 带/不带 WUNTRACED、continued 带/不带、
/// exited 恒报）。
fn standard_ledger() -> WaitPidLedger {
    let mut l = WaitPidLedger::new();
    let _ = l.wait(1, EV_EXITED, O_WNOHANG); // report
    let _ = l.wait(2, EV_STOPPED, O_WNOHANG | O_WUNTRACED); // report
    let _ = l.wait(3, EV_STOPPED, O_WNOHANG); // zero
    let _ = l.wait(4, EV_CONTINUED, O_WUNTRACED); // block
    let _ = l.wait(5, EV_CONTINUED, O_WNOHANG | O_WCONTINUED); // report
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知选项位
    let mut l = WaitPidLedger::new();
    if l.wait(1, EV_EXITED, 0x08) == Err(WP_UNK_OPT) {
        caught += 1;
    }
    // 注 2：零选项
    let mut l = WaitPidLedger::new();
    if l.wait(1, EV_EXITED, 0) == Err(WP_ZERO_OPT) {
        caught += 1;
    }
    // 注 3：簇过滤（未知事件簇）
    if match_ret(9, O_WNOHANG) == Err(WP_FILTER) {
        caught += 1;
    }
    // 注 4：重等待号
    let mut l = standard_ledger();
    if l.wait(1, EV_EXITED, O_WNOHANG) == Err(WP_DUP_WAIT) {
        caught += 1;
    }
    // 注 5：印后等待
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.wait(9, EV_EXITED, O_WNOHANG) == Err(WP_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 兼容账；J2 兼容语义；J3 兼容指纹。
pub fn run_unx9142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9142");
    // J1-1 五组合全落账
    let l = standard_ledger();
    let ok = l.len() == 5 && (1..=5).all(|n| l.call(n).is_some());
    s.add("9142-J1-五组合全落账", ok, if ok { "" } else { "兼容账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("WP1|exited|opts=0x01|report")
        && text.contains("WP3|stopped|opts=0x01|zero")
        && text.contains("WP4|continued|opts=0x02|block")
        && text.contains("WAITPID-SEAL");
    s.add("9142-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 匹配核恒等（五组合返回档逐一验证）
    let ok = match_ret(EV_EXITED, O_WNOHANG) == Ok(R_REPORT)
        && match_ret(EV_STOPPED, O_WNOHANG | O_WUNTRACED) == Ok(R_REPORT)
        && match_ret(EV_STOPPED, O_WNOHANG) == Ok(R_ZERO)
        && match_ret(EV_CONTINUED, O_WUNTRACED) == Ok(R_BLOCK)
        && match_ret(EV_CONTINUED, O_WNOHANG | O_WCONTINUED) == Ok(R_REPORT);
    s.add("9142-J2-匹配核恒等", ok, if ok { "" } else { "匹配核漂移" });
    // J2-2 选项位闭集
    let ok = match_ret(EV_EXITED, 0x08) == Err(WP_UNK_OPT) && match_ret(EV_EXITED, 0x10) == Err(WP_UNK_OPT);
    s.add("9142-J2-选项闭集", ok, if ok { "" } else { "选项闸漂移" });
    // J2-3 返回档计数（report3/zero1/block1）
    let rc = l.ret_counts();
    let ok = rc == [3, 1, 1];
    let detail = if ok { String::new() } else { format!("{},{},{}", rc[0], rc[1], rc[2]) };
    s.add_dyn("9142-J2-返回档计数", rc == [3, 1, 1], &detail);
    // J2-4 WNOHANG 非阻塞语义（不可报事件 → zero 而非 block）
    let ok = match_ret(EV_CONTINUED, O_WNOHANG) == Ok(R_ZERO);
    s.add("9142-J2-WNOHANG语义", ok, if ok { "" } else { "非阻塞漂移" });
    // J2-5 重号拒
    let mut l3 = standard_ledger();
    let ok = l3.wait(1, EV_KILLED, O_WNOHANG) == Err(WP_DUP_WAIT);
    s.add("9142-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J2-6 零选项拒
    let mut l4 = WaitPidLedger::new();
    let ok = l4.wait(1, EV_EXITED, 0) == Err(WP_ZERO_OPT);
    s.add("9142-J2-零选项拒", ok, if ok { "" } else { "零选项闸失效" });
    // J3-1 兼容指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9142-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 选项变签变
    let mut l5 = WaitPidLedger::new();
    let _ = l5.wait(1, EV_EXITED, O_WNOHANG);
    let fp0 = l5.ledger_fp();
    let _ = l5.wait(2, EV_EXITED, O_WNOHANG | O_WUNTRACED);
    let ok = l5.ledger_fp() != fp0;
    s.add("9142-J3-选项变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l6 = standard_ledger();
    let ok = l6.seal() && l6.sealed();
    s.add("9142-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒等待
    let ok = l6.wait(9, EV_EXITED, O_WNOHANG) == Err(WP_FROZEN);
    s.add("9142-J3-印后拒等待", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9142-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9142_all_checks_green() {
        let set = run_unx9142_checks();
        assert!(set.all_passed(), "UNX-F9142 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9142_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
