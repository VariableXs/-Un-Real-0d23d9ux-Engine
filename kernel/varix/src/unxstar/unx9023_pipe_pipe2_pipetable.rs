//! UNX-F9023 pipe/pipe2 本体档（PipeTable 对齐）（0x622a0 段）
//!
//! 锚点：UNX-F9023-J1 管道账（MKPIPE/OPEN/CLOSE 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 管道语义（MKPIPE 加合 / OPEN 并合 / CLOSE 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（PIP 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B12。
//!
//! 结构：管道账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! PIP 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 pipe/pipe2 本体档（PipeTable 对齐） 的语义单点：以三式账本承载 MKPIPE/OPEN/CLOSE
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x622a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x622a0;
pub const PIP_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const PIP_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const PIP_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const PIP_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const PIP_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// MKPIPE 式：加合（new = old.wrapping_add(val)）。
pub const PIP_OP_MKPIPE: u32 = 0;
/// OPEN 式：并合（new = old | val）。
pub const PIP_OP_OPEN: u32 = 1;
/// CLOSE 式：直设（new = val）。
pub const PIP_OP_CLOSE: u32 = 2;
/// 键域上界（1..=16）。
pub const PIP_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn pip_op_name(code: u32) -> &'static str {
    match code {
        PIP_OP_MKPIPE => "MKPIPE",
        PIP_OP_OPEN => "OPEN",
        PIP_OP_CLOSE => "CLOSE",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn pip_key_ok(key: u8) -> bool {
    key != 0 && key <= PIP_KEY_MAX
}

/// 领域辅助：幂 2 判定（0 视为非幂 2）。
pub fn pip_is_pow2(val: u64) -> bool {
    val != 0 && val & (val - 1) == 0
}

/// 单笔管道账。
#[derive(Clone)]
pub struct PipeOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl PipeOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            pip_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn pip_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        PIP_OP_MKPIPE => old.wrapping_add(val),
        PIP_OP_OPEN => old | val,
        PIP_OP_CLOSE => val,
        _ => old,
    }
}

/// 管道账本。
pub struct PipeLedger {
    ops: Vec<PipeOp>,
    state: u64,
    sealed: bool,
}

impl PipeLedger {
    pub fn new() -> Self {
        PipeLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔管道：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(PIP_SEALED);
        }
        if code > PIP_OP_CLOSE {
            return Err(PIP_BAD_OP);
        }
        if !pip_key_ok(key) {
            return Err(PIP_KEY_OOB);
        }
        let old = self.state;
        let new = pip_apply(old, code, val);
        self.ops.push(PipeOp {
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
            if op.new_state != pip_apply(op.old_state, op.code, op.val) {
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

    /// PIP 指纹：状态 + 各笔操作折叠。
    pub fn pip_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9023u32;
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
        out.push_str(&format!("PIP-STATE|{}\n", self.state));
        out.push_str(&format!("PIP-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("PIP-SEAL\n");
        }
        out
    }
}

impl Default for PipeLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（MKPIPE[5]=17→OPEN[8]=256→MKPIPE[4]=8→CLOSE[14]=120）。
/// 手验：0→17→273→281→120。
fn standard_pip() -> PipeLedger {
    let mut l = PipeLedger::new();
    let _ = l.apply(PIP_OP_MKPIPE, 5, 17); // old 0   → new 17
    let _ = l.apply(PIP_OP_OPEN, 8, 256); // old 17 → new 273
    let _ = l.apply(PIP_OP_MKPIPE, 4, 8); // old 273 → new 281
    let _ = l.apply(PIP_OP_CLOSE, 14, 120); // old 281 → new 120
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = PipeLedger::new();
    if l.apply(7, 1, 1).err() == Some(PIP_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = PipeLedger::new();
    if l.apply(PIP_OP_MKPIPE, 0, 1).err() == Some(PIP_KEY_OOB)
        && l.apply(PIP_OP_MKPIPE, 17, 1).err() == Some(PIP_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_pip();
    let _ = l.seal();
    if l.apply(PIP_OP_MKPIPE, 1, 1).err() == Some(PIP_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = PipeLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_pip();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 管道账；J2 管道语义；J3 封账印。
pub fn run_unx9023_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9023");
    let p = standard_pip();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9023-J1-操作全落账", ok, if ok { "" } else { "管道账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 17, 273, 281];
    s.add("9023-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("MKPIPE|5|old0|new17")
        && text.contains("OPEN|8|old17|new273")
        && text.contains("MKPIPE|4|old273|new281")
        && text.contains("CLOSE|14|old281|new120")
        && text.contains("PIP-STATE|120")
        && text.contains("PIP-POPC|4");
    s.add("9023-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 120
    let ok = p.state == 120;
    s.add("9023-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 MKPIPE 加合（0 + 17 = 17）
    let ok = p.ops[0].code == PIP_OP_MKPIPE
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 17
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(17);
    s.add("9023-J2-MKPIPE加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 OPEN 并合（17 | 256 = 273）
    let ok = p.ops[1].code == PIP_OP_OPEN
        && p.ops[1].old_state == 17
        && p.ops[1].new_state == 273
        && p.ops[1].new_state == p.ops[1].old_state | 256;
    s.add("9023-J2-OPEN并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 CLOSE 直设（281 → 120）
    let ok = p.ops[3].code == PIP_OP_CLOSE
        && p.ops[3].old_state == 281
        && p.ops[3].new_state == 120
        && p.ops[3].new_state == 120;
    s.add("9023-J2-CLOSE直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = pip_key_ok(5) && pip_key_ok(16) && !pip_key_ok(0) && !pip_key_ok(17);
    s.add("9023-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 幂二辅助（幂 2 判定（0 视为非幂 2））
    let ok = pip_is_pow2(256) && pip_is_pow2(17) == false && !pip_is_pow2(0);
    s.add("9023-J2-幂二", ok, if ok { "" } else { "幂二漂移" });
    // J3-1 指纹确定
    let ok = standard_pip().pip_fp() == standard_pip().pip_fp();
    s.add("9023-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_pip();
    let fp0 = p2.pip_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.pip_fp() != fp0;
    s.add("9023-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_pip();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("PIP-SEAL");
    s.add("9023-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(PIP_OP_CLOSE, 0, 0).err() == Some(PIP_SEALED);
    s.add("9023-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9023-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9023_all_checks_green() {
        let set = run_unx9023_checks();
        assert!(set.all_passed(), "UNX-F9023 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9023_state_stable() {
        let l = standard_pip();
        assert!(l.chain_ok());
        assert_eq!(l.state, 120);
        assert_eq!(l.state.count_ones(), 4);
    }
}
