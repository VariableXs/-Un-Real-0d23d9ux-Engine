//! UNX-F9041 错误矩阵抽样协议总纲（30 号抽样规则、正反判据定义）（0x624e0 段）
//!
//! 锚点：UNX-F9041-J1 抽样账（PICK/JUDGE/LOG 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 抽样语义（PICK 加合 / JUDGE 并合 / LOG 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（S30 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B13。
//!
//! 结构：抽样账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! S30 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 错误矩阵抽样协议总纲（30 号抽样规则、正反判据定义） 的语义单点：以三式账本承载 PICK/JUDGE/LOG
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x624e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x624e0;
pub const S30_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const S30_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const S30_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const S30_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const S30_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// PICK 式：加合（new = old.wrapping_add(val)）。
pub const S30_OP_PICK: u32 = 0;
/// JUDGE 式：并合（new = old | val）。
pub const S30_OP_JUDGE: u32 = 1;
/// LOG 式：直设（new = val）。
pub const S30_OP_LOG: u32 = 2;
/// 键域上界（1..=16）。
pub const S30_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn s30_op_name(code: u32) -> &'static str {
    match code {
        S30_OP_PICK => "PICK",
        S30_OP_JUDGE => "JUDGE",
        S30_OP_LOG => "LOG",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn s30_key_ok(key: u8) -> bool {
    key != 0 && key <= S30_KEY_MAX
}

/// 领域辅助：状态高位摘要（>> 4 截影，指纹前置压缩）。
pub fn s30_hi4(state: u64) -> u32 {
    (state >> 4) as u32
}

/// 单笔抽样账。
#[derive(Clone)]
pub struct SampleOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl SampleOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            s30_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn s30_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        S30_OP_PICK => old.wrapping_add(val),
        S30_OP_JUDGE => old | val,
        S30_OP_LOG => val,
        _ => old,
    }
}

/// 抽样账本。
pub struct SampleLedger {
    ops: Vec<SampleOp>,
    state: u64,
    sealed: bool,
}

impl SampleLedger {
    pub fn new() -> Self {
        SampleLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔抽样：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(S30_SEALED);
        }
        if code > S30_OP_LOG {
            return Err(S30_BAD_OP);
        }
        if !s30_key_ok(key) {
            return Err(S30_KEY_OOB);
        }
        let old = self.state;
        let new = s30_apply(old, code, val);
        self.ops.push(SampleOp {
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
            if op.new_state != s30_apply(op.old_state, op.code, op.val) {
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

    /// S30 指纹：状态 + 各笔操作折叠。
    pub fn s30_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9041u32;
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
        out.push_str(&format!("S30-STATE|{}\n", self.state));
        out.push_str(&format!("S30-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("S30-SEAL\n");
        }
        out
    }
}

impl Default for SampleLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（PICK[7]=11→JUDGE[2]=256→PICK[10]=5→LOG[12]=138）。
/// 手验：0→11→267→272→138。
fn standard_s30() -> SampleLedger {
    let mut l = SampleLedger::new();
    let _ = l.apply(S30_OP_PICK, 7, 11); // old 0   → new 11
    let _ = l.apply(S30_OP_JUDGE, 2, 256); // old 11 → new 267
    let _ = l.apply(S30_OP_PICK, 10, 5); // old 267 → new 272
    let _ = l.apply(S30_OP_LOG, 12, 138); // old 272 → new 138
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = SampleLedger::new();
    if l.apply(7, 1, 1).err() == Some(S30_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = SampleLedger::new();
    if l.apply(S30_OP_PICK, 0, 1).err() == Some(S30_KEY_OOB)
        && l.apply(S30_OP_PICK, 17, 1).err() == Some(S30_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_s30();
    let _ = l.seal();
    if l.apply(S30_OP_PICK, 1, 1).err() == Some(S30_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = SampleLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_s30();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 抽样账；J2 抽样语义；J3 封账印。
pub fn run_unx9041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9041");
    let p = standard_s30();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9041-J1-操作全落账", ok, if ok { "" } else { "抽样账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 11, 267, 272];
    s.add("9041-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("PICK|7|old0|new11")
        && text.contains("JUDGE|2|old11|new267")
        && text.contains("PICK|10|old267|new272")
        && text.contains("LOG|12|old272|new138")
        && text.contains("S30-STATE|138")
        && text.contains("S30-POPC|3");
    s.add("9041-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 138
    let ok = p.state == 138;
    s.add("9041-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 PICK 加合（0 + 11 = 11）
    let ok = p.ops[0].code == S30_OP_PICK
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 11
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(11);
    s.add("9041-J2-PICK加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 JUDGE 并合（11 | 256 = 267）
    let ok = p.ops[1].code == S30_OP_JUDGE
        && p.ops[1].old_state == 11
        && p.ops[1].new_state == 267
        && p.ops[1].new_state == p.ops[1].old_state | 256;
    s.add("9041-J2-JUDGE并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 LOG 直设（272 → 138）
    let ok = p.ops[3].code == S30_OP_LOG
        && p.ops[3].old_state == 272
        && p.ops[3].new_state == 138
        && p.ops[3].new_state == 138;
    s.add("9041-J2-LOG直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = s30_key_ok(7) && s30_key_ok(16) && !s30_key_ok(0) && !s30_key_ok(17);
    s.add("9041-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 高位辅助（状态高位摘要（>> 4 截影））
    let ok = s30_hi4(267) == 16 && s30_hi4(0) == 0;
    s.add("9041-J2-高位", ok, if ok { "" } else { "高位漂移" });
    // J3-1 指纹确定
    let ok = standard_s30().s30_fp() == standard_s30().s30_fp();
    s.add("9041-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_s30();
    let fp0 = p2.s30_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.s30_fp() != fp0;
    s.add("9041-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_s30();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("S30-SEAL");
    s.add("9041-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(S30_OP_LOG, 0, 0).err() == Some(S30_SEALED);
    s.add("9041-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9041-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9041_all_checks_green() {
        let set = run_unx9041_checks();
        assert!(set.all_passed(), "UNX-F9041 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9041_state_stable() {
        let l = standard_s30();
        assert!(l.chain_ok());
        assert_eq!(l.state, 138);
        assert_eq!(l.state.count_ones(), 3);
    }
}
