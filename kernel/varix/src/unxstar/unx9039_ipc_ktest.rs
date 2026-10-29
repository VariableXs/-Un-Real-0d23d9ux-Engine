//! UNX-F9039 IPC 族 ktest 半自动驱动（0x624a0 段）
//!
//! 锚点：UNX-F9039-J1 探针账（ARM/FIRE/CHECK 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 探针语义（ARM 加合 / FIRE 并合 / CHECK 直设三式运算 +
//! 键 1~8 域核 + popcount 守恒）；J3 封账印（IKT 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B12。
//!
//! 结构：探针账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! IKT 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 IPC 族 ktest 半自动驱动 的语义单点：以三式账本承载 ARM/FIRE/CHECK
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x624a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x624a0;
pub const IKT_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const IKT_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 8）
pub const IKT_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const IKT_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const IKT_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// ARM 式：加合（new = old.wrapping_add(val)）。
pub const IKT_OP_ARM: u32 = 0;
/// FIRE 式：并合（new = old | val）。
pub const IKT_OP_FIRE: u32 = 1;
/// CHECK 式：直设（new = val）。
pub const IKT_OP_CHECK: u32 = 2;
/// 键域上界（1..=8）。
pub const IKT_KEY_MAX: u8 = 8;

/// op 名渲染。
pub fn ikt_op_name(code: u32) -> &'static str {
    match code {
        IKT_OP_ARM => "ARM",
        IKT_OP_FIRE => "FIRE",
        IKT_OP_CHECK => "CHECK",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~8。
pub fn ikt_key_ok(key: u8) -> bool {
    key != 0 && key <= IKT_KEY_MAX
}

/// 领域辅助：幂 2 判定（0 视为非幂 2）。
pub fn ikt_is_pow2(val: u64) -> bool {
    val != 0 && val & (val - 1) == 0
}

/// 单笔探针账。
#[derive(Clone)]
pub struct IpcProbeOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl IpcProbeOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            ikt_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn ikt_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        IKT_OP_ARM => old.wrapping_add(val),
        IKT_OP_FIRE => old | val,
        IKT_OP_CHECK => val,
        _ => old,
    }
}

/// 探针账本。
pub struct IpcProbeLedger {
    ops: Vec<IpcProbeOp>,
    state: u64,
    sealed: bool,
}

impl IpcProbeLedger {
    pub fn new() -> Self {
        IpcProbeLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔探针：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(IKT_SEALED);
        }
        if code > IKT_OP_CHECK {
            return Err(IKT_BAD_OP);
        }
        if !ikt_key_ok(key) {
            return Err(IKT_KEY_OOB);
        }
        let old = self.state;
        let new = ikt_apply(old, code, val);
        self.ops.push(IpcProbeOp {
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
            if op.new_state != ikt_apply(op.old_state, op.code, op.val) {
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

    /// IKT 指纹：状态 + 各笔操作折叠。
    pub fn ikt_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9039u32;
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
        out.push_str(&format!("IKT-STATE|{}\n", self.state));
        out.push_str(&format!("IKT-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("IKT-SEAL\n");
        }
        out
    }
}

impl Default for IpcProbeLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（ARM[5]=9→FIRE[8]=64→ARM[4]=3→CHECK[6]=136）。
/// 手验：0→9→73→76→136。
fn standard_ikt() -> IpcProbeLedger {
    let mut l = IpcProbeLedger::new();
    let _ = l.apply(IKT_OP_ARM, 5, 9); // old 0   → new 9
    let _ = l.apply(IKT_OP_FIRE, 8, 64); // old 9 → new 73
    let _ = l.apply(IKT_OP_ARM, 4, 3); // old 73 → new 76
    let _ = l.apply(IKT_OP_CHECK, 6, 136); // old 76 → new 136
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = IpcProbeLedger::new();
    if l.apply(7, 1, 1).err() == Some(IKT_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 9 双向）
    let mut l = IpcProbeLedger::new();
    if l.apply(IKT_OP_ARM, 0, 1).err() == Some(IKT_KEY_OOB)
        && l.apply(IKT_OP_ARM, 9, 1).err() == Some(IKT_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_ikt();
    let _ = l.seal();
    if l.apply(IKT_OP_ARM, 1, 1).err() == Some(IKT_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = IpcProbeLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_ikt();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 探针账；J2 探针语义；J3 封账印。
pub fn run_unx9039_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9039");
    let p = standard_ikt();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9039-J1-操作全落账", ok, if ok { "" } else { "探针账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 9, 73, 76];
    s.add("9039-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("ARM|5|old0|new9")
        && text.contains("FIRE|8|old9|new73")
        && text.contains("ARM|4|old73|new76")
        && text.contains("CHECK|6|old76|new136")
        && text.contains("IKT-STATE|136")
        && text.contains("IKT-POPC|2");
    s.add("9039-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 136
    let ok = p.state == 136;
    s.add("9039-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 ARM 加合（0 + 9 = 9）
    let ok = p.ops[0].code == IKT_OP_ARM
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 9
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(9);
    s.add("9039-J2-ARM加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 FIRE 并合（9 | 64 = 73）
    let ok = p.ops[1].code == IKT_OP_FIRE
        && p.ops[1].old_state == 9
        && p.ops[1].new_state == 73
        && p.ops[1].new_state == p.ops[1].old_state | 64;
    s.add("9039-J2-FIRE并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 CHECK 直设（76 → 136）
    let ok = p.ops[3].code == IKT_OP_CHECK
        && p.ops[3].old_state == 76
        && p.ops[3].new_state == 136
        && p.ops[3].new_state == 136;
    s.add("9039-J2-CHECK直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~8 内收、0 与 9 拒）
    let ok = ikt_key_ok(5) && ikt_key_ok(8) && !ikt_key_ok(0) && !ikt_key_ok(9);
    s.add("9039-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 幂二辅助（幂 2 判定（0 视为非幂 2））
    let ok = ikt_is_pow2(64) && ikt_is_pow2(9) == false && !ikt_is_pow2(0);
    s.add("9039-J2-幂二", ok, if ok { "" } else { "幂二漂移" });
    // J3-1 指纹确定
    let ok = standard_ikt().ikt_fp() == standard_ikt().ikt_fp();
    s.add("9039-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_ikt();
    let fp0 = p2.ikt_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.ikt_fp() != fp0;
    s.add("9039-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_ikt();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("IKT-SEAL");
    s.add("9039-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(IKT_OP_CHECK, 0, 0).err() == Some(IKT_SEALED);
    s.add("9039-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9039-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9039_all_checks_green() {
        let set = run_unx9039_checks();
        assert!(set.all_passed(), "UNX-F9039 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9039_state_stable() {
        let l = standard_ikt();
        assert!(l.chain_ok());
        assert_eq!(l.state, 136);
        assert_eq!(l.state.count_ones(), 2);
    }
}
