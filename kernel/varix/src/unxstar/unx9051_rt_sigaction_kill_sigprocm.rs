//! UNX-F9051 抽样组十：信号族正反双判据（rt_sigaction/kill/sigprocmask）（0x62620 段）
//!
//! 锚点：UNX-F9051-J1 信号账（ACTION/KILL/MASK 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 信号语义（ACTION 加合 / KILL 并合 / MASK 直设三式运算 +
//! 键 1~64 域核 + popcount 守恒）；J3 封账印（SGP 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B13。
//!
//! 结构：信号账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! SGP 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 抽样组十：信号族正反双判据（rt_sigaction/kill/sigprocmask） 的语义单点：以三式账本承载 ACTION/KILL/MASK
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62620 独占 0x20）。
pub const SEG_BASE: u32 = 0x62620;
pub const SGP_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const SGP_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 64）
pub const SGP_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const SGP_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const SGP_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// ACTION 式：加合（new = old.wrapping_add(val)）。
pub const SGP_OP_ACTION: u32 = 0;
/// KILL 式：并合（new = old | val）。
pub const SGP_OP_KILL: u32 = 1;
/// MASK 式：直设（new = val）。
pub const SGP_OP_MASK: u32 = 2;
/// 键域上界（1..=64）。
pub const SGP_KEY_MAX: u8 = 64;

/// op 名渲染。
pub fn sgp_op_name(code: u32) -> &'static str {
    match code {
        SGP_OP_ACTION => "ACTION",
        SGP_OP_KILL => "KILL",
        SGP_OP_MASK => "MASK",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~64。
pub fn sgp_key_ok(key: u8) -> bool {
    key != 0 && key <= SGP_KEY_MAX
}

/// 领域辅助：键 → 分桶（每 4 键一桶，1..=64 共 16 桶）。
pub fn sgp_bucket(key: u8) -> u32 {
    ((key as u32 - 1) / 4) + 1
}

/// 单笔信号账。
#[derive(Clone)]
pub struct SigSampleOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl SigSampleOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            sgp_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn sgp_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        SGP_OP_ACTION => old.wrapping_add(val),
        SGP_OP_KILL => old | val,
        SGP_OP_MASK => val,
        _ => old,
    }
}

/// 信号账本。
pub struct SigSampleLedger {
    ops: Vec<SigSampleOp>,
    state: u64,
    sealed: bool,
}

impl SigSampleLedger {
    pub fn new() -> Self {
        SigSampleLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔信号：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(SGP_SEALED);
        }
        if code > SGP_OP_MASK {
            return Err(SGP_BAD_OP);
        }
        if !sgp_key_ok(key) {
            return Err(SGP_KEY_OOB);
        }
        let old = self.state;
        let new = sgp_apply(old, code, val);
        self.ops.push(SigSampleOp {
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
            if op.new_state != sgp_apply(op.old_state, op.code, op.val) {
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

    /// SGP 指纹：状态 + 各笔操作折叠。
    pub fn sgp_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9051u32;
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
        out.push_str(&format!("SGP-STATE|{}\n", self.state));
        out.push_str(&format!("SGP-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("SGP-SEAL\n");
        }
        out
    }
}

impl Default for SigSampleLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（ACTION[49]=9→KILL[52]=64→ACTION[24]=8→MASK[18]=148）。
/// 手验：0→9→73→81→148。
fn standard_sgp() -> SigSampleLedger {
    let mut l = SigSampleLedger::new();
    let _ = l.apply(SGP_OP_ACTION, 49, 9); // old 0   → new 9
    let _ = l.apply(SGP_OP_KILL, 52, 64); // old 9 → new 73
    let _ = l.apply(SGP_OP_ACTION, 24, 8); // old 73 → new 81
    let _ = l.apply(SGP_OP_MASK, 18, 148); // old 81 → new 148
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = SigSampleLedger::new();
    if l.apply(7, 1, 1).err() == Some(SGP_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 65 双向）
    let mut l = SigSampleLedger::new();
    if l.apply(SGP_OP_ACTION, 0, 1).err() == Some(SGP_KEY_OOB)
        && l.apply(SGP_OP_ACTION, 65, 1).err() == Some(SGP_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_sgp();
    let _ = l.seal();
    if l.apply(SGP_OP_ACTION, 1, 1).err() == Some(SGP_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = SigSampleLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_sgp();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 信号账；J2 信号语义；J3 封账印。
pub fn run_unx9051_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9051");
    let p = standard_sgp();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9051-J1-操作全落账", ok, if ok { "" } else { "信号账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 9, 73, 81];
    s.add("9051-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("ACTION|49|old0|new9")
        && text.contains("KILL|52|old9|new73")
        && text.contains("ACTION|24|old73|new81")
        && text.contains("MASK|18|old81|new148")
        && text.contains("SGP-STATE|148")
        && text.contains("SGP-POPC|3");
    s.add("9051-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 148
    let ok = p.state == 148;
    s.add("9051-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 ACTION 加合（0 + 9 = 9）
    let ok = p.ops[0].code == SGP_OP_ACTION
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 9
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(9);
    s.add("9051-J2-ACTION加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 KILL 并合（9 | 64 = 73）
    let ok = p.ops[1].code == SGP_OP_KILL
        && p.ops[1].old_state == 9
        && p.ops[1].new_state == 73
        && p.ops[1].new_state == p.ops[1].old_state | 64;
    s.add("9051-J2-KILL并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 MASK 直设（81 → 148）
    let ok = p.ops[3].code == SGP_OP_MASK
        && p.ops[3].old_state == 81
        && p.ops[3].new_state == 148
        && p.ops[3].new_state == 148;
    s.add("9051-J2-MASK直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~64 内收、0 与 65 拒）
    let ok = sgp_key_ok(49) && sgp_key_ok(64) && !sgp_key_ok(0) && !sgp_key_ok(65);
    s.add("9051-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 分桶辅助（键 → 分桶（每 4 键一桶））
    let ok = sgp_bucket(49) == 13 && sgp_bucket(64) == 16;
    s.add("9051-J2-分桶", ok, if ok { "" } else { "分桶漂移" });
    // J3-1 指纹确定
    let ok = standard_sgp().sgp_fp() == standard_sgp().sgp_fp();
    s.add("9051-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_sgp();
    let fp0 = p2.sgp_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.sgp_fp() != fp0;
    s.add("9051-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_sgp();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("SGP-SEAL");
    s.add("9051-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(SGP_OP_MASK, 0, 0).err() == Some(SGP_SEALED);
    s.add("9051-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9051-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9051_all_checks_green() {
        let set = run_unx9051_checks();
        assert!(set.all_passed(), "UNX-F9051 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9051_state_stable() {
        let l = standard_sgp();
        assert!(l.chain_ok());
        assert_eq!(l.state, 148);
        assert_eq!(l.state.count_ones(), 3);
    }
}
