//! UNX-F9011 sigpending/sigtimedwait/sigwaitinfo（0x62120 段）
//!
//! 锚点：UNX-F9011-J1 待收账（PEND/WAIT/POLL 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 待收语义（PEND 加合 / WAIT 并合 / POLL 直设三式运算 +
//! 键 1~64 域核 + popcount 守恒）；J3 封账印（PND 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11。
//!
//! 结构：待收账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! PND 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 sigpending/sigtimedwait/sigwaitinfo 的语义单点：以三式账本承载 PEND/WAIT/POLL
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62120 独占 0x20）。
pub const SEG_BASE: u32 = 0x62120;
pub const PND_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const PND_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 64）
pub const PND_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const PND_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const PND_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// PEND 式：加合（new = old.wrapping_add(val)）。
pub const PND_OP_PEND: u32 = 0;
/// WAIT 式：并合（new = old | val）。
pub const PND_OP_WAIT: u32 = 1;
/// POLL 式：直设（new = val）。
pub const PND_OP_POLL: u32 = 2;
/// 键域上界（1..=64）。
pub const PND_KEY_MAX: u8 = 64;

/// op 名渲染。
pub fn pnd_op_name(code: u32) -> &'static str {
    match code {
        PND_OP_PEND => "PEND",
        PND_OP_WAIT => "WAIT",
        PND_OP_POLL => "POLL",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~64。
pub fn pnd_key_ok(key: u8) -> bool {
    key != 0 && key <= PND_KEY_MAX
}

/// 领域辅助：键 → 分桶（每 4 键一桶，1..=64 共 16 桶）。
pub fn pnd_bucket(key: u8) -> u32 {
    ((key as u32 - 1) / 4) + 1
}

/// 单笔待收账。
#[derive(Clone)]
pub struct PendingOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl PendingOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            pnd_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn pnd_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        PND_OP_PEND => old.wrapping_add(val),
        PND_OP_WAIT => old | val,
        PND_OP_POLL => val,
        _ => old,
    }
}

/// 待收账本。
pub struct PendingLedger {
    ops: Vec<PendingOp>,
    state: u64,
    sealed: bool,
}

impl PendingLedger {
    pub fn new() -> Self {
        PendingLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔待收：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(PND_SEALED);
        }
        if code > PND_OP_POLL {
            return Err(PND_BAD_OP);
        }
        if !pnd_key_ok(key) {
            return Err(PND_KEY_OOB);
        }
        let old = self.state;
        let new = pnd_apply(old, code, val);
        self.ops.push(PendingOp {
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
            if op.new_state != pnd_apply(op.old_state, op.code, op.val) {
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

    /// PND 指纹：状态 + 各笔操作折叠。
    pub fn pnd_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9011u32;
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
        out.push_str(&format!("PND-STATE|{}\n", self.state));
        out.push_str(&format!("PND-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("PND-SEAL\n");
        }
        out
    }
}

impl Default for PendingLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（PEND[9]=17→WAIT[44]=256→PEND[32]=3→POLL[58]=108）。
/// 手验：0→17→273→276→108。
fn standard_pnd() -> PendingLedger {
    let mut l = PendingLedger::new();
    let _ = l.apply(PND_OP_PEND, 9, 17); // old 0   → new 17
    let _ = l.apply(PND_OP_WAIT, 44, 256); // old 17 → new 273
    let _ = l.apply(PND_OP_PEND, 32, 3); // old 273 → new 276
    let _ = l.apply(PND_OP_POLL, 58, 108); // old 276 → new 108
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = PendingLedger::new();
    if l.apply(7, 1, 1).err() == Some(PND_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 65 双向）
    let mut l = PendingLedger::new();
    if l.apply(PND_OP_PEND, 0, 1).err() == Some(PND_KEY_OOB)
        && l.apply(PND_OP_PEND, 65, 1).err() == Some(PND_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_pnd();
    let _ = l.seal();
    if l.apply(PND_OP_PEND, 1, 1).err() == Some(PND_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = PendingLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_pnd();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 待收账；J2 待收语义；J3 封账印。
pub fn run_unx9011_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9011");
    let p = standard_pnd();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9011-J1-操作全落账", ok, if ok { "" } else { "待收账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 17, 273, 276];
    s.add("9011-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("PEND|9|old0|new17")
        && text.contains("WAIT|44|old17|new273")
        && text.contains("PEND|32|old273|new276")
        && text.contains("POLL|58|old276|new108")
        && text.contains("PND-STATE|108")
        && text.contains("PND-POPC|4");
    s.add("9011-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 108
    let ok = p.state == 108;
    s.add("9011-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 PEND 加合（0 + 17 = 17）
    let ok = p.ops[0].code == PND_OP_PEND
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 17
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(17);
    s.add("9011-J2-PEND加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 WAIT 并合（17 | 256 = 273）
    let ok = p.ops[1].code == PND_OP_WAIT
        && p.ops[1].old_state == 17
        && p.ops[1].new_state == 273
        && p.ops[1].new_state == p.ops[1].old_state | 256;
    s.add("9011-J2-WAIT并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 POLL 直设（276 → 108）
    let ok = p.ops[3].code == PND_OP_POLL
        && p.ops[3].old_state == 276
        && p.ops[3].new_state == 108
        && p.ops[3].new_state == 108;
    s.add("9011-J2-POLL直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~64 内收、0 与 65 拒）
    let ok = pnd_key_ok(9) && pnd_key_ok(64) && !pnd_key_ok(0) && !pnd_key_ok(65);
    s.add("9011-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 分桶辅助（键 → 分桶（每 4 键一桶））
    let ok = pnd_bucket(9) == 3 && pnd_bucket(64) == 16;
    s.add("9011-J2-分桶", ok, if ok { "" } else { "分桶漂移" });
    // J3-1 指纹确定
    let ok = standard_pnd().pnd_fp() == standard_pnd().pnd_fp();
    s.add("9011-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_pnd();
    let fp0 = p2.pnd_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.pnd_fp() != fp0;
    s.add("9011-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_pnd();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("PND-SEAL");
    s.add("9011-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(PND_OP_POLL, 0, 0).err() == Some(PND_SEALED);
    s.add("9011-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9011-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9011_all_checks_green() {
        let set = run_unx9011_checks();
        assert!(set.all_passed(), "UNX-F9011 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9011_state_stable() {
        let l = standard_pnd();
        assert!(l.chain_ok());
        assert_eq!(l.state, 108);
        assert_eq!(l.state.count_ones(), 4);
    }
}
