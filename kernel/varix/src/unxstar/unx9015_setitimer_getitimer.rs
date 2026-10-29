//! UNX-F9015 setitimer/getitimer 间隔定时器（0x621a0 段）
//!
//! 锚点：UNX-F9015-J1 间隔账（START/STOP/REARM 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 间隔语义（START 加合 / STOP 并合 / REARM 直设三式运算 +
//! 键 1~4 域核 + popcount 守恒）；J3 封账印（ITM 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11。
//!
//! 结构：间隔账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! ITM 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 setitimer/getitimer 间隔定时器 的语义单点：以三式账本承载 START/STOP/REARM
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x621a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x621a0;
pub const ITM_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const ITM_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 4）
pub const ITM_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const ITM_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const ITM_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// START 式：加合（new = old.wrapping_add(val)）。
pub const ITM_OP_START: u32 = 0;
/// STOP 式：并合（new = old | val）。
pub const ITM_OP_STOP: u32 = 1;
/// REARM 式：直设（new = val）。
pub const ITM_OP_REARM: u32 = 2;
/// 键域上界（1..=4）。
pub const ITM_KEY_MAX: u8 = 4;

/// op 名渲染。
pub fn itm_op_name(code: u32) -> &'static str {
    match code {
        ITM_OP_START => "START",
        ITM_OP_STOP => "STOP",
        ITM_OP_REARM => "REARM",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~4。
pub fn itm_key_ok(key: u8) -> bool {
    key != 0 && key <= ITM_KEY_MAX
}

/// 领域辅助：幂 2 判定（0 视为非幂 2）。
pub fn itm_is_pow2(val: u64) -> bool {
    val != 0 && val & (val - 1) == 0
}

/// 单笔间隔账。
#[derive(Clone)]
pub struct ITimerOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl ITimerOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            itm_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn itm_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        ITM_OP_START => old.wrapping_add(val),
        ITM_OP_STOP => old | val,
        ITM_OP_REARM => val,
        _ => old,
    }
}

/// 间隔账本。
pub struct ITimerLedger {
    ops: Vec<ITimerOp>,
    state: u64,
    sealed: bool,
}

impl ITimerLedger {
    pub fn new() -> Self {
        ITimerLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔间隔：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(ITM_SEALED);
        }
        if code > ITM_OP_REARM {
            return Err(ITM_BAD_OP);
        }
        if !itm_key_ok(key) {
            return Err(ITM_KEY_OOB);
        }
        let old = self.state;
        let new = itm_apply(old, code, val);
        self.ops.push(ITimerOp {
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
            if op.new_state != itm_apply(op.old_state, op.code, op.val) {
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

    /// ITM 指纹：状态 + 各笔操作折叠。
    pub fn itm_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9015u32;
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
        out.push_str(&format!("ITM-STATE|{}\n", self.state));
        out.push_str(&format!("ITM-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("ITM-SEAL\n");
        }
        out
    }
}

impl Default for ITimerLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（START[1]=9→STOP[4]=64→START[4]=7→REARM[2]=112）。
/// 手验：0→9→73→80→112。
fn standard_itm() -> ITimerLedger {
    let mut l = ITimerLedger::new();
    let _ = l.apply(ITM_OP_START, 1, 9); // old 0   → new 9
    let _ = l.apply(ITM_OP_STOP, 4, 64); // old 9 → new 73
    let _ = l.apply(ITM_OP_START, 4, 7); // old 73 → new 80
    let _ = l.apply(ITM_OP_REARM, 2, 112); // old 80 → new 112
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = ITimerLedger::new();
    if l.apply(7, 1, 1).err() == Some(ITM_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 5 双向）
    let mut l = ITimerLedger::new();
    if l.apply(ITM_OP_START, 0, 1).err() == Some(ITM_KEY_OOB)
        && l.apply(ITM_OP_START, 5, 1).err() == Some(ITM_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_itm();
    let _ = l.seal();
    if l.apply(ITM_OP_START, 1, 1).err() == Some(ITM_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = ITimerLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_itm();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 间隔账；J2 间隔语义；J3 封账印。
pub fn run_unx9015_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9015");
    let p = standard_itm();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9015-J1-操作全落账", ok, if ok { "" } else { "间隔账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 9, 73, 80];
    s.add("9015-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("START|1|old0|new9")
        && text.contains("STOP|4|old9|new73")
        && text.contains("START|4|old73|new80")
        && text.contains("REARM|2|old80|new112")
        && text.contains("ITM-STATE|112")
        && text.contains("ITM-POPC|3");
    s.add("9015-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 112
    let ok = p.state == 112;
    s.add("9015-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 START 加合（0 + 9 = 9）
    let ok = p.ops[0].code == ITM_OP_START
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 9
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(9);
    s.add("9015-J2-START加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 STOP 并合（9 | 64 = 73）
    let ok = p.ops[1].code == ITM_OP_STOP
        && p.ops[1].old_state == 9
        && p.ops[1].new_state == 73
        && p.ops[1].new_state == p.ops[1].old_state | 64;
    s.add("9015-J2-STOP并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 REARM 直设（80 → 112）
    let ok = p.ops[3].code == ITM_OP_REARM
        && p.ops[3].old_state == 80
        && p.ops[3].new_state == 112
        && p.ops[3].new_state == 112;
    s.add("9015-J2-REARM直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~4 内收、0 与 5 拒）
    let ok = itm_key_ok(1) && itm_key_ok(4) && !itm_key_ok(0) && !itm_key_ok(5);
    s.add("9015-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 幂二辅助（幂 2 判定（0 视为非幂 2））
    let ok = itm_is_pow2(64) && itm_is_pow2(9) == false && !itm_is_pow2(0);
    s.add("9015-J2-幂二", ok, if ok { "" } else { "幂二漂移" });
    // J3-1 指纹确定
    let ok = standard_itm().itm_fp() == standard_itm().itm_fp();
    s.add("9015-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_itm();
    let fp0 = p2.itm_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.itm_fp() != fp0;
    s.add("9015-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_itm();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("ITM-SEAL");
    s.add("9015-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(ITM_OP_REARM, 0, 0).err() == Some(ITM_SEALED);
    s.add("9015-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9015-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9015_all_checks_green() {
        let set = run_unx9015_checks();
        assert!(set.all_passed(), "UNX-F9015 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9015_state_stable() {
        let l = standard_itm();
        assert!(l.chain_ok());
        assert_eq!(l.state, 112);
        assert_eq!(l.state.count_ones(), 3);
    }
}
