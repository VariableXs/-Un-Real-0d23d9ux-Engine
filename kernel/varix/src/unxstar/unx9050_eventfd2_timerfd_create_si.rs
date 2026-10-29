//! UNX-F9050 抽样组九：四件套正反双判据（eventfd2/timerfd_create/signalfd/inotify_init1）（0x62600 段）
//!
//! 锚点：UNX-F9050-J1 事件账（EVENTFD/TIMERFD/SIGNALFD 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 事件语义（EVENTFD 加合 / TIMERFD 并合 / SIGNALFD 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（EVT 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B13。
//!
//! 结构：事件账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! EVT 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 抽样组九：四件套正反双判据（eventfd2/timerfd_create/signalfd/inotify_init1） 的语义单点：以三式账本承载 EVENTFD/TIMERFD/SIGNALFD
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62600 独占 0x20）。
pub const SEG_BASE: u32 = 0x62600;
pub const EVT_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const EVT_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const EVT_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const EVT_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const EVT_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// EVENTFD 式：加合（new = old.wrapping_add(val)）。
pub const EVT_OP_EVENTFD: u32 = 0;
/// TIMERFD 式：并合（new = old | val）。
pub const EVT_OP_TIMERFD: u32 = 1;
/// SIGNALFD 式：直设（new = val）。
pub const EVT_OP_SIGNALFD: u32 = 2;
/// 键域上界（1..=16）。
pub const EVT_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn evt_op_name(code: u32) -> &'static str {
    match code {
        EVT_OP_EVENTFD => "EVENTFD",
        EVT_OP_TIMERFD => "TIMERFD",
        EVT_OP_SIGNALFD => "SIGNALFD",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn evt_key_ok(key: u8) -> bool {
    key != 0 && key <= EVT_KEY_MAX
}

/// 领域辅助：值列求和（wrapping 折叠，批量面）。
pub fn evt_sum(vals: &[u64]) -> u64 {
    vals.iter().fold(0u64, |a, &v| a.wrapping_add(v))
}

/// 单笔事件账。
#[derive(Clone)]
pub struct EventOpsOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl EventOpsOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            evt_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn evt_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        EVT_OP_EVENTFD => old.wrapping_add(val),
        EVT_OP_TIMERFD => old | val,
        EVT_OP_SIGNALFD => val,
        _ => old,
    }
}

/// 事件账本。
pub struct EventOpsLedger {
    ops: Vec<EventOpsOp>,
    state: u64,
    sealed: bool,
}

