//! UNX-F9046 抽样组五：时间族正反双判据（clock_gettime/nanosleep/gettimeofday）（0x62580 段）
//!
//! 锚点：UNX-F9046-J1 时间账（CLOCK/SLEEP/GETTOD 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 时间语义（CLOCK 加合 / SLEEP 并合 / GETTOD 直设三式运算 +
//! 键 1~8 域核 + popcount 守恒）；J3 封账印（TIC 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B13。
//!
//! 结构：时间账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! TIC 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 抽样组五：时间族正反双判据（clock_gettime/nanosleep/gettimeofday） 的语义单点：以三式账本承载 CLOCK/SLEEP/GETTOD
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62580 独占 0x20）。
pub const SEG_BASE: u32 = 0x62580;
pub const TIC_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const TIC_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 8）
pub const TIC_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const TIC_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const TIC_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// CLOCK 式：加合（new = old.wrapping_add(val)）。
pub const TIC_OP_CLOCK: u32 = 0;
/// SLEEP 式：并合（new = old | val）。
pub const TIC_OP_SLEEP: u32 = 1;
/// GETTOD 式：直设（new = val）。
pub const TIC_OP_GETTOD: u32 = 2;
/// 键域上界（1..=8）。
pub const TIC_KEY_MAX: u8 = 8;

/// op 名渲染。
pub fn tic_op_name(code: u32) -> &'static str {
    match code {
        TIC_OP_CLOCK => "CLOCK",
        TIC_OP_SLEEP => "SLEEP",
        TIC_OP_GETTOD => "GETTOD",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~8。
pub fn tic_key_ok(key: u8) -> bool {
    key != 0 && key <= TIC_KEY_MAX
}

/// 领域辅助：两态跨度（饱和差，b < a 时归零）。
pub fn tic_span(a: u64, b: u64) -> u64 {
    b.saturating_sub(a)
}

/// 单笔时间账。
#[derive(Clone)]
pub struct TimeOpsOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl TimeOpsOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            tic_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn tic_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        TIC_OP_CLOCK => old.wrapping_add(val),
        TIC_OP_SLEEP => old | val,
        TIC_OP_GETTOD => val,
        _ => old,
    }
}

/// 时间账本。
pub struct TimeOpsLedger {
    ops: Vec<TimeOpsOp>,
    state: u64,
    sealed: bool,
}

impl TimeOpsLedger {
    pub fn new() -> Self {
        TimeOpsLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔时间：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(TIC_SEALED);
        }
        if code > TIC_OP_GETTOD {
            return Err(TIC_BAD_OP);
        }
        if !tic_key_ok(key) {
            return Err(TIC_KEY_OOB);
        }
        let old = self.state;
        let new = tic_apply(old, code, val);
        self.ops.push(TimeOpsOp {
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
            if op.new_state != tic_apply(op.old_state, op.code, op.val) {
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

    /// TIC 指纹：状态 + 各笔操作折叠。
    pub fn tic_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9046u32;
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
        out.push_str(&format!("TIC-STATE|{}\n", self.state));
        out.push_str(&format!("TIC-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("TIC-SEAL\n");
        }
        out
    }
}

impl Default for TimeOpsLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（CLOCK[4]=16→SLEEP[3]=128→CLOCK[1]=3→GETTOD[7]=143）。
/// 手验：0→16→144→147→143。
fn standard_tic() -> TimeOpsLedger {
    let mut l = TimeOpsLedger::new();
    let _ = l.apply(TIC_OP_CLOCK, 4, 16); // old 0   → new 16
    let _ = l.apply(TIC_OP_SLEEP, 3, 128); // old 16 → new 144
    let _ = l.apply(TIC_OP_CLOCK, 1, 3); // old 144 → new 147
    let _ = l.apply(TIC_OP_GETTOD, 7, 143); // old 147 → new 143
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = TimeOpsLedger::new();
    if l.apply(7, 1, 1).err() == Some(TIC_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 9 双向）
    let mut l = TimeOpsLedger::new();
    if l.apply(TIC_OP_CLOCK, 0, 1).err() == Some(TIC_KEY_OOB)
        && l.apply(TIC_OP_CLOCK, 9, 1).err() == Some(TIC_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_tic();
    let _ = l.seal();
    if l.apply(TIC_OP_CLOCK, 1, 1).err() == Some(TIC_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = TimeOpsLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_tic();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 时间账；J2 时间语义；J3 封账印。
pub fn run_unx9046_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9046");
    let p = standard_tic();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9046-J1-操作全落账", ok, if ok { "" } else { "时间账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 16, 144, 147];
    s.add("9046-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("CLOCK|4|old0|new16")
        && text.contains("SLEEP|3|old16|new144")
        && text.contains("CLOCK|1|old144|new147")
        && text.contains("GETTOD|7|old147|new143")
        && text.contains("TIC-STATE|143")
        && text.contains("TIC-POPC|5");
    s.add("9046-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 143
    let ok = p.state == 143;
    s.add("9046-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 CLOCK 加合（0 + 16 = 16）
    let ok = p.ops[0].code == TIC_OP_CLOCK
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 16
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(16);
    s.add("9046-J2-CLOCK加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 SLEEP 并合（16 | 128 = 144）
    let ok = p.ops[1].code == TIC_OP_SLEEP
        && p.ops[1].old_state == 16
        && p.ops[1].new_state == 144
        && p.ops[1].new_state == p.ops[1].old_state | 128;
    s.add("9046-J2-SLEEP并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 GETTOD 直设（147 → 143）
    let ok = p.ops[3].code == TIC_OP_GETTOD
        && p.ops[3].old_state == 147
        && p.ops[3].new_state == 143
        && p.ops[3].new_state == 143;
    s.add("9046-J2-GETTOD直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~8 内收、0 与 9 拒）
    let ok = tic_key_ok(4) && tic_key_ok(8) && !tic_key_ok(0) && !tic_key_ok(9);
    s.add("9046-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 跨度辅助（两态跨度（饱和差））
    let ok = tic_span(0, 16) == 16 && tic_span(16, 16) == 0;
    s.add("9046-J2-跨度", ok, if ok { "" } else { "跨度漂移" });
    // J3-1 指纹确定
    let ok = standard_tic().tic_fp() == standard_tic().tic_fp();
    s.add("9046-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_tic();
    let fp0 = p2.tic_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.tic_fp() != fp0;
    s.add("9046-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_tic();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("TIC-SEAL");
    s.add("9046-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(TIC_OP_GETTOD, 0, 0).err() == Some(TIC_SEALED);
    s.add("9046-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9046-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9046_all_checks_green() {
        let set = run_unx9046_checks();
        assert!(set.all_passed(), "UNX-F9046 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9046_state_stable() {
        let l = standard_tic();
        assert!(l.chain_ok());
        assert_eq!(l.state, 143);
        assert_eq!(l.state.count_ones(), 5);
    }
}
