//! UNX-F9020 ktest syscall 面 B11 批断言集（0x62240 段）
//!
//! 锚点：UNX-F9020-J1 批断账（REG/RUN/VERD 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 批断语义（REG 加合 / RUN 并合 / VERD 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（KB1 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11。
//!
//! 结构：批断账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! KB1 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 ktest syscall 面 B11 批断言集 的语义单点：以三式账本承载 REG/RUN/VERD
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62240 独占 0x20）。
pub const SEG_BASE: u32 = 0x62240;
pub const KB1_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const KB1_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const KB1_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const KB1_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const KB1_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// REG 式：加合（new = old.wrapping_add(val)）。
pub const KB1_OP_REG: u32 = 0;
/// RUN 式：并合（new = old | val）。
pub const KB1_OP_RUN: u32 = 1;
/// VERD 式：直设（new = val）。
pub const KB1_OP_VERD: u32 = 2;
/// 键域上界（1..=16）。
pub const KB1_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn kb1_op_name(code: u32) -> &'static str {
    match code {
        KB1_OP_REG => "REG",
        KB1_OP_RUN => "RUN",
        KB1_OP_VERD => "VERD",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn kb1_key_ok(key: u8) -> bool {
    key != 0 && key <= KB1_KEY_MAX
}

/// 领域辅助：值钳制（>64 截到 64，短值原样通过）。
pub fn kb1_clamp(val: u64) -> u64 {
    if val > 64 {
        64
    } else {
        val
    }
}

/// 单笔批断账。
#[derive(Clone)]
pub struct B11AssertOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl B11AssertOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            kb1_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn kb1_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        KB1_OP_REG => old.wrapping_add(val),
        KB1_OP_RUN => old | val,
        KB1_OP_VERD => val,
        _ => old,
    }
}

/// 批断账本。
pub struct B11AssertLedger {
    ops: Vec<B11AssertOp>,
    state: u64,
    sealed: bool,
}

impl B11AssertLedger {
    pub fn new() -> Self {
        B11AssertLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔批断：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(KB1_SEALED);
        }
        if code > KB1_OP_VERD {
            return Err(KB1_BAD_OP);
        }
        if !kb1_key_ok(key) {
            return Err(KB1_KEY_OOB);
        }
        let old = self.state;
        let new = kb1_apply(old, code, val);
        self.ops.push(B11AssertOp {
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
            if op.new_state != kb1_apply(op.old_state, op.code, op.val) {
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

    /// KB1 指纹：状态 + 各笔操作折叠。
    pub fn kb1_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9020u32;
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
        out.push_str(&format!("KB1-STATE|{}\n", self.state));
        out.push_str(&format!("KB1-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("KB1-SEAL\n");
        }
        out
    }
}

impl Default for B11AssertLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（REG[2]=14→RUN[9]=2048→REG[11]=5→VERD[9]=117）。
/// 手验：0→14→2062→2067→117。
fn standard_kb1() -> B11AssertLedger {
    let mut l = B11AssertLedger::new();
    let _ = l.apply(KB1_OP_REG, 2, 14); // old 0   → new 14
    let _ = l.apply(KB1_OP_RUN, 9, 2048); // old 14 → new 2062
    let _ = l.apply(KB1_OP_REG, 11, 5); // old 2062 → new 2067
    let _ = l.apply(KB1_OP_VERD, 9, 117); // old 2067 → new 117
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = B11AssertLedger::new();
    if l.apply(7, 1, 1).err() == Some(KB1_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = B11AssertLedger::new();
    if l.apply(KB1_OP_REG, 0, 1).err() == Some(KB1_KEY_OOB)
        && l.apply(KB1_OP_REG, 17, 1).err() == Some(KB1_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_kb1();
    let _ = l.seal();
    if l.apply(KB1_OP_REG, 1, 1).err() == Some(KB1_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = B11AssertLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_kb1();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 批断账；J2 批断语义；J3 封账印。
pub fn run_unx9020_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9020");
    let p = standard_kb1();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9020-J1-操作全落账", ok, if ok { "" } else { "批断账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 14, 2062, 2067];
    s.add("9020-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("REG|2|old0|new14")
        && text.contains("RUN|9|old14|new2062")
        && text.contains("REG|11|old2062|new2067")
        && text.contains("VERD|9|old2067|new117")
        && text.contains("KB1-STATE|117")
        && text.contains("KB1-POPC|5");
    s.add("9020-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 117
    let ok = p.state == 117;
    s.add("9020-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 REG 加合（0 + 14 = 14）
    let ok = p.ops[0].code == KB1_OP_REG
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 14
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(14);
    s.add("9020-J2-REG加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 RUN 并合（14 | 2048 = 2062）
    let ok = p.ops[1].code == KB1_OP_RUN
        && p.ops[1].old_state == 14
        && p.ops[1].new_state == 2062
        && p.ops[1].new_state == p.ops[1].old_state | 2048;
    s.add("9020-J2-RUN并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 VERD 直设（2067 → 117）
    let ok = p.ops[3].code == KB1_OP_VERD
        && p.ops[3].old_state == 2067
        && p.ops[3].new_state == 117
        && p.ops[3].new_state == 117;
    s.add("9020-J2-VERD直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = kb1_key_ok(2) && kb1_key_ok(16) && !kb1_key_ok(0) && !kb1_key_ok(17);
    s.add("9020-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 钳制辅助（值钳制（>64 截到 64））
    let ok = kb1_clamp(14) == 14 && kb1_clamp(999) == 64;
    s.add("9020-J2-钳制", ok, if ok { "" } else { "钳制漂移" });
    // J3-1 指纹确定
    let ok = standard_kb1().kb1_fp() == standard_kb1().kb1_fp();
    s.add("9020-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_kb1();
    let fp0 = p2.kb1_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.kb1_fp() != fp0;
    s.add("9020-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_kb1();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("KB1-SEAL");
    s.add("9020-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(KB1_OP_VERD, 0, 0).err() == Some(KB1_SEALED);
    s.add("9020-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9020-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9020_all_checks_green() {
        let set = run_unx9020_checks();
        assert!(set.all_passed(), "UNX-F9020 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9020_state_stable() {
        let l = standard_kb1();
        assert!(l.chain_ok());
        assert_eq!(l.state, 117);
        assert_eq!(l.state.count_ones(), 5);
    }
}
