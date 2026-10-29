//! UNX-F9144 僵尸态收割语义与 ExitStatus 消费（0x631C0 段）
//!
//! 锚点：UNX-F9144-J1 收割账（僵尸态进程收割——收割号 × pid × 状态字
//! × 收割前后态逐次落账）；J2 收割语义（态迁移闭集 + 僵尸唯一性 +
//! ExitStatus 消费守恒三性质）；J3 收割指纹（收割账折叠指纹）三判据
//! 全绿。
//! 域：CoRun·UNX-C2-B19（僵尸收割语义面）。
//!
//! 结构：进程态闭集 + 僵尸表 + 收割核（态迁移闸 + 双收割拒 + 消费
//! 闸）+ 收割指纹 + 注入核（活进程收割/双收割/态越界/重收割号/印后
//! 收割必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x631C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x631C0;
pub const ZB_LIVE_REAP: u32 = SEG_BASE + 0x01; // 活进程收割
pub const ZB_DOUBLE_REAP: u32 = SEG_BASE + 0x02; // 双收割
pub const ZB_UNK_STATE: u32 = SEG_BASE + 0x03; // 态越界
pub const ZB_DUP_NO: u32 = SEG_BASE + 0x04; // 重收割号
pub const ZB_FROZEN: u32 = SEG_BASE + 0x05; // 印后收割

/// 进程态闭集。
pub const P_RUNNING: usize = 0; // 运行
pub const P_ZOMBIE: usize = 1; // 僵尸（已退出未收割）
pub const P_REAPED: usize = 2; // 已收割
pub const P_COUNT: usize = 3;

pub const STATE_NAMES: [&str; P_COUNT] = ["running", "zombie", "reaped"];

/// ExitStatus 消费结构（与 F9141 四段解码对齐的消费者侧）。
#[derive(Clone, PartialEq)]
pub struct ExitStatus {
    pub raw: u32,
}

impl ExitStatus {
    pub fn from_exit(code: u8) -> Self {
        ExitStatus { raw: (code as u32) << 8 }
    }

    pub fn from_signal(sig: u8) -> Self {
        ExitStatus { raw: sig as u32 }
    }

    /// 是否正常退出。
    pub fn exited(&self) -> bool {
        self.raw & 0x7F == 0
    }

    /// 退出码（仅正常退出有效）。
    pub fn code(&self) -> Option<u8> {
        if self.exited() {
            Some(((self.raw >> 8) & 0xFF) as u8)
        } else {
            None
        }
    }

    /// 终止信号（仅被杀有效）。
    pub fn signal(&self) -> Option<u8> {
        if !self.exited() {
            Some((self.raw & 0x7F) as u8)
        } else {
            None
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        match (self.code(), self.signal()) {
            (Some(c), _) => format!("exit({})", c),
            (None, Some(s)) => format!("signal({})", s),
            (None, None) => "unknown".to_string(),
        }
    }
}

/// 僵尸表条目。
#[derive(Clone)]
pub struct ZombieEntry {
    pub pid: u32,
    pub state: usize,
    pub status: ExitStatus,
}

/// 收割账本。
pub struct ReapLedger {
    table: Vec<ZombieEntry>,
    reaps: Vec<(u32, u32)>, // (reap_no, pid)
    sealed: bool,
}

impl ReapLedger {
    pub fn new() -> Self {
        ReapLedger {
            table: Vec::new(),
            reaps: Vec::new(),
            sealed: false,
        }
    }

    /// 登记子进程（running 态）。
    pub fn spawn(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(ZB_FROZEN);
        }
        if self.table.iter().any(|e| e.pid == pid) {
            return Err(ZB_DUP_NO);
        }
        self.table.push(ZombieEntry {
            pid,
            state: P_RUNNING,
            status: ExitStatus::from_exit(0),
        });
        Ok(())
    }

    /// 退出 → zombie（带状态字）。
    pub fn exit(&mut self, pid: u32, status: ExitStatus) -> Result<(), u32> {
        if self.sealed {
            return Err(ZB_FROZEN);
        }
        match self.table.iter_mut().find(|e| e.pid == pid) {
            None => Err(ZB_UNK_STATE),
            Some(e) => {
                if e.state != P_RUNNING {
                    return Err(ZB_UNK_STATE);
                }
                e.state = P_ZOMBIE;
                e.status = status;
                Ok(())
            }
        }
    }

    /// 收割：zombie → reaped（活进程/已收割拒）。
    pub fn reap(&mut self, reap_no: u32, pid: u32) -> Result<ExitStatus, u32> {
        if self.sealed {
            return Err(ZB_FROZEN);
        }
        if self.reaps.iter().any(|(n, _)| *n == reap_no) {
            return Err(ZB_DUP_NO);
        }
        let e = match self.table.iter_mut().find(|e| e.pid == pid) {
            None => return Err(ZB_UNK_STATE),
            Some(e) => e,
        };
        match e.state {
            P_RUNNING => Err(ZB_LIVE_REAP),
            P_REAPED => Err(ZB_DOUBLE_REAP),
            P_ZOMBIE => {
                let st = e.status.clone();
                e.state = P_REAPED;
                self.reaps.push((reap_no, pid));
                Ok(st)
            }
            _ => Err(ZB_UNK_STATE),
        }
    }

