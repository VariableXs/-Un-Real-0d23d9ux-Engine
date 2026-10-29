//! UNX-F9145 wait 错误矩阵（ECHILD/EINTR/EINVAL）（0x631E0 段）
//!
//! 锚点：UNX-F9145-J1 错误账（wait 族错误矩阵——错误号 × 触发场景 ×
//! 错误码 × 留痕态逐条落账）；J2 错误语义（错误码闭集 + 场景到码映
//! 射唯一 + 留痕必填三性质）；J3 错误指纹（错误账折叠指纹）三判据全
//! 绿。
//! 域：CoRun·UNX-C2-B19（wait 族错误矩阵面）。
//!
//! 结构：错误码闭集 + 触发场景闭集 + 错误账本 + 映射核（场景→码唯一
//! 闸 + 留痕闸）+ 错误指纹 + 注入核（场景错码映射错/空留痕/重错误号
//! /未知场景/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x631E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x631E0;
pub const WE_UNK_SCENE: u32 = SEG_BASE + 0x01; // 未知场景
pub const WE_CODE_MISS: u32 = SEG_BASE + 0x02; // 场景错码映射错
pub const WE_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const WE_DUP_NO: u32 = SEG_BASE + 0x04; // 重错误号
pub const WE_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 错误码闭集。
pub const E_ECHILD: u32 = 10; // 无可等子进程
pub const E_EINTR: u32 = 4; // 被信号打断
pub const E_EINVAL: u32 = 22; // 非法选项/参数
pub const E_COUNT: usize = 3;

pub const CODE_NAMES: [&str; E_COUNT] = ["ECHILD", "EINTR", "EINVAL"];

/// 触发场景闭集。
pub const SC_NO_CHILD: usize = 0; // 无子进程可等 → ECHILD
pub const SC_SIG_INT: usize = 1; // 等待中被信号打断 → EINTR
pub const SC_BAD_OPTS: usize = 2; // 非法选项组合 → EINVAL
pub const SC_BAD_PID: usize = 3; // 非法 pid/进程组 → EINVAL
pub const SC_COUNT: usize = 4;

pub const SCENE_NAMES: [&str; SC_COUNT] = ["no-child", "sig-int", "bad-opts", "bad-pid"];

/// 场景 → 错误码唯一映射。
pub fn scene_to_code(scene: usize) -> Result<u32, u32> {
    match scene {
        SC_NO_CHILD => Ok(E_ECHILD),
        SC_SIG_INT => Ok(E_EINTR),
        SC_BAD_OPTS | SC_BAD_PID => Ok(E_EINVAL),
        _ => Err(WE_UNK_SCENE),
    }
}

/// 单条错误账。
#[derive(Clone)]
pub struct WaitErr {
    pub err_no: u32,
    pub scene: usize,
    pub code: u32,
    pub note: String,
}

impl WaitErr {
    /// 合法：场景闭集 + 映射一致 + 留痕非空。
    pub fn ok(&self) -> bool {
        scene_to_code(self.scene) == Ok(self.code) && !self.note.is_empty()
    }

    fn render_line(&self) -> String {
        format!(
            "WE{}|{}|{}|{}",
            self.err_no,
            SCENE_NAMES[self.scene],
            CODE_NAMES[(self.code % 10 == 0 && self.code == E_ECHILD) as usize
                | ((self.code == E_EINTR) as usize) * 1
                | ((self.code == E_EINVAL) as usize) * 2],
            self.note
        )
    }
}

/// 错误账本。
pub struct WaitErrLedger {
    entries: Vec<WaitErr>,
    sealed: bool,
}

