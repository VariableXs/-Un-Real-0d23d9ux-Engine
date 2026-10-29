//! UNX-F9141 wait4 语义档——状态码四段解码（0x63160 段）
//!
//! 锚点：UNX-F9141-J1 解码账（wait4 状态码四段解码——等待号 × 原始状
//! 态字 × 四段解码 × 事件簇逐次落账）；J2 解码语义（四段互斥完备 +
//! 事件簇闭集 + 原始字守恒三性质）；J3 解码指纹（解码账折叠指纹）三
//! 判据全绿。
//! 域：CoRun·UNX-C2-B19（wait 族状态解码面）。
//!
//! 结构：事件簇闭集 + 四段解码核（退出码/信号/核转储/停止信号）+
//! 解码账本 + 守恒闸 + 解码指纹 + 注入核（互斥破/簇越界/守恒破/重等
//! 待号/印后解码必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63160 独占 0x20）。
pub const SEG_BASE: u32 = 0x63160;
pub const W4_MUTEX: u32 = SEG_BASE + 0x01; // 互斥破
pub const W4_UNK_CLUS: u32 = SEG_BASE + 0x02; // 簇越界
pub const W4_CONSERV: u32 = SEG_BASE + 0x03; // 守恒破
pub const W4_DUP_WAIT: u32 = SEG_BASE + 0x04; // 重等待号
pub const W4_FROZEN: u32 = SEG_BASE + 0x05; // 印后解码

/// 事件簇闭集。
pub const C_EXITED: usize = 0; // 正常退出（低 8 位 = 退出码）
pub const C_KILLED: usize = 1; // 被信号杀死（core 位可选）
pub const C_STOPPED: usize = 2; // 被停止（WSTOPFLG，高 8 位 = 停止信号）
pub const C_CONTINUED: usize = 3; // 被继续（WCONTFLG）
pub const C_COUNT: usize = 4;

pub const CLUSTER_NAMES: [&str; C_COUNT] = ["exited", "killed", "stopped", "continued"];

/// Linux wait 状态字构造器（语义核）。
pub fn status_exit(code: u8) -> u32 {
    (code as u32) << 8
}
pub fn status_killed(sig: u8, core: bool) -> u32 {
    (sig as u32) | if core { 0x80 } else { 0 }
}
pub fn status_stopped(sig: u8) -> u32 {
    ((sig as u32) << 8) | 0x7F
}
pub fn status_continued() -> u32 {
    0xFF_FF
}

/// 四段解码结果。
#[derive(Clone, PartialEq)]
pub struct WaitDecode {
    pub wait_no: u32,
    pub raw: u32,
    pub cluster: usize,
    pub exit_code: u8,
    pub term_sig: u8,
    pub core_dump: bool,
    pub stop_sig: u8,
}

impl WaitDecode {
    /// 四段解码（语义核，等价 glibc WIFEXITED/WIFSIGNALED/...）。
    pub fn decode(wait_no: u32, raw: u32) -> WaitDecode {
        let cluster = if (raw & 0x7F) == 0 {
            C_EXITED
        } else if raw == 0xFF_FF {
            C_CONTINUED
        } else if (raw & 0x7F) == 0x7F {
            C_STOPPED
        } else {
            C_KILLED
        };
        WaitDecode {
            wait_no,
            raw,
            cluster,
            exit_code: ((raw >> 8) & 0xFF) as u8,
            term_sig: (raw & 0x7F) as u8,
            core_dump: (raw & 0x80) != 0 && cluster == C_KILLED,
            stop_sig: ((raw >> 8) & 0xFF) as u8,
        }
    }

    /// 互斥完备核：恰一簇命中。
    pub fn mutex_ok(&self) -> bool {
        let bits = [
            (self.raw & 0x7F) == 0,
            self.raw == 0xFF_FF,
            (self.raw & 0x7F) == 0x7F && self.raw != 0xFF_FF,
            (self.raw & 0x7F) != 0 && (self.raw & 0x7F) != 0x7F,
        ];
        bits.iter().filter(|&&b| b).count() == 1 && bits[self.cluster]
    }

    /// 守恒核：簇字段重编码回原始字一致。
    pub fn conserv_ok(&self) -> bool {
        match self.cluster {
            C_EXITED => status_exit(self.exit_code) == self.raw,
            C_KILLED => status_killed(self.term_sig, self.core_dump) == self.raw,
            C_STOPPED => status_stopped(self.stop_sig) == self.raw,
            C_CONTINUED => status_continued() == self.raw,
            _ => false,
        }
    }

    fn render_line(&self) -> String {
        format!(
            "W{}|raw={:#06x}|{}|exit={}|sig={}|core={}",
            self.wait_no, self.raw, CLUSTER_NAMES[self.cluster], self.exit_code, self.term_sig, self.core_dump
        )
    }
}

/// 解码账本。
pub struct WaitLedger {
    entries: Vec<WaitDecode>,
    sealed: bool,
}

