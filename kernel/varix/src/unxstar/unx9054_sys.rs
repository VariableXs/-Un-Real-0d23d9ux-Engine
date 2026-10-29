//! UNX-F9054 反判据方法论（非法输入→期望错误码，边界值登记）（0x62680 段）
//!
//! 锚点：UNX-F9054-J1 反判账（INVAL/EXPECT/VERD 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 反判语义（INVAL 加合 / EXPECT 并合 / VERD 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（NJU 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B13。
//!
//! 结构：反判账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! NJU 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 反判据方法论（非法输入→期望错误码，边界值登记） 的语义单点：以三式账本承载 INVAL/EXPECT/VERD
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62680 独占 0x20）。
pub const SEG_BASE: u32 = 0x62680;
pub const NJU_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const NJU_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const NJU_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const NJU_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const NJU_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// INVAL 式：加合（new = old.wrapping_add(val)）。
pub const NJU_OP_INVAL: u32 = 0;
/// EXPECT 式：并合（new = old | val）。
pub const NJU_OP_EXPECT: u32 = 1;
/// VERD 式：直设（new = val）。
pub const NJU_OP_VERD: u32 = 2;
/// 键域上界（1..=16）。
pub const NJU_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn nju_op_name(code: u32) -> &'static str {
    match code {
        NJU_OP_INVAL => "INVAL",
        NJU_OP_EXPECT => "EXPECT",
        NJU_OP_VERD => "VERD",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn nju_key_ok(key: u8) -> bool {
    key != 0 && key <= NJU_KEY_MAX
}

/// 领域辅助：两态跨度（饱和差，b < a 时归零）。
pub fn nju_span(a: u64, b: u64) -> u64 {
    b.saturating_sub(a)
}

/// 单笔反判账。
#[derive(Clone)]
pub struct NegJudgeOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl NegJudgeOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            nju_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn nju_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        NJU_OP_INVAL => old.wrapping_add(val),
        NJU_OP_EXPECT => old | val,
        NJU_OP_VERD => val,
        _ => old,
    }
}

/// 反判账本。
pub struct NegJudgeLedger {
    ops: Vec<NegJudgeOp>,
    state: u64,
    sealed: bool,
}

impl NegJudgeLedger {
    pub fn new() -> Self {
        NegJudgeLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔反判：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(NJU_SEALED);
        }
        if code > NJU_OP_VERD {
            return Err(NJU_BAD_OP);
        }
        if !nju_key_ok(key) {
            return Err(NJU_KEY_OOB);
        }
        let old = self.state;
        let new = nju_apply(old, code, val);
        self.ops.push(NegJudgeOp {
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
            if op.new_state != nju_apply(op.old_state, op.code, op.val) {
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

    /// NJU 指纹：状态 + 各笔操作折叠。
    pub fn nju_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9054u32;
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
        out.push_str(&format!("NJU-STATE|{}\n", self.state));
        out.push_str(&format!("NJU-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("NJU-SEAL\n");
        }
        out
    }
}

impl Default for NegJudgeLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（INVAL[4]=12→EXPECT[3]=512→INVAL[1]=4→VERD[7]=151）。
/// 手验：0→12→524→528→151。
fn standard_nju() -> NegJudgeLedger {
    let mut l = NegJudgeLedger::new();
    let _ = l.apply(NJU_OP_INVAL, 4, 12); // old 0   → new 12
    let _ = l.apply(NJU_OP_EXPECT, 3, 512); // old 12 → new 524
    let _ = l.apply(NJU_OP_INVAL, 1, 4); // old 524 → new 528
    let _ = l.apply(NJU_OP_VERD, 7, 151); // old 528 → new 151
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = NegJudgeLedger::new();
    if l.apply(7, 1, 1).err() == Some(NJU_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = NegJudgeLedger::new();
    if l.apply(NJU_OP_INVAL, 0, 1).err() == Some(NJU_KEY_OOB)
        && l.apply(NJU_OP_INVAL, 17, 1).err() == Some(NJU_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_nju();
    let _ = l.seal();
    if l.apply(NJU_OP_INVAL, 1, 1).err() == Some(NJU_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = NegJudgeLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_nju();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 反判账；J2 反判语义；J3 封账印。
pub fn run_unx9054_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9054");
    let p = standard_nju();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9054-J1-操作全落账", ok, if ok { "" } else { "反判账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 12, 524, 528];
    s.add("9054-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("INVAL|4|old0|new12")
        && text.contains("EXPECT|3|old12|new524")
        && text.contains("INVAL|1|old524|new528")
        && text.contains("VERD|7|old528|new151")
        && text.contains("NJU-STATE|151")
        && text.contains("NJU-POPC|5");
    s.add("9054-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 151
    let ok = p.state == 151;
    s.add("9054-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 INVAL 加合（0 + 12 = 12）
    let ok = p.ops[0].code == NJU_OP_INVAL
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 12
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(12);
    s.add("9054-J2-INVAL加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 EXPECT 并合（12 | 512 = 524）
    let ok = p.ops[1].code == NJU_OP_EXPECT
        && p.ops[1].old_state == 12
        && p.ops[1].new_state == 524
        && p.ops[1].new_state == p.ops[1].old_state | 512;
    s.add("9054-J2-EXPECT并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 VERD 直设（528 → 151）
    let ok = p.ops[3].code == NJU_OP_VERD
        && p.ops[3].old_state == 528
        && p.ops[3].new_state == 151
        && p.ops[3].new_state == 151;
    s.add("9054-J2-VERD直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = nju_key_ok(4) && nju_key_ok(16) && !nju_key_ok(0) && !nju_key_ok(17);
    s.add("9054-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 跨度辅助（两态跨度（饱和差））
    let ok = nju_span(0, 12) == 12 && nju_span(12, 12) == 0;
    s.add("9054-J2-跨度", ok, if ok { "" } else { "跨度漂移" });
    // J3-1 指纹确定
    let ok = standard_nju().nju_fp() == standard_nju().nju_fp();
    s.add("9054-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_nju();
    let fp0 = p2.nju_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.nju_fp() != fp0;
    s.add("9054-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_nju();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("NJU-SEAL");
    s.add("9054-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(NJU_OP_VERD, 0, 0).err() == Some(NJU_SEALED);
    s.add("9054-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9054-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9054_all_checks_green() {
        let set = run_unx9054_checks();
        assert!(set.all_passed(), "UNX-F9054 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9054_state_stable() {
        let l = standard_nju();
        assert!(l.chain_ok());
        assert_eq!(l.state, 151);
        assert_eq!(l.state.count_ones(), 5);
    }
}