impl WaitErrLedger {
    pub fn new() -> Self {
        WaitErrLedger {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条：场景闸 + 映射闸 + 留痕闸 + 重号拒 + 印后拒。
    pub fn add(&mut self, e: WaitErr) -> Result<(), u32> {
        if self.sealed {
            return Err(WE_FROZEN);
        }
        if e.scene >= SC_COUNT {
            return Err(WE_UNK_SCENE);
        }
        if scene_to_code(e.scene) != Ok(e.code) {
            return Err(WE_CODE_MISS);
        }
        if e.note.is_empty() {
            return Err(WE_EMPTY_NOTE);
        }
        if self.entries.iter().any(|x| x.err_no == e.err_no) {
            return Err(WE_DUP_NO);
        }
        self.entries.push(e);
        Ok(())
    }

    pub fn entry(&self, no: u32) -> Option<&WaitErr> {
        self.entries.iter().find(|x| x.err_no == no)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 各码计数。
    pub fn code_counts(&self) -> [usize; E_COUNT] {
        let mut c = [0usize; E_COUNT];
        for x in &self.entries {
            match x.code {
                E_ECHILD => c[0] += 1,
                E_EINTR => c[1] += 1,
                E_EINVAL => c[2] += 1,
                _ => {}
            }
        }
        c
    }

    /// 错误指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.entries {
            h = h.wrapping_mul(0x0100_0193) ^ x.err_no;
            h = h.wrapping_mul(0x0100_0193) ^ x.scene as u32;
            h = h.wrapping_mul(0x0100_0193) ^ x.code;
            for b in x.note.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.is_empty() || !self.entries.iter().all(|x| x.ok()) {
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
            out.push_str("WAITERR-SEAL\n");
        }
        out
    }
}

impl Default for WaitErrLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四场景各一条。
fn standard_ledger() -> WaitErrLedger {
    let mut l = WaitErrLedger::new();
    let _ = l.add(WaitErr {
        err_no: 1,
        scene: SC_NO_CHILD,
        code: E_ECHILD,
        note: "no-child-reapable".to_string(),
    });
    let _ = l.add(WaitErr {
        err_no: 2,
        scene: SC_SIG_INT,
        code: E_EINTR,
        note: "sigchld-interrupt".to_string(),
    });
    let _ = l.add(WaitErr {
        err_no: 3,
        scene: SC_BAD_OPTS,
        code: E_EINVAL,
        note: "opts-conflict".to_string(),
    });
    let _ = l.add(WaitErr {
        err_no: 4,
        scene: SC_BAD_PID,
        code: E_EINVAL,
        note: "pid-not-child".to_string(),
    });
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    let mk = |no: u32, scene: usize, code: u32, note: &str| WaitErr {
        err_no: no,
        scene,
        code,
        note: note.to_string(),
    };
    // 注 1：未知场景
    let mut l = WaitErrLedger::new();
    if l.add(mk(1, 9, E_ECHILD, "x")) == Err(WE_UNK_SCENE) {
        caught += 1;
    }
    // 注 2：映射错（no-child 配 EINTR）
    let mut l = WaitErrLedger::new();
    if l.add(mk(1, SC_NO_CHILD, E_EINTR, "x")) == Err(WE_CODE_MISS) {
        caught += 1;
    }
    // 注 3：空留痕
    let mut l = WaitErrLedger::new();
    if l.add(mk(1, SC_NO_CHILD, E_ECHILD, "")) == Err(WE_EMPTY_NOTE) {
        caught += 1;
    }
    // 注 4：重错误号
    let mut l = standard_ledger();
    if l.add(mk(1, SC_SIG_INT, E_EINTR, "dup")) == Err(WE_DUP_NO) {
        caught += 1;
    }
    // 注 5：印后登记
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.add(mk(9, SC_NO_CHILD, E_ECHILD, "late")) == Err(WE_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 错误账；J2 错误语义；J3 错误指纹。
pub fn run_unx9145_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9145");
    // J1-1 四场景全落账
    let l = standard_ledger();
    let ok = l.len() == 4 && (1..=4).all(|n| l.entry(n).is_some());
    s.add("9145-J1-四场景全落账", ok, if ok { "" } else { "错误账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("WE1|no-child|ECHILD|no-child-reapable")
        && text.contains("WE3|bad-opts|EINVAL|opts-conflict")
        && text.contains("WAITERR-SEAL");
    s.add("9145-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 场景到码唯一映射
    let ok = scene_to_code(SC_NO_CHILD) == Ok(E_ECHILD)
        && scene_to_code(SC_SIG_INT) == Ok(E_EINTR)
        && scene_to_code(SC_BAD_OPTS) == Ok(E_EINVAL)
        && scene_to_code(SC_BAD_PID) == Ok(E_EINVAL);
    s.add("9145-J2-映射唯一", ok, if ok { "" } else { "映射漂移" });
    // J2-2 合法性（全部）
    let ok = (1..=4).all(|n| l.entry(n).map(|x| x.ok()).unwrap_or(false));
    s.add("9145-J2-合法性", ok, if ok { "" } else { "合法性漂移" });
    // J2-3 码计数（ECHILD1/EINTR1/EINVAL2）
    let cc = l.code_counts();
    let ok = cc == [1, 1, 2];
    let detail = if ok { String::new() } else { format!("{},{},{}", cc[0], cc[1], cc[2]) };
    s.add_dyn("9145-J2-码计数", cc == [1, 1, 2], &detail);
    // J2-4 未知场景拒
    let ok = scene_to_code(9) == Err(WE_UNK_SCENE);
    s.add("9145-J2-未知场景拒", ok, if ok { "" } else { "场景闸失效" });
    // J2-5 重号拒
    let mut l3 = standard_ledger();
    let ok = l3.add(WaitErr {
        err_no: 2,
        scene: SC_SIG_INT,
        code: E_EINTR,
        note: "dup".to_string(),
    }) == Err(WE_DUP_NO);
    s.add("9145-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J3-1 错误指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9145-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 留痕变签变
    let mut l4 = standard_ledger();
    let fp0 = l4.ledger_fp();
    if let Some(x) = l4.entries.first_mut() {
        x.note.push('x');
    }
    let ok = l4.ledger_fp() != fp0;
    s.add("9145-J3-留痕变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l5 = standard_ledger();
    let ok = l5.seal() && l5.sealed();
    s.add("9145-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = l5.add(WaitErr {
        err_no: 9,
        scene: SC_NO_CHILD,
        code: E_ECHILD,
        note: "late".to_string(),
    }) == Err(WE_FROZEN);
    s.add("9145-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9145-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9145_all_checks_green() {
        let set = run_unx9145_checks();
        assert!(set.all_passed(), "UNX-F9145 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9145_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