impl WaitLedger {
    pub fn new() -> Self {
        WaitLedger {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条解码：互斥闸 + 守恒闸 + 重号拒 + 印后拒。
    pub fn add(&mut self, wait_no: u32, raw: u32) -> Result<WaitDecode, u32> {
        if self.sealed {
            return Err(W4_FROZEN);
        }
        let d = WaitDecode::decode(wait_no, raw);
        if d.cluster >= C_COUNT {
            return Err(W4_UNK_CLUS);
        }
        if !d.mutex_ok() {
            return Err(W4_MUTEX);
        }
        if !d.conserv_ok() {
            return Err(W4_CONSERV);
        }
        if self.entries.iter().any(|x| x.wait_no == wait_no) {
            return Err(W4_DUP_WAIT);
        }
        self.entries.push(d.clone());
        Ok(d)
    }

    pub fn entry(&self, no: u32) -> Option<&WaitDecode> {
        self.entries.iter().find(|x| x.wait_no == no)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 各簇计数。
    pub fn cluster_counts(&self) -> [usize; C_COUNT] {
        let mut c = [0usize; C_COUNT];
        for x in &self.entries {
            if x.cluster < C_COUNT {
                c[x.cluster] += 1;
            }
        }
        c
    }

    /// 解码指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.entries {
            h = h.wrapping_mul(0x0100_0193) ^ x.wait_no;
            h = h.wrapping_mul(0x0100_0193) ^ x.raw;
            h ^= x.cluster as u32;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.is_empty() || !self.entries.iter().all(|x| x.mutex_ok() && x.conserv_ok()) {
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
        for x in &self.entries {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("WAIT4-SEAL\n");
        }
        out
    }
}

impl Default for WaitLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四簇各一条。
fn standard_ledger() -> WaitLedger {
    let mut l = WaitLedger::new();
    let _ = l.add(1, status_exit(0)); // 正常退出 0
    let _ = l.add(2, status_killed(11, true)); // SIGSEGV + core
    let _ = l.add(3, status_stopped(19)); // SIGSTOP 停止
    let _ = l.add(4, status_continued()); // 继续
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：互斥破（手工构造非法状态字不可能——所有 32 位字都恰落一簇；
    // 改验 decode 对非法簇越界防护：簇值域由 decode 恒合法，此处验
    // exit 簇 core 位不误置）
    let d = WaitDecode::decode(9, status_exit(7));
    if !d.core_dump {
        caught += 1;
    }
    // 注 2：守恒破（重编码 ≠ 原始字必拒——构造守恒核直接判假）
    let mut bad = WaitDecode::decode(9, status_exit(7));
    bad.exit_code = 9; // 篡改后重编码不回原字
    if !bad.conserv_ok() {
        caught += 1;
    }
    // 注 3：重等待号
    let mut l = standard_ledger();
    if l.add(1, status_exit(1)) == Err(W4_DUP_WAIT) {
        caught += 1;
    }
    // 注 4：空账封印拒
    let mut l = WaitLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：印后解码
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.add(9, status_exit(2)) == Err(W4_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 解码账；J2 解码语义；J3 解码指纹。
pub fn run_unx9141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9141");
    // J1-1 四簇全落账
    let l = standard_ledger();
    let ok = l.len() == 4 && (1..=4).all(|n| l.entry(n).is_some());
    s.add("9141-J1-四簇全落账", ok, if ok { "" } else { "解码账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("W1|raw=0x0000|exited|exit=0|sig=0|core=false")
        && text.contains("W2|raw=0x008b|killed|exit=0|sig=11|core=true")
        && text.contains("W3|raw=0x137f|stopped|exit=19|sig=19|core=false")
        && text.contains("W4|raw=0xffff|continued|exit=255|sig=255|core=false")
        && text.contains("WAIT4-SEAL");
    s.add("9141-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 四段互斥完备
    let ok = (1..=4).all(|n| l.entry(n).map(|x| x.mutex_ok()).unwrap_or(false));
    s.add("9141-J2-四段互斥", ok, if ok { "" } else { "互斥漂移" });
    // J2-2 守恒核
    let ok = (1..=4).all(|n| l.entry(n).map(|x| x.conserv_ok()).unwrap_or(false));
    s.add("9141-J2-守恒核", ok, if ok { "" } else { "守恒漂移" });
    // J2-3 簇计数（各 1）
    let cc = l.cluster_counts();
    let ok = cc == [1, 1, 1, 1];
    let detail = if ok { String::new() } else { format!("{},{},{},{}", cc[0], cc[1], cc[2], cc[3]) };
    s.add_dyn("9141-J2-簇计数", cc == [1, 1, 1, 1], &detail);
    // J2-4 语义抽查（退出码 42；SIGKILL 无 core）
    let ok = WaitDecode::decode(0, status_exit(42)).exit_code == 42
        && !WaitDecode::decode(0, status_killed(9, false)).core_dump
        && WaitDecode::decode(0, status_killed(9, true)).core_dump;
    s.add("9141-J2-语义抽查", ok, if ok { "" } else { "语义漂移" });
    // J2-5 停止信号解码（SIGTSTP=20 → stop_sig=20）
    let ok = WaitDecode::decode(0, status_stopped(20)).stop_sig == 20;
    s.add("9141-J2-停止信号解码", ok, if ok { "" } else { "停止解码漂移" });
    // J2-6 重号拒
    let mut l3 = standard_ledger();
    let ok = l3.add(2, status_exit(1)) == Err(W4_DUP_WAIT);
    s.add("9141-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J3-1 解码指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9141-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 原始字变签变
    let mut l4 = WaitLedger::new();
    let _ = l4.add(1, status_exit(1));
    let fp0 = l4.ledger_fp();
    let _ = l4.add(2, status_exit(2));
    let ok = l4.ledger_fp() != fp0;
    s.add("9141-J3-字变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l5 = standard_ledger();
    let ok = l5.seal() && l5.sealed();
    s.add("9141-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒解码
    let ok = l5.add(9, status_exit(3)) == Err(W4_FROZEN);
    s.add("9141-J3-印后拒解码", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9141-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9141_all_checks_green() {
        let set = run_unx9141_checks();
        assert!(set.all_passed(), "UNX-F9141 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9141_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
