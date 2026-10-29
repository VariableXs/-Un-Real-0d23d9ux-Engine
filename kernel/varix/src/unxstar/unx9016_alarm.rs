//! UNX-F9016 alarm 兼容挂点（0x621c0 段）
//!
//! 锚点：UNX-F9016-J1 闹铃账（SET/CLEAR/FIRE 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 闹铃语义（SET 加合 / CLEAR 并合 / FIRE 直设三式运算 +
//! 键 1~32 域核 + popcount 守恒）；J3 封账印（ALM 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11。
//!
//! 结构：闹铃账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! ALM 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 alarm 兼容挂点 的语义单点：以三式账本承载 SET/CLEAR/FIRE
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x621c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x621c0;
pub const ALM_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const ALM_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 32）
pub const ALM_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const ALM_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const ALM_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// SET 式：加合（new = old.wrapping_add(val)）。
pub const ALM_OP_SET: u32 = 0;
/// CLEAR 式：并合（new = old | val）。
pub const ALM_OP_CLEAR: u32 = 1;
/// FIRE 式：直设（new = val）。
pub const ALM_OP_FIRE: u32 = 2;
/// 键域上界（1..=32）。
pub const ALM_KEY_MAX: u8 = 32;

/// op 名渲染。
pub fn alm_op_name(code: u32) -> &'static str {
    match code {
        ALM_OP_SET => "SET",
        ALM_OP_CLEAR => "CLEAR",
        ALM_OP_FIRE => "FIRE",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~32。
pub fn alm_key_ok(key: u8) -> bool {
    key != 0 && key <= ALM_KEY_MAX
}

/// 领域辅助：状态留数（模 13 折叠，账页轮转位）。
pub fn alm_mod13(state: u64) -> u32 {
    (state % 13) as u32
}

/// 单笔闹铃账。
#[derive(Clone)]
pub struct AlarmOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl AlarmOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            alm_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn alm_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        ALM_OP_SET => old.wrapping_add(val),
        ALM_OP_CLEAR => old | val,
        ALM_OP_FIRE => val,
        _ => old,
    }
}

/// 闹铃账本。
pub struct AlarmLedger {
    ops: Vec<AlarmOp>,
    state: u64,
    sealed: bool,
}

impl AlarmLedger {
    pub fn new() -> Self {
        AlarmLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔闹铃：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(ALM_SEALED);
        }
        if code > ALM_OP_FIRE {
            return Err(ALM_BAD_OP);
        }
        if !alm_key_ok(key) {
            return Err(ALM_KEY_OOB);
        }
        let old = self.state;
        let new = alm_apply(old, code, val);
        self.ops.push(AlarmOp {
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
            if op.new_state != alm_apply(op.old_state, op.code, op.val) {
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

    /// ALM 指纹：状态 + 各笔操作折叠。
    pub fn alm_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9016u32;
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
        out.push_str(&format!("ALM-STATE|{}\n", self.state));
        out.push_str(&format!("ALM-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("ALM-SEAL\n");
        }
        out
    }
}

impl Default for AlarmLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（SET[14]=10→CLEAR[5]=128→SET[15]=8→FIRE[29]=113）。
/// 手验：0→10→138→146→113。
fn standard_alm() -> AlarmLedger {
    let mut l = AlarmLedger::new();
    let _ = l.apply(ALM_OP_SET, 14, 10); // old 0   → new 10
    let _ = l.apply(ALM_OP_CLEAR, 5, 128); // old 10 → new 138
    let _ = l.apply(ALM_OP_SET, 15, 8); // old 138 → new 146
    let _ = l.apply(ALM_OP_FIRE, 29, 113); // old 146 → new 113
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = AlarmLedger::new();
    if l.apply(7, 1, 1).err() == Some(ALM_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 33 双向）
    let mut l = AlarmLedger::new();
    if l.apply(ALM_OP_SET, 0, 1).err() == Some(ALM_KEY_OOB)
        && l.apply(ALM_OP_SET, 33, 1).err() == Some(ALM_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_alm();
    let _ = l.seal();
    if l.apply(ALM_OP_SET, 1, 1).err() == Some(ALM_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = AlarmLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_alm();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 闹铃账；J2 闹铃语义；J3 封账印。
pub fn run_unx9016_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9016");
    let p = standard_alm();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9016-J1-操作全落账", ok, if ok { "" } else { "闹铃账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 10, 138, 146];
    s.add("9016-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("SET|14|old0|new10")
        && text.contains("CLEAR|5|old10|new138")
        && text.contains("SET|15|old138|new146")
        && text.contains("FIRE|29|old146|new113")
        && text.contains("ALM-STATE|113")
        && text.contains("ALM-POPC|4");
    s.add("9016-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 113
    let ok = p.state == 113;
    s.add("9016-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 SET 加合（0 + 10 = 10）
    let ok = p.ops[0].code == ALM_OP_SET
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 10
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(10);
    s.add("9016-J2-SET加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 CLEAR 并合（10 | 128 = 138）
    let ok = p.ops[1].code == ALM_OP_CLEAR
        && p.ops[1].old_state == 10
        && p.ops[1].new_state == 138
        && p.ops[1].new_state == p.ops[1].old_state | 128;
    s.add("9016-J2-CLEAR并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 FIRE 直设（146 → 113）
    let ok = p.ops[3].code == ALM_OP_FIRE
        && p.ops[3].old_state == 146
        && p.ops[3].new_state == 113
        && p.ops[3].new_state == 113;
    s.add("9016-J2-FIRE直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~32 内收、0 与 33 拒）
    let ok = alm_key_ok(14) && alm_key_ok(32) && !alm_key_ok(0) && !alm_key_ok(33);
    s.add("9016-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 留数辅助（状态留数（模 13 折叠））
    let ok = alm_mod13(146) == 3 && alm_mod13(0) == 0;
    s.add("9016-J2-留数", ok, if ok { "" } else { "留数漂移" });
    // J3-1 指纹确定
    let ok = standard_alm().alm_fp() == standard_alm().alm_fp();
    s.add("9016-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_alm();
    let fp0 = p2.alm_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.alm_fp() != fp0;
    s.add("9016-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_alm();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("ALM-SEAL");
    s.add("9016-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(ALM_OP_FIRE, 0, 0).err() == Some(ALM_SEALED);
    s.add("9016-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9016-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9016_all_checks_green() {
        let set = run_unx9016_checks();
        assert!(set.all_passed(), "UNX-F9016 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9016_state_stable() {
        let l = standard_alm();
        assert!(l.chain_ok());
        assert_eq!(l.state, 113);
        assert_eq!(l.state.count_ones(), 4);
    }
}
