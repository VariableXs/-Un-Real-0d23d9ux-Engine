//! UNX-F9034 IPC 资源限额（RLIMIT/MSGMNB/SEMMNS）落账（0x62400 段）
//!
//! 锚点：UNX-F9034-J1 限额账（SET/HIT/CLAMP 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 限额语义（SET 加合 / HIT 并合 / CLAMP 直设三式运算 +
//! 键 1~8 域核 + popcount 守恒）；J3 封账印（ILM 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B12。
//!
//! 结构：限额账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! ILM 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 IPC 资源限额（RLIMIT/MSGMNB/SEMMNS）落账 的语义单点：以三式账本承载 SET/HIT/CLAMP
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62400 独占 0x20）。
pub const SEG_BASE: u32 = 0x62400;
pub const ILM_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const ILM_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 8）
pub const ILM_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const ILM_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const ILM_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// SET 式：加合（new = old.wrapping_add(val)）。
pub const ILM_OP_SET: u32 = 0;
/// HIT 式：并合（new = old | val）。
pub const ILM_OP_HIT: u32 = 1;
/// CLAMP 式：直设（new = val）。
pub const ILM_OP_CLAMP: u32 = 2;
/// 键域上界（1..=8）。
pub const ILM_KEY_MAX: u8 = 8;

/// op 名渲染。
pub fn ilm_op_name(code: u32) -> &'static str {
    match code {
        ILM_OP_SET => "SET",
        ILM_OP_HIT => "HIT",
        ILM_OP_CLAMP => "CLAMP",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~8。
pub fn ilm_key_ok(key: u8) -> bool {
    key != 0 && key <= ILM_KEY_MAX
}

/// 领域辅助：值列求和（wrapping 折叠，批量面）。
pub fn ilm_sum(vals: &[u64]) -> u64 {
    vals.iter().fold(0u64, |a, &v| a.wrapping_add(v))
}

/// 单笔限额账。
#[derive(Clone)]
pub struct IpcLimitOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl IpcLimitOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            ilm_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn ilm_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        ILM_OP_SET => old.wrapping_add(val),
        ILM_OP_HIT => old | val,
        ILM_OP_CLAMP => val,
        _ => old,
    }
}

/// 限额账本。
pub struct IpcLimitLedger {
    ops: Vec<IpcLimitOp>,
    state: u64,
    sealed: bool,
}

impl IpcLimitLedger {
    pub fn new() -> Self {
        IpcLimitLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔限额：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(ILM_SEALED);
        }
        if code > ILM_OP_CLAMP {
            return Err(ILM_BAD_OP);
        }
        if !ilm_key_ok(key) {
            return Err(ILM_KEY_OOB);
        }
        let old = self.state;
        let new = ilm_apply(old, code, val);
        self.ops.push(IpcLimitOp {
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
            if op.new_state != ilm_apply(op.old_state, op.code, op.val) {
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

    /// ILM 指纹：状态 + 各笔操作折叠。
    pub fn ilm_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9034u32;
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
        out.push_str(&format!("ILM-STATE|{}\n", self.state));
        out.push_str(&format!("ILM-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("ILM-SEAL\n");
        }
        out
    }
}

impl Default for IpcLimitLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（SET[8]=16→HIT[7]=128→SET[5]=5→CLAMP[3]=131）。
/// 手验：0→16→144→149→131。
fn standard_ilm() -> IpcLimitLedger {
    let mut l = IpcLimitLedger::new();
    let _ = l.apply(ILM_OP_SET, 8, 16); // old 0   → new 16
    let _ = l.apply(ILM_OP_HIT, 7, 128); // old 16 → new 144
    let _ = l.apply(ILM_OP_SET, 5, 5); // old 144 → new 149
    let _ = l.apply(ILM_OP_CLAMP, 3, 131); // old 149 → new 131
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = IpcLimitLedger::new();
    if l.apply(7, 1, 1).err() == Some(ILM_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 9 双向）
    let mut l = IpcLimitLedger::new();
    if l.apply(ILM_OP_SET, 0, 1).err() == Some(ILM_KEY_OOB)
        && l.apply(ILM_OP_SET, 9, 1).err() == Some(ILM_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_ilm();
    let _ = l.seal();
    if l.apply(ILM_OP_SET, 1, 1).err() == Some(ILM_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = IpcLimitLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_ilm();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 限额账；J2 限额语义；J3 封账印。
pub fn run_unx9034_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9034");
    let p = standard_ilm();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9034-J1-操作全落账", ok, if ok { "" } else { "限额账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 16, 144, 149];
    s.add("9034-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("SET|8|old0|new16")
        && text.contains("HIT|7|old16|new144")
        && text.contains("SET|5|old144|new149")
        && text.contains("CLAMP|3|old149|new131")
        && text.contains("ILM-STATE|131")
        && text.contains("ILM-POPC|3");
    s.add("9034-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 131
    let ok = p.state == 131;
    s.add("9034-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 SET 加合（0 + 16 = 16）
    let ok = p.ops[0].code == ILM_OP_SET
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 16
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(16);
    s.add("9034-J2-SET加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 HIT 并合（16 | 128 = 144）
    let ok = p.ops[1].code == ILM_OP_HIT
        && p.ops[1].old_state == 16
        && p.ops[1].new_state == 144
        && p.ops[1].new_state == p.ops[1].old_state | 128;
    s.add("9034-J2-HIT并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 CLAMP 直设（149 → 131）
    let ok = p.ops[3].code == ILM_OP_CLAMP
        && p.ops[3].old_state == 149
        && p.ops[3].new_state == 131
        && p.ops[3].new_state == 131;
    s.add("9034-J2-CLAMP直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~8 内收、0 与 9 拒）
    let ok = ilm_key_ok(8) && ilm_key_ok(8) && !ilm_key_ok(0) && !ilm_key_ok(9);
    s.add("9034-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 求和辅助（值列求和（wrapping 折叠））
    let ok = ilm_sum(&[16, 5]) == 21 && ilm_sum(&[]) == 0;
    s.add("9034-J2-求和", ok, if ok { "" } else { "求和漂移" });
    // J3-1 指纹确定
    let ok = standard_ilm().ilm_fp() == standard_ilm().ilm_fp();
    s.add("9034-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_ilm();
    let fp0 = p2.ilm_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.ilm_fp() != fp0;
    s.add("9034-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_ilm();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("ILM-SEAL");
    s.add("9034-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(ILM_OP_CLAMP, 0, 0).err() == Some(ILM_SEALED);
    s.add("9034-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9034-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9034_all_checks_green() {
        let set = run_unx9034_checks();
        assert!(set.all_passed(), "UNX-F9034 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9034_state_stable() {
        let l = standard_ilm();
        assert!(l.chain_ok());
        assert_eq!(l.state, 131);
        assert_eq!(l.state.count_ones(), 3);
    }
}
