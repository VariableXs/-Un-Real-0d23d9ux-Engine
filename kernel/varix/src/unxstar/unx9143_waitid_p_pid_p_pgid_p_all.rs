//! UNX-F9143 waitid 语义档——P_PID/P_PGID/P_ALL（0x631A0 段）
//!
//! 锚点：UNX-F9143-J1 idtype 账（waitid 三 idtype 语义——调用号 ×
//! idtype × id 值 × 匹配结果逐次落账）；J2 idtype 语义（idtype 闭集
//! + 匹配规则推导 + WEXITED/WSTOPPED/WCONTINUED 事件掩码三性质）；
//! J3 idtype 指纹（idtype 账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（waitid 语义面）。
//!
//! 结构：idtype 闭集 + 事件掩码闭集 + 匹配核 + 账本 + 匹配指纹 + 注
//! 入核（未知 idtype/空掩码/掩码不匹配/P_ALL 带 id/印后调用必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x631A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x631A0;
pub const WI_UNK_IDTYPE: u32 = SEG_BASE + 0x01; // 未知 idtype
pub const WI_EMPTY_MASK: u32 = SEG_BASE + 0x02; // 空事件掩码
pub const WI_MASK_MISS: u32 = SEG_BASE + 0x03; // 掩码不匹配
pub const WI_ALL_WITH_ID: u32 = SEG_BASE + 0x04; // P_ALL 带 id
pub const WI_FROZEN: u32 = SEG_BASE + 0x05; // 印后调用

/// idtype 闭集。
pub const T_PID: usize = 0; // 精确 pid
pub const T_PGID: usize = 1; // 进程组
pub const T_ALL: usize = 2; // 任意子进程
pub const T_COUNT: usize = 3;

pub const IDTYPE_NAMES: [&str; T_COUNT] = ["P_PID", "P_PGID", "P_ALL"];

/// 事件位掩码闭集。
pub const M_EXITED: u32 = 0x001; // WEXITED
pub const M_STOPPED: u32 = 0x002; // WSTOPPED
pub const M_CONTINUED: u32 = 0x004; // WCONTINUED
pub const M_ALL: u32 = M_EXITED | M_STOPPED | M_CONTINUED;

/// 事件簇（子进程当前事件）。
pub const EV_EXITED: usize = 0;
pub const EV_STOPPED: usize = 1;
pub const EV_CONTINUED: usize = 2;
pub const EV_COUNT: usize = 3;

pub const EV_BITS: [u32; EV_COUNT] = [M_EXITED, M_STOPPED, M_CONTINUED];
pub const EV_NAMES: [&str; EV_COUNT] = ["exited", "stopped", "continued"];

/// 匹配核：idtype × id × 子进程(pid, pgid, 事件) × 掩码 → 是否命中。
pub fn idtype_match(
    idtype: usize,
    id: u32,
    mask: u32,
    child_pid: u32,
    child_pgid: u32,
    ev: usize,
) -> Result<bool, u32> {
    if idtype >= T_COUNT {
        return Err(WI_UNK_IDTYPE);
    }
    if mask & !M_ALL != 0 || mask == 0 {
        return Err(WI_EMPTY_MASK);
    }
    if ev >= EV_COUNT {
        return Err(WI_MASK_MISS);
    }
    let id_hit = match idtype {
        T_PID => id == child_pid,
        T_PGID => id == child_pgid,
        T_ALL => true,
        _ => false,
    };
    let mask_hit = mask & EV_BITS[ev] != 0;
    Ok(id_hit && mask_hit)
}

/// 单条 idtype 账。
#[derive(Clone)]
pub struct WaitIdCall {
    pub call_no: u32,
    pub idtype: usize,
    pub id: u32,
    pub mask: u32,
    pub matched: bool,
}

impl WaitIdCall {
    fn render_line(&self) -> String {
        format!(
            "WI{}|{}|id={}|mask={:#05x}|{}",
            self.call_no,
            IDTYPE_NAMES[self.idtype],
            self.id,
            self.mask,
            if self.matched { "HIT" } else { "MISS" }
        )
    }
}

/// idtype 账本。
pub struct WaitIdLedger {
    calls: Vec<WaitIdCall>,
    sealed: bool,
}

