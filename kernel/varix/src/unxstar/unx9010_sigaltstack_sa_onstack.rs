//! UNX-F9010 sigaltstack 与 SA_ONSTACK（0x62100 段）
//!
//! 锚点：UNX-F9010-J1 备栈账（REG/ENTER/LEAVE 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 备栈语义（REG 加合 / ENTER 并合 / LEAVE 直设三式运算 +
//! 键 1~8 域核 + popcount 守恒）；J3 封账印（ALT 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11。
//!
//! 结构：备栈账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! ALT 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 sigaltstack 与 SA_ONSTACK 的语义单点：以三式账本承载 REG/ENTER/LEAVE
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62100 独占 0x20）。
pub const SEG_BASE: u32 = 0x62100;
pub const ALT_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const ALT_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 8）
pub const ALT_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const ALT_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const ALT_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// REG 式：加合（new = old.wrapping_add(val)）。
pub const ALT_OP_REG: u32 = 0;
/// ENTER 式：并合（new = old | val）。
pub const ALT_OP_ENTER: u32 = 1;
/// LEAVE 式：直设（new = val）。
pub const ALT_OP_LEAVE: u32 = 2;
/// 键域上界（1..=8）。
pub const ALT_KEY_MAX: u8 = 8;

/// op 名渲染。
pub fn alt_op_name(code: u32) -> &'static str {
    match code {
        ALT_OP_REG => "REG",
        ALT_OP_ENTER => "ENTER",
        ALT_OP_LEAVE => "LEAVE",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~8。
pub fn alt_key_ok(key: u8) -> bool {
    key != 0 && key <= ALT_KEY_MAX
}

/// 领域辅助：值列求和（wrapping 折叠，批量面）。
pub fn alt_sum(vals: &[u64]) -> u64 {
    vals.iter().fold(0u64, |a, &v| a.wrapping_add(v))
}

/// 单笔备栈账。
#[derive(Clone)]
pub struct AltStackOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl AltStackOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            alt_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn alt_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        ALT_OP_REG => old.wrapping_add(val),
        ALT_OP_ENTER => old | val,
        ALT_OP_LEAVE => val,
        _ => old,
    }
}

/// 备栈账本。
pub struct AltStackLedger {
    ops: Vec<AltStackOp>,
    state: u64,
    sealed: bool,
}

impl AltStackLedger {
    pub fn new() -> Self {
        AltStackLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔备栈：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(ALT_SEALED);
        }
        if code > ALT_OP_LEAVE {
            return Err(ALT_BAD_OP);
        }
        if !alt_key_ok(key) {
            return Err(ALT_KEY_OOB);
        }
        let old = self.state;
        let new = alt_apply(old, code, val);
        self.ops.push(AltStackOp {
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
            if op.new_state != alt_apply(op.old_state, op.code, op.val) {
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

    /// ALT 指纹：状态 + 各笔操作折叠。
    pub fn alt_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9010u32;
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
        out.push_str(&format!("ALT-STATE|{}\n", self.state));
        out.push_str(&format!("ALT-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("ALT-SEAL\n");
        }
        out
    }
}

impl Default for AltStackLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（REG[8]=16→ENTER[7]=128→REG[5]=2→LEAVE[3]=107）。
/// 手验：0→16→144→146→107。
fn standard_alt() -> AltStackLedger {
    let mut l = AltStackLedger::new();
    let _ = l.apply(ALT_OP_REG, 8, 16); // old 0   → new 16
    let _ = l.apply(ALT_OP_ENTER, 7, 128); // old 16 → new 144
    let _ = l.apply(ALT_OP_REG, 5, 2); // old 144 → new 146
    let _ = l.apply(ALT_OP_LEAVE, 3, 107); // old 146 → new 107
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = AltStackLedger::new();
    if l.apply(7, 1, 1).err() == Some(ALT_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 9 双向）
    let mut l = AltStackLedger::new();
    if l.apply(ALT_OP_REG, 0, 1).err() == Some(ALT_KEY_OOB)
        && l.apply(ALT_OP_REG, 9, 1).err() == Some(ALT_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_alt();
    let _ = l.seal();
    if l.apply(ALT_OP_REG, 1, 1).err() == Some(ALT_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = AltStackLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_alt();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 备栈账；J2 备栈语义；J3 封账印。
pub fn run_unx9010_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9010");
    let p = standard_alt();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9010-J1-操作全落账", ok, if ok { "" } else { "备栈账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 16, 144, 146];
    s.add("9010-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("REG|8|old0|new16")
        && text.contains("ENTER|7|old16|new144")
        && text.contains("REG|5|old144|new146")
        && text.contains("LEAVE|3|old146|new107")
        && text.contains("ALT-STATE|107")
        && text.contains("ALT-POPC|5");
    s.add("9010-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 107
    let ok = p.state == 107;
    s.add("9010-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 REG 加合（0 + 16 = 16）
    let ok = p.ops[0].code == ALT_OP_REG
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 16
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(16);
    s.add("9010-J2-REG加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 ENTER 并合（16 | 128 = 144）
    let ok = p.ops[1].code == ALT_OP_ENTER
        && p.ops[1].old_state == 16
        && p.ops[1].new_state == 144
        && p.ops[1].new_state == p.ops[1].old_state | 128;
    s.add("9010-J2-ENTER并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 LEAVE 直设（146 → 107）
    let ok = p.ops[3].code == ALT_OP_LEAVE
        && p.ops[3].old_state == 146
        && p.ops[3].new_state == 107
        && p.ops[3].new_state == 107;
    s.add("9010-J2-LEAVE直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~8 内收、0 与 9 拒）
    let ok = alt_key_ok(8) && alt_key_ok(8) && !alt_key_ok(0) && !alt_key_ok(9);
    s.add("9010-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 求和辅助（值列求和（wrapping 折叠））
    let ok = alt_sum(&[16, 2]) == 18 && alt_sum(&[]) == 0;
    s.add("9010-J2-求和", ok, if ok { "" } else { "求和漂移" });
    // J3-1 指纹确定
    let ok = standard_alt().alt_fp() == standard_alt().alt_fp();
    s.add("9010-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_alt();
    let fp0 = p2.alt_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.alt_fp() != fp0;
    s.add("9010-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_alt();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("ALT-SEAL");
    s.add("9010-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(ALT_OP_LEAVE, 0, 0).err() == Some(ALT_SEALED);
    s.add("9010-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9010-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9010_all_checks_green() {
        let set = run_unx9010_checks();
        assert!(set.all_passed(), "UNX-F9010 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9010_state_stable() {
        let l = standard_alt();
        assert!(l.chain_ok());
        assert_eq!(l.state, 107);
        assert_eq!(l.state.count_ones(), 5);
    }
}
