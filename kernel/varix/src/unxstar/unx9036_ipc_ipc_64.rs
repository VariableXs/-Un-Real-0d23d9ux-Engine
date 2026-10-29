//! UNX-F9036 ipc 系统调用兼容门（IPC_64 旗标）（0x62440 段）
//!
//! 锚点：UNX-F9036-J1 旗标账（LEGACY/MODERN/NORM 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 旗标语义（LEGACY 加合 / MODERN 并合 / NORM 直设三式运算 +
//! 键 1~4 域核 + popcount 守恒）；J3 封账印（I64 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B12。
//!
//! 结构：旗标账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! I64 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 ipc 系统调用兼容门（IPC_64 旗标） 的语义单点：以三式账本承载 LEGACY/MODERN/NORM
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62440 独占 0x20）。
pub const SEG_BASE: u32 = 0x62440;
pub const I64_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const I64_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 4）
pub const I64_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const I64_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const I64_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// LEGACY 式：加合（new = old.wrapping_add(val)）。
pub const I64_OP_LEGACY: u32 = 0;
/// MODERN 式：并合（new = old | val）。
pub const I64_OP_MODERN: u32 = 1;
/// NORM 式：直设（new = val）。
pub const I64_OP_NORM: u32 = 2;
/// 键域上界（1..=4）。
pub const I64_KEY_MAX: u8 = 4;

/// op 名渲染。
pub fn i64_op_name(code: u32) -> &'static str {
    match code {
        I64_OP_LEGACY => "LEGACY",
        I64_OP_MODERN => "MODERN",
        I64_OP_NORM => "NORM",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~4。
pub fn i64_key_ok(key: u8) -> bool {
    key != 0 && key <= I64_KEY_MAX
}

/// 领域辅助：值钳制（>64 截到 64，短值原样通过）。
pub fn i64_clamp(val: u64) -> u64 {
    if val > 64 {
        64
    } else {
        val
    }
}

/// 单笔旗标账。
#[derive(Clone)]
pub struct Ipc64Op {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl Ipc64Op {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            i64_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn i64_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        I64_OP_LEGACY => old.wrapping_add(val),
        I64_OP_MODERN => old | val,
        I64_OP_NORM => val,
        _ => old,
    }
}

/// 旗标账本。
pub struct Ipc64Ledger {
    ops: Vec<Ipc64Op>,
    state: u64,
    sealed: bool,
}

impl Ipc64Ledger {
    pub fn new() -> Self {
        Ipc64Ledger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔旗标：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(I64_SEALED);
        }
        if code > I64_OP_NORM {
            return Err(I64_BAD_OP);
        }
        if !i64_key_ok(key) {
            return Err(I64_KEY_OOB);
        }
        let old = self.state;
        let new = i64_apply(old, code, val);
        self.ops.push(Ipc64Op {
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
            if op.new_state != i64_apply(op.old_state, op.code, op.val) {
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

    /// I64 指纹：状态 + 各笔操作折叠。
    pub fn i64_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9036u32;
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
        out.push_str(&format!("I64-STATE|{}\n", self.state));
        out.push_str(&format!("I64-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("I64-SEAL\n");
        }
        out
    }
}

impl Default for Ipc64Ledger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（LEGACY[2]=18→MODERN[1]=512→LEGACY[3]=7→NORM[1]=133）。
/// 手验：0→18→530→537→133。
fn standard_i64() -> Ipc64Ledger {
    let mut l = Ipc64Ledger::new();
    let _ = l.apply(I64_OP_LEGACY, 2, 18); // old 0   → new 18
    let _ = l.apply(I64_OP_MODERN, 1, 512); // old 18 → new 530
    let _ = l.apply(I64_OP_LEGACY, 3, 7); // old 530 → new 537
    let _ = l.apply(I64_OP_NORM, 1, 133); // old 537 → new 133
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = Ipc64Ledger::new();
    if l.apply(7, 1, 1).err() == Some(I64_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 5 双向）
    let mut l = Ipc64Ledger::new();
    if l.apply(I64_OP_LEGACY, 0, 1).err() == Some(I64_KEY_OOB)
        && l.apply(I64_OP_LEGACY, 5, 1).err() == Some(I64_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_i64();
    let _ = l.seal();
    if l.apply(I64_OP_LEGACY, 1, 1).err() == Some(I64_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = Ipc64Ledger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_i64();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 旗标账；J2 旗标语义；J3 封账印。
pub fn run_unx9036_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9036");
    let p = standard_i64();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9036-J1-操作全落账", ok, if ok { "" } else { "旗标账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 18, 530, 537];
    s.add("9036-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("LEGACY|2|old0|new18")
        && text.contains("MODERN|1|old18|new530")
        && text.contains("LEGACY|3|old530|new537")
        && text.contains("NORM|1|old537|new133")
        && text.contains("I64-STATE|133")
        && text.contains("I64-POPC|3");
    s.add("9036-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 133
    let ok = p.state == 133;
    s.add("9036-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 LEGACY 加合（0 + 18 = 18）
    let ok = p.ops[0].code == I64_OP_LEGACY
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 18
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(18);
    s.add("9036-J2-LEGACY加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 MODERN 并合（18 | 512 = 530）
    let ok = p.ops[1].code == I64_OP_MODERN
        && p.ops[1].old_state == 18
        && p.ops[1].new_state == 530
        && p.ops[1].new_state == p.ops[1].old_state | 512;
    s.add("9036-J2-MODERN并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 NORM 直设（537 → 133）
    let ok = p.ops[3].code == I64_OP_NORM
        && p.ops[3].old_state == 537
        && p.ops[3].new_state == 133
        && p.ops[3].new_state == 133;
    s.add("9036-J2-NORM直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~4 内收、0 与 5 拒）
    let ok = i64_key_ok(2) && i64_key_ok(4) && !i64_key_ok(0) && !i64_key_ok(5);
    s.add("9036-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 钳制辅助（值钳制（>64 截到 64））
    let ok = i64_clamp(18) == 18 && i64_clamp(999) == 64;
    s.add("9036-J2-钳制", ok, if ok { "" } else { "钳制漂移" });
    // J3-1 指纹确定
    let ok = standard_i64().i64_fp() == standard_i64().i64_fp();
    s.add("9036-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_i64();
    let fp0 = p2.i64_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.i64_fp() != fp0;
    s.add("9036-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_i64();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("I64-SEAL");
    s.add("9036-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(I64_OP_NORM, 0, 0).err() == Some(I64_SEALED);
    s.add("9036-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9036-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9036_all_checks_green() {
        let set = run_unx9036_checks();
        assert!(set.all_passed(), "UNX-F9036 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9036_state_stable() {
        let l = standard_i64();
        assert!(l.chain_ok());
        assert_eq!(l.state, 133);
        assert_eq!(l.state.count_ones(), 3);
    }
}