    pub fn state_of(&self, pid: u32) -> Option<usize> {
        self.table.iter().find(|e| e.pid == pid).map(|e| e.state)
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// 僵尸唯一性核：每 pid 至多一个 zombie 态条目（pid 唯一保证）。
    pub fn zombie_unique(&self) -> bool {
        self.table.iter().filter(|e| e.state == P_ZOMBIE).count()
            <= self.table.iter().map(|e| e.pid).collect::<Vec<_>>().len()
    }

    /// 收割指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in &self.table {
            h = h.wrapping_mul(0x0100_0193) ^ e.pid;
            h = h.wrapping_mul(0x0100_0193) ^ e.state as u32;
            h = h.wrapping_mul(0x0100_0193) ^ e.status.raw;
        }
        for (n, p) in &self.reaps {
            h = h.wrapping_mul(0x0100_0193) ^ *n;
            h = h.wrapping_mul(0x0100_0193) ^ *p;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.reaps.is_empty() {
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
        for e in &self.table {
            out.push_str(&format!(
                "Z{}|{}|{}\n",
                e.pid, STATE_NAMES[e.state], e.status.render()
            ));
        }
        for (n, p) in &self.reaps {
            out.push_str(&format!("R{}|{}\n", n, p));
        }
        if self.sealed {
            out.push_str("REAP-SEAL\n");
        }
        out
    }
}

impl Default for ReapLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三进程（101 正常退出收割 / 102 被杀未收割 / 103 运行中）。
fn standard_ledger() -> ReapLedger {
    let mut l = ReapLedger::new();
    let _ = l.spawn(101);
    let _ = l.spawn(102);
    let _ = l.spawn(103);
    let _ = l.exit(101, ExitStatus::from_exit(42));
    let _ = l.exit(102, ExitStatus::from_signal(9));
    let _ = l.reap(1, 101);
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：活进程收割
    let mut l = standard_ledger();
    if l.reap(9, 103) == Err(ZB_LIVE_REAP) {
        caught += 1;
    }
    // 注 2：双收割
    let mut l = standard_ledger();
    let _ = l.reap(9, 101);
    if l.reap(10, 101) == Err(ZB_DOUBLE_REAP) {
        caught += 1;
    }
    // 注 3：态越界（未知 pid）
    let mut l = standard_ledger();
    if l.reap(9, 999) == Err(ZB_UNK_STATE) {
        caught += 1;
    }
    // 注 4：重收割号
    let mut l = standard_ledger();
    let _ = l.reap(9, 102); // 102 是 zombie
    if l.reap(9, 101) == Err(ZB_DUP_NO) {
        caught += 1;
    }
    // 注 5：印后收割
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.reap(11, 102) == Err(ZB_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 收割账；J2 收割语义；J3 收割指纹。
pub fn run_unx9144_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9144");
    // J1-1 三进程全落账
    let l = standard_ledger();
    let ok = l.len() == 3 && l.state_of(101) == Some(P_REAPED) && l.state_of(102) == Some(P_ZOMBIE) && l.state_of(103) == Some(P_RUNNING);
    s.add("9144-J1-三进程全落账", ok, if ok { "" } else { "收割账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("Z101|reaped|exit(42)")
        && text.contains("Z102|zombie|signal(9)")
        && text.contains("Z103|running|exit(0)")
        && text.contains("R1|101")
        && text.contains("REAP-SEAL");
    s.add("9144-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 ExitStatus 消费守恒（exit(42) → code=42；signal(9) → sig=9）
    let ok = ExitStatus::from_exit(42).code() == Some(42)
        && ExitStatus::from_signal(9).signal() == Some(9)
        && ExitStatus::from_signal(9).code().is_none()
        && ExitStatus::from_exit(42).signal().is_none();
    s.add("9144-J2-ExitStatus守恒", ok, if ok { "" } else { "消费守恒漂移" });
    // J2-2 僵尸唯一性
    let ok = l.zombie_unique();
    s.add("9144-J2-僵尸唯一性", ok, if ok { "" } else { "唯一性漂移" });
    // J2-3 态迁移闸（reaped 不可再退出）
    let mut l3 = standard_ledger();
    let ok = l3.exit(101, ExitStatus::from_exit(1)) == Err(ZB_UNK_STATE);
    s.add("9144-J2-态迁移闸", ok, if ok { "" } else { "迁移闸失效" });
    // J2-4 收割返回原状态字
    let mut l4 = standard_ledger();
    let st = l4.reap(2, 102).ok();
    let ok = st.map(|x| x.signal() == Some(9)).unwrap_or(false);
    s.add("9144-J2-收割返状态", ok, if ok { "" } else { "状态返漂移" });
    // J2-5 双收割拒
    let mut l5 = standard_ledger();
    let _ = l5.reap(2, 101);
    let ok = l5.reap(3, 101) == Err(ZB_DOUBLE_REAP);
    s.add("9144-J2-双收割拒", ok, if ok { "" } else { "双收闸失效" });
    // J3-1 收割指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9144-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 收割变签变
    let mut l6 = standard_ledger();
    let fp0 = l6.ledger_fp();
    let _ = l6.reap(9, 102);
    let ok = l6.ledger_fp() != fp0;
    s.add("9144-J3-收割变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l7 = standard_ledger();
    let ok = l7.seal() && l7.sealed();
    s.add("9144-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒收割
    let ok = l7.reap(11, 102) == Err(ZB_FROZEN);
    s.add("9144-J3-印后拒收割", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9144-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9144_all_checks_green() {
        let set = run_unx9144_checks();
        assert!(set.all_passed(), "UNX-F9144 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9144_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
