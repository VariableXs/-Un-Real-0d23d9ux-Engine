//! UNX-F9037 管道与多路复用联动（pipe→epoll 消费复测）（0x62460 段）
//!
//! 锚点：UNX-F9037-J1 通流账（REG/READY/FIRE 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 通流语义（REG 加合 / READY 并合 / FIRE 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（PEP 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B12。
//!
//! 结构：通流账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! PEP 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 管道与多路复用联动（pipe→epoll 消费复测） 的语义单点：以三式账本承载 REG/READY/FIRE
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62460 独占 0x20）。
pub const SEG_BASE: u32 = 0x62460;
pub const PEP_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const PEP_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const PEP_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const PEP_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const PEP_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// REG 式：加合（new = old.wrapping_add(val)）。
pub const PEP_OP_REG: u32 = 0;
/// READY 式：并合（new = old | val）。
pub const PEP_OP_READY: u32 = 1;
/// FIRE 式：直设（new = val）。
pub const PEP_OP_FIRE: u32 = 2;
/// 键域上界（1..=16）。
pub const PEP_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn pep_op_name(code: u32) -> &'static str {
    match code {
        PEP_OP_REG => "REG",
        PEP_OP_READY => "READY",
        PEP_OP_FIRE => "FIRE",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn pep_key_ok(key: u8) -> bool {
    key != 0 && key <= PEP_KEY_MAX
}

/// 领域辅助：状态奇偶（popcount 模 2，账面校验位）。
pub fn pep_parity(state: u64) -> u32 {
    state.count_ones() % 2
}

/// 单笔通流账。
#[derive(Clone)]
pub struct PipeEpollOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl PipeEpollOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            pep_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn pep_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        PEP_OP_REG => old.wrapping_add(val),
        PEP_OP_READY => old | val,
        PEP_OP_FIRE => val,
        _ => old,
    }
}

/// 通流账本。
pub struct PipeEpollLedger {
    ops: Vec<PipeEpollOp>,
    state: u64,
    sealed: bool,
}

impl PipeEpollLedger {
    pub fn new() -> Self {
        PipeEpollLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔通流：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(PEP_SEALED);
        }
        if code > PEP_OP_FIRE {
            return Err(PEP_BAD_OP);
        }
        if !pep_key_ok(key) {
            return Err(PEP_KEY_OOB);
        }
        let old = self.state;
        let new = pep_apply(old, code, val);
        self.ops.push(PipeEpollOp {
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
            if op.new_state != pep_apply(op.old_state, op.code, op.val) {
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

    /// PEP 指纹：状态 + 各笔操作折叠。
    pub fn pep_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9037u32;
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
        out.push_str(&format!("PEP-STATE|{}\n", self.state));
        out.push_str(&format!("PEP-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("PEP-SEAL\n");
        }
        out
    }
}

impl Default for PipeEpollLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（REG[3]=19→READY[14]=1024→REG[14]=8→FIRE[16]=134）。
/// 手验：0→19→1043→1051→134。
fn standard_pep() -> PipeEpollLedger {
    let mut l = PipeEpollLedger::new();
    let _ = l.apply(PEP_OP_REG, 3, 19); // old 0   → new 19
    let _ = l.apply(PEP_OP_READY, 14, 1024); // old 19 → new 1043
    let _ = l.apply(PEP_OP_REG, 14, 8); // old 1043 → new 1051
    let _ = l.apply(PEP_OP_FIRE, 16, 134); // old 1051 → new 134
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = PipeEpollLedger::new();
    if l.apply(7, 1, 1).err() == Some(PEP_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = PipeEpollLedger::new();
    if l.apply(PEP_OP_REG, 0, 1).err() == Some(PEP_KEY_OOB)
        && l.apply(PEP_OP_REG, 17, 1).err() == Some(PEP_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_pep();
    let _ = l.seal();
    if l.apply(PEP_OP_REG, 1, 1).err() == Some(PEP_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = PipeEpollLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_pep();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 通流账；J2 通流语义；J3 封账印。
pub fn run_unx9037_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9037");
    let p = standard_pep();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9037-J1-操作全落账", ok, if ok { "" } else { "通流账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 19, 1043, 1051];
    s.add("9037-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("REG|3|old0|new19")
        && text.contains("READY|14|old19|new1043")
        && text.contains("REG|14|old1043|new1051")
        && text.contains("FIRE|16|old1051|new134")
        && text.contains("PEP-STATE|134")
        && text.contains("PEP-POPC|3");
    s.add("9037-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 134
    let ok = p.state == 134;
    s.add("9037-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 REG 加合（0 + 19 = 19）
    let ok = p.ops[0].code == PEP_OP_REG
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 19
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(19);
    s.add("9037-J2-REG加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 READY 并合（19 | 1024 = 1043）
    let ok = p.ops[1].code == PEP_OP_READY
        && p.ops[1].old_state == 19
        && p.ops[1].new_state == 1043
        && p.ops[1].new_state == p.ops[1].old_state | 1024;
    s.add("9037-J2-READY并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 FIRE 直设（1051 → 134）
    let ok = p.ops[3].code == PEP_OP_FIRE
        && p.ops[3].old_state == 1051
        && p.ops[3].new_state == 134
        && p.ops[3].new_state == 134;
    s.add("9037-J2-FIRE直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = pep_key_ok(3) && pep_key_ok(16) && !pep_key_ok(0) && !pep_key_ok(17);
    s.add("9037-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 奇偶辅助（状态奇偶（popcount 模 2 校验面））
    let ok = pep_parity(1043) == 0 && pep_parity(0) == 0;
    s.add("9037-J2-奇偶", ok, if ok { "" } else { "奇偶漂移" });
    // J3-1 指纹确定
    let ok = standard_pep().pep_fp() == standard_pep().pep_fp();
    s.add("9037-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_pep();
    let fp0 = p2.pep_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.pep_fp() != fp0;
    s.add("9037-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_pep();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("PEP-SEAL");
    s.add("9037-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(PEP_OP_FIRE, 0, 0).err() == Some(PEP_SEALED);
    s.add("9037-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9037-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9037_all_checks_green() {
        let set = run_unx9037_checks();
        assert!(set.all_passed(), "UNX-F9037 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9037_state_stable() {
        let l = standard_pep();
        assert!(l.chain_ok());
        assert_eq!(l.state, 134);
        assert_eq!(l.state.count_ones(), 3);
    }
}
