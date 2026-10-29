//! UNX-F9004 raise/pidfd_send_signal 挂点（0x62040 段）
//!
//! 锚点：UNX-F9004-J1 挂点账（RAISE/PIDFD/DIRECT 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 挂点语义（RAISE 加合 / PIDFD 并合 / DIRECT 直设三式运算 +
//! 键 1~64 域核 + popcount 守恒）；J3 封账印（RAI 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11。
//!
//! 结构：挂点账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! RAI 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 raise/pidfd_send_signal 挂点 的语义单点：以三式账本承载 RAISE/PIDFD/DIRECT
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62040 独占 0x20）。
pub const SEG_BASE: u32 = 0x62040;
pub const RAI_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const RAI_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 64）
pub const RAI_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const RAI_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const RAI_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// RAISE 式：加合（new = old.wrapping_add(val)）。
pub const RAI_OP_RAISE: u32 = 0;
/// PIDFD 式：并合（new = old | val）。
pub const RAI_OP_PIDFD: u32 = 1;
/// DIRECT 式：直设（new = val）。
pub const RAI_OP_DIRECT: u32 = 2;
/// 键域上界（1..=64）。
pub const RAI_KEY_MAX: u8 = 64;

/// op 名渲染。
pub fn rai_op_name(code: u32) -> &'static str {
    match code {
        RAI_OP_RAISE => "RAISE",
        RAI_OP_PIDFD => "PIDFD",
        RAI_OP_DIRECT => "DIRECT",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~64。
pub fn rai_key_ok(key: u8) -> bool {
    key != 0 && key <= RAI_KEY_MAX
}

/// 领域辅助：值钳制（>64 截到 64，短值原样通过）。
pub fn rai_clamp(val: u64) -> u64 {
    if val > 64 {
        64
    } else {
        val
    }
}

/// 单笔挂点账。
#[derive(Clone)]
pub struct RaiseOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl RaiseOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            rai_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn rai_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        RAI_OP_RAISE => old.wrapping_add(val),
        RAI_OP_PIDFD => old | val,
        RAI_OP_DIRECT => val,
        _ => old,
    }
}

/// 挂点账本。
pub struct RaiseLedger {
    ops: Vec<RaiseOp>,
    state: u64,
    sealed: bool,
}

impl RaiseLedger {
    pub fn new() -> Self {
        RaiseLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔挂点：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(RAI_SEALED);
        }
        if code > RAI_OP_DIRECT {
            return Err(RAI_BAD_OP);
        }
        if !rai_key_ok(key) {
            return Err(RAI_KEY_OOB);
        }
        let old = self.state;
        let new = rai_apply(old, code, val);
        self.ops.push(RaiseOp {
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
            if op.new_state != rai_apply(op.old_state, op.code, op.val) {
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

    /// RAI 指纹：状态 + 各笔操作折叠。
    pub fn rai_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9004u32;
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
        out.push_str(&format!("RAI-STATE|{}\n", self.state));
        out.push_str(&format!("RAI-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("RAI-SEAL\n");
        }
        out
    }
}

impl Default for RaiseLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（RAISE[2]=10→PIDFD[9]=128→RAISE[11]=3→DIRECT[9]=101）。
/// 手验：0→10→138→141→101。
fn standard_rai() -> RaiseLedger {
    let mut l = RaiseLedger::new();
    let _ = l.apply(RAI_OP_RAISE, 2, 10); // old 0   → new 10
    let _ = l.apply(RAI_OP_PIDFD, 9, 128); // old 10 → new 138
    let _ = l.apply(RAI_OP_RAISE, 11, 3); // old 138 → new 141
    let _ = l.apply(RAI_OP_DIRECT, 9, 101); // old 141 → new 101
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = RaiseLedger::new();
    if l.apply(7, 1, 1).err() == Some(RAI_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 65 双向）
    let mut l = RaiseLedger::new();
    if l.apply(RAI_OP_RAISE, 0, 1).err() == Some(RAI_KEY_OOB)
        && l.apply(RAI_OP_RAISE, 65, 1).err() == Some(RAI_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_rai();
    let _ = l.seal();
    if l.apply(RAI_OP_RAISE, 1, 1).err() == Some(RAI_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = RaiseLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_rai();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 挂点账；J2 挂点语义；J3 封账印。
pub fn run_unx9004_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9004");
    let p = standard_rai();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9004-J1-操作全落账", ok, if ok { "" } else { "挂点账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 10, 138, 141];
    s.add("9004-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("RAISE|2|old0|new10")
        && text.contains("PIDFD|9|old10|new138")
        && text.contains("RAISE|11|old138|new141")
        && text.contains("DIRECT|9|old141|new101")
        && text.contains("RAI-STATE|101")
        && text.contains("RAI-POPC|4");
    s.add("9004-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 101
    let ok = p.state == 101;
    s.add("9004-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 RAISE 加合（0 + 10 = 10）
    let ok = p.ops[0].code == RAI_OP_RAISE
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 10
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(10);
    s.add("9004-J2-RAISE加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 PIDFD 并合（10 | 128 = 138）
    let ok = p.ops[1].code == RAI_OP_PIDFD
        && p.ops[1].old_state == 10
        && p.ops[1].new_state == 138
        && p.ops[1].new_state == p.ops[1].old_state | 128;
    s.add("9004-J2-PIDFD并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 DIRECT 直设（141 → 101）
    let ok = p.ops[3].code == RAI_OP_DIRECT
        && p.ops[3].old_state == 141
        && p.ops[3].new_state == 101
        && p.ops[3].new_state == 101;
    s.add("9004-J2-DIRECT直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~64 内收、0 与 65 拒）
    let ok = rai_key_ok(2) && rai_key_ok(64) && !rai_key_ok(0) && !rai_key_ok(65);
    s.add("9004-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 钳制辅助（值钳制（>64 截到 64））
    let ok = rai_clamp(10) == 10 && rai_clamp(999) == 64;
    s.add("9004-J2-钳制", ok, if ok { "" } else { "钳制漂移" });
    // J3-1 指纹确定
    let ok = standard_rai().rai_fp() == standard_rai().rai_fp();
    s.add("9004-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_rai();
    let fp0 = p2.rai_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.rai_fp() != fp0;
    s.add("9004-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_rai();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("RAI-SEAL");
    s.add("9004-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(RAI_OP_DIRECT, 0, 0).err() == Some(RAI_SEALED);
    s.add("9004-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9004-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9004_all_checks_green() {
        let set = run_unx9004_checks();
        assert!(set.all_passed(), "UNX-F9004 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9004_state_stable() {
        let l = standard_rai();
        assert!(l.chain_ok());
        assert_eq!(l.state, 101);
        assert_eq!(l.state.count_ones(), 4);
    }
}