impl WaitIdLedger {
    pub fn new() -> Self {
        WaitIdLedger {
            calls: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条调用：idtype 闸 + 掩码闸 + P_ALL 带参拒 + 匹配推导。
    pub fn call(&mut self, no: u32, idtype: usize, id: u32, mask: u32, child: (u32, u32, usize)) -> Result<bool, u32> {
        if self.sealed {
            return Err(WI_FROZEN);
        }
        if idtype >= T_COUNT {
            return Err(WI_UNK_IDTYPE);
        }
        if mask & !M_ALL != 0 || mask == 0 {
            return Err(WI_EMPTY_MASK);
        }
        if idtype == T_ALL && id != 0 {
            return Err(WI_ALL_WITH_ID);
        }
        let matched = idtype_match(idtype, id, mask, child.0, child.1, child.2)?;
        self.calls.push(WaitIdCall {
            call_no: no,
            idtype,
            id,
            mask,
            matched,
        });
        Ok(matched)
    }

    pub fn entry(&self, no: u32) -> Option<&WaitIdCall> {
        self.calls.iter().find(|x| x.call_no == no)
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// 命中数。
    pub fn hits(&self) -> usize {
        self.calls.iter().filter(|x| x.matched).count()
    }

    /// idtype 指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.calls {
            h = h.wrapping_mul(0x0100_0193) ^ x.call_no;
            h = h.wrapping_mul(0x0100_0193) ^ x.idtype as u32;
            h = h.wrapping_mul(0x0100_0193) ^ x.id;
            h = h.wrapping_mul(0x0100_0193) ^ x.mask;
            h ^= x.matched as u32;
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
            out.push_str("WAITID-SEAL\n");
        }
        out
    }
}

impl Default for WaitIdLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四调用（P_PID 命中/未命中、P_PGID 命中、P_ALL 命中）。
/// 子进程：pid=42 pgid=7 事件=exited。
fn standard_ledger() -> WaitIdLedger {
    let mut l = WaitIdLedger::new();
    let _ = l.call(1, T_PID, 42, M_EXITED, (42, 7, EV_EXITED));
    let _ = l.call(2, T_PID, 43, M_EXITED, (42, 7, EV_EXITED));
    let _ = l.call(3, T_PGID, 7, M_EXITED | M_STOPPED, (42, 7, EV_EXITED));
    let _ = l.call(4, T_ALL, 0, M_ALL, (42, 7, EV_EXITED));
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知 idtype
    let mut l = WaitIdLedger::new();
    if l.call(1, 9, 1, M_EXITED, (1, 1, EV_EXITED)) == Err(WI_UNK_IDTYPE) {
        caught += 1;
    }
    // 注 2：空掩码
    let mut l = WaitIdLedger::new();
    if l.call(1, T_PID, 1, 0, (1, 1, EV_EXITED)) == Err(WI_EMPTY_MASK) {
        caught += 1;
    }
    // 注 3：掩码越界位
    let mut l = WaitIdLedger::new();
    if l.call(1, T_PID, 1, 0x08, (1, 1, EV_EXITED)) == Err(WI_EMPTY_MASK) {
        caught += 1;
    }
    // 注 4：P_ALL 带 id
    let mut l = WaitIdLedger::new();
    if l.call(1, T_ALL, 5, M_EXITED, (1, 1, EV_EXITED)) == Err(WI_ALL_WITH_ID) {
        caught += 1;
    }
    // 注 5：印后调用
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.call(9, T_PID, 42, M_EXITED, (42, 7, EV_EXITED)) == Err(WI_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 idtype 账；J2 idtype 语义；J3 idtype 指纹。
pub fn run_unx9143_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9143");
    // J1-1 四调用全落账
    let l = standard_ledger();
    let ok = l.len() == 4 && (1..=4).all(|n| l.entry(n).is_some());
    s.add("9143-J1-四调用全落账", ok, if ok { "" } else { "idtype 账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("WI1|P_PID|id=42|mask=0x0001|HIT")
        && text.contains("WI2|P_PID|id=43|mask=0x0001|MISS")
        && text.contains("WI4|P_ALL|id=0|mask=0x0007|HIT")
        && text.contains("WAITID-SEAL");
    s.add("9143-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 三 idtype 匹配规则
    let ok = idtype_match(T_PID, 42, M_EXITED, 42, 7, EV_EXITED) == Ok(true)
        && idtype_match(T_PGID, 7, M_EXITED, 42, 7, EV_EXITED) == Ok(true)
        && idtype_match(T_ALL, 0, M_EXITED, 42, 7, EV_EXITED) == Ok(true)
        && idtype_match(T_PID, 43, M_EXITED, 42, 7, EV_EXITED) == Ok(false);
    s.add("9143-J2-三idtype规则", ok, if ok { "" } else { "匹配规则漂移" });
    // J2-2 事件掩码过滤（子进程 stopped 但掩码只收 exited → MISS）
    let ok = idtype_match(T_ALL, 0, M_EXITED, 42, 7, EV_STOPPED) == Ok(false)
        && idtype_match(T_ALL, 0, M_EXITED | M_STOPPED, 42, 7, EV_STOPPED) == Ok(true);
    s.add("9143-J2-掩码过滤", ok, if ok { "" } else { "掩码过滤漂移" });
    // J2-3 命中数（3/4）
    let ok = l.hits() == 3;
    s.add("9143-J2-命中数", ok, if ok { "" } else { "命中数漂移" });
    // J2-4 idtype 闭集
    let ok = idtype_match(9, 1, M_EXITED, 1, 1, EV_EXITED) == Err(WI_UNK_IDTYPE);
    s.add("9143-J2-idtype闭集", ok, if ok { "" } else { "闭集闸失效" });
    // J2-5 P_ALL 带参拒
    let mut l3 = WaitIdLedger::new();
    let ok = l3.call(1, T_ALL, 9, M_EXITED, (1, 1, EV_EXITED)) == Err(WI_ALL_WITH_ID);
    s.add("9143-J2-P_ALL带参拒", ok, if ok { "" } else { "带参闸失效" });
    // J2-6 空掩码拒
    let mut l4 = WaitIdLedger::new();
    let ok = l4.call(1, T_PID, 1, 0, (1, 1, EV_EXITED)) == Err(WI_EMPTY_MASK);
    s.add("9143-J2-空掩码拒", ok, if ok { "" } else { "掩码闸失效" });
    // J3-1 idtype 指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9143-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 掩码变签变
    let mut l5 = WaitIdLedger::new();
    let _ = l5.call(1, T_PID, 42, M_EXITED, (42, 7, EV_EXITED));
    let fp0 = l5.ledger_fp();
    let _ = l5.call(2, T_PID, 42, M_EXITED | M_CONTINUED, (42, 7, EV_EXITED));
    let ok = l5.ledger_fp() != fp0;
    s.add("9143-J3-掩码变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l6 = standard_ledger();
    let ok = l6.seal() && l6.sealed();
    s.add("9143-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒调用
    let ok = l6.call(9, T_PID, 42, M_EXITED, (42, 7, EV_EXITED)) == Err(WI_FROZEN);
    s.add("9143-J3-印后拒调用", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9143-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9143_all_checks_green() {
        let set = run_unx9143_checks();
        assert!(set.all_passed(), "UNX-F9143 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9143_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