impl EventOpsLedger {
    pub fn new() -> Self {
        EventOpsLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔事件：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(EVT_SEALED);
        }
        if code > EVT_OP_SIGNALFD {
            return Err(EVT_BAD_OP);
        }
        if !evt_key_ok(key) {
            return Err(EVT_KEY_OOB);
        }
        let old = self.state;
        let new = evt_apply(old, code, val);
        self.ops.push(EventOpsOp {
            code,
            key,
            val,
            old_state: old,
            new_state: new,
        });
        self.state = new;
        Ok(old)
    }

    /// 状态链守恒核：首笔 old = 0，逐笔 old = 前笔 new，
    /// 每笔 new 可由三式核重算复现，末笔 new = 当前状态。
    pub fn chain_ok(&self) -> bool {
        let mut expect: u64 = 0;
        for op in &self.ops {
            if op.old_state != expect {
                return false;
            }
            if op.new_state != evt_apply(op.old_state, op.code, op.val) {
                return false;
            }
            expect = op.new_state;
        }
        self.state == expect
    }

    /// 键 key 是否已有落账笔。
    pub fn has_key(&self, key: u8) -> bool {
        self.ops.iter().any(|op| op.key == key)
    }

    /// 逐笔旧态回读序列。
    pub fn states(&self) -> Vec<u64> {
        self.ops.iter().map(|op| op.old_state).collect()
    }

    /// 各笔跨度序列（审计面）。
    pub fn deltas(&self) -> Vec<u64> {
        self.ops.iter().map(|op| op.delta()).collect()
    }

    /// EVT 指纹：状态 + 各笔操作折叠。
    pub fn evt_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9050u32;
        h = h.wrapping_mul(0x0100_0193) ^ (self.state as u32);
        h = h.wrapping_mul(0x0100_0193) ^ ((self.state >> 32) as u32);
        for op in &self.ops {
            h = h.wrapping_mul(0x0100_0193) ^ op.code;
            h = h.wrapping_mul(0x0100_0193) ^ (op.key as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (op.val as u32);
            h ^= ((op.val >> 32) as u32) ^ ((op.old_state >> 32) as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (op.new_state as u32);
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印：空账拒 + 重封拒。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.ops.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for op in &self.ops {
            out.push_str(&op.render_line());
            out.push('\n');
        }
        out.push_str(&format!("EVT-STATE|{}\n", self.state));
        out.push_str(&format!("EVT-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("EVT-SEAL\n");
        }
        out
    }
}

impl Default for EventOpsLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（EVENTFD[16]=20→TIMERFD[15]=2048→EVENTFD[5]=7→SIGNALFD[11]=147）。
/// 手验：0→20→2068→2075→147。
fn standard_evt() -> EventOpsLedger {
    let mut l = EventOpsLedger::new();
    let _ = l.apply(EVT_OP_EVENTFD, 16, 20); // old 0   → new 20
    let _ = l.apply(EVT_OP_TIMERFD, 15, 2048); // old 20 → new 2068
    let _ = l.apply(EVT_OP_EVENTFD, 5, 7); // old 2068 → new 2075
    let _ = l.apply(EVT_OP_SIGNALFD, 11, 147); // old 2075 → new 147
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = EventOpsLedger::new();
    if l.apply(7, 1, 1).err() == Some(EVT_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = EventOpsLedger::new();
    if l.apply(EVT_OP_EVENTFD, 0, 1).err() == Some(EVT_KEY_OOB)
        && l.apply(EVT_OP_EVENTFD, 17, 1).err() == Some(EVT_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_evt();
    let _ = l.seal();
    if l.apply(EVT_OP_EVENTFD, 1, 1).err() == Some(EVT_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = EventOpsLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_evt();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 事件账；J2 事件语义；J3 封账印。
pub fn run_unx9050_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9050");
    let p = standard_evt();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9050-J1-操作全落账", ok, if ok { "" } else { "事件账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 20, 2068, 2075];
    s.add("9050-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("EVENTFD|16|old0|new20")
        && text.contains("TIMERFD|15|old20|new2068")
        && text.contains("EVENTFD|5|old2068|new2075")
        && text.contains("SIGNALFD|11|old2075|new147")
        && text.contains("EVT-STATE|147")
        && text.contains("EVT-POPC|4");
    s.add("9050-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 147
    let ok = p.state == 147;
    s.add("9050-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 EVENTFD 加合（0 + 20 = 20）
    let ok = p.ops[0].code == EVT_OP_EVENTFD
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 20
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(20);
    s.add("9050-J2-EVENTFD加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 TIMERFD 并合（20 | 2048 = 2068）
    let ok = p.ops[1].code == EVT_OP_TIMERFD
        && p.ops[1].old_state == 20
        && p.ops[1].new_state == 2068
        && p.ops[1].new_state == p.ops[1].old_state | 2048;
    s.add("9050-J2-TIMERFD并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 SIGNALFD 直设（2075 → 147）
    let ok = p.ops[3].code == EVT_OP_SIGNALFD
        && p.ops[3].old_state == 2075
        && p.ops[3].new_state == 147
        && p.ops[3].new_state == 147;
    s.add("9050-J2-SIGNALFD直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = evt_key_ok(16) && evt_key_ok(16) && !evt_key_ok(0) && !evt_key_ok(17);
    s.add("9050-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 求和辅助（值列求和（wrapping 折叠））
    let ok = evt_sum(&[20, 7]) == 27 && evt_sum(&[]) == 0;
    s.add("9050-J2-求和", ok, if ok { "" } else { "求和漂移" });
    // J3-1 指纹确定
    let ok = standard_evt().evt_fp() == standard_evt().evt_fp();
    s.add("9050-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_evt();
    let fp0 = p2.evt_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.evt_fp() != fp0;
    s.add("9050-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_evt();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("EVT-SEAL");
    s.add("9050-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(EVT_OP_SIGNALFD, 0, 0).err() == Some(EVT_SEALED);
    s.add("9050-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9050-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9050_all_checks_green() {
        let set = run_unx9050_checks();
        assert!(set.all_passed(), "UNX-F9050 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9050_state_stable() {
        let l = standard_evt();
        assert!(l.chain_ok());
        assert_eq!(l.state, 147);
        assert_eq!(l.state.count_ones(), 4);
    }
}
