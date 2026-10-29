//! UNX-F9017 信号错误矩阵（EINVAL/ESRCH/EPERM/EBADF）（0x621e0 段）
//!
//! 锚点：UNX-F9017-J1 错误账（EINVAL/ESRCH/EPERM 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 错误语义（EINVAL 加合 / ESRCH 并合 / EPERM 直设三式运算 +
//! 键 1~32 域核 + popcount 守恒）；J3 封账印（ERM 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11。
//!
//! 结构：错误账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! ERM 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 信号错误矩阵（EINVAL/ESRCH/EPERM/EBADF） 的语义单点：以三式账本承载 EINVAL/ESRCH/EPERM
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x621e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x621e0;
pub const ERM_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const ERM_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 32）
pub const ERM_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const ERM_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const ERM_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// EINVAL 式：加合（new = old.wrapping_add(val)）。
pub const ERM_OP_EINVAL: u32 = 0;
/// ESRCH 式：并合（new = old | val）。
pub const ERM_OP_ESRCH: u32 = 1;
/// EPERM 式：直设（new = val）。
pub const ERM_OP_EPERM: u32 = 2;
/// 键域上界（1..=32）。
pub const ERM_KEY_MAX: u8 = 32;

/// op 名渲染。
pub fn erm_op_name(code: u32) -> &'static str {
    match code {
        ERM_OP_EINVAL => "EINVAL",
        ERM_OP_ESRCH => "ESRCH",
        ERM_OP_EPERM => "EPERM",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~32。
pub fn erm_key_ok(key: u8) -> bool {
    key != 0 && key <= ERM_KEY_MAX
}

/// 领域辅助：状态高位摘要（>> 4 截影，指纹前置压缩）。
pub fn erm_hi4(state: u64) -> u32 {
    (state >> 4) as u32
}

/// 单笔错误账。
#[derive(Clone)]
pub struct SigErrOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl SigErrOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            erm_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn erm_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        ERM_OP_EINVAL => old.wrapping_add(val),
        ERM_OP_ESRCH => old | val,
        ERM_OP_EPERM => val,
        _ => old,
    }
}

/// 错误账本。
pub struct SigErrLedger {
    ops: Vec<SigErrOp>,
    state: u64,
    sealed: bool,
}

impl SigErrLedger {
    pub fn new() -> Self {
        SigErrLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔错误：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(ERM_SEALED);
        }
        if code > ERM_OP_EPERM {
            return Err(ERM_BAD_OP);
        }
        if !erm_key_ok(key) {
            return Err(ERM_KEY_OOB);
        }
        let old = self.state;
        let new = erm_apply(old, code, val);
        self.ops.push(SigErrOp {
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
            if op.new_state != erm_apply(op.old_state, op.code, op.val) {
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

    /// ERM 指纹：状态 + 各笔操作折叠。
    pub fn erm_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9017u32;
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
        out.push_str(&format!("ERM-STATE|{}\n", self.state));
        out.push_str(&format!("ERM-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("ERM-SEAL\n");
        }
        out
    }
}

impl Default for SigErrLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（EINVAL[15]=11→ESRCH[10]=256→EINVAL[18]=2→EPERM[4]=114）。
/// 手验：0→11→267→269→114。
fn standard_erm() -> SigErrLedger {
    let mut l = SigErrLedger::new();
    let _ = l.apply(ERM_OP_EINVAL, 15, 11); // old 0   → new 11
    let _ = l.apply(ERM_OP_ESRCH, 10, 256); // old 11 → new 267
    let _ = l.apply(ERM_OP_EINVAL, 18, 2); // old 267 → new 269
    let _ = l.apply(ERM_OP_EPERM, 4, 114); // old 269 → new 114
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = SigErrLedger::new();
    if l.apply(7, 1, 1).err() == Some(ERM_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 33 双向）
    let mut l = SigErrLedger::new();
    if l.apply(ERM_OP_EINVAL, 0, 1).err() == Some(ERM_KEY_OOB)
        && l.apply(ERM_OP_EINVAL, 33, 1).err() == Some(ERM_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_erm();
    let _ = l.seal();
    if l.apply(ERM_OP_EINVAL, 1, 1).err() == Some(ERM_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = SigErrLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_erm();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 错误账；J2 错误语义；J3 封账印。
pub fn run_unx9017_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9017");
    let p = standard_erm();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9017-J1-操作全落账", ok, if ok { "" } else { "错误账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 11, 267, 269];
    s.add("9017-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("EINVAL|15|old0|new11")
        && text.contains("ESRCH|10|old11|new267")
        && text.contains("EINVAL|18|old267|new269")
        && text.contains("EPERM|4|old269|new114")
        && text.contains("ERM-STATE|114")
        && text.contains("ERM-POPC|4");
    s.add("9017-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 114
    let ok = p.state == 114;
    s.add("9017-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 EINVAL 加合（0 + 11 = 11）
    let ok = p.ops[0].code == ERM_OP_EINVAL
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 11
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(11);
    s.add("9017-J2-EINVAL加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 ESRCH 并合（11 | 256 = 267）
    let ok = p.ops[1].code == ERM_OP_ESRCH
        && p.ops[1].old_state == 11
        && p.ops[1].new_state == 267
        && p.ops[1].new_state == p.ops[1].old_state | 256;
    s.add("9017-J2-ESRCH并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 EPERM 直设（269 → 114）
    let ok = p.ops[3].code == ERM_OP_EPERM
        && p.ops[3].old_state == 269
        && p.ops[3].new_state == 114
        && p.ops[3].new_state == 114;
    s.add("9017-J2-EPERM直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~32 内收、0 与 33 拒）
    let ok = erm_key_ok(15) && erm_key_ok(32) && !erm_key_ok(0) && !erm_key_ok(33);
    s.add("9017-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 高位辅助（状态高位摘要（>> 4 截影））
    let ok = erm_hi4(267) == 16 && erm_hi4(0) == 0;
    s.add("9017-J2-高位", ok, if ok { "" } else { "高位漂移" });
    // J3-1 指纹确定
    let ok = standard_erm().erm_fp() == standard_erm().erm_fp();
    s.add("9017-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_erm();
    let fp0 = p2.erm_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.erm_fp() != fp0;
    s.add("9017-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_erm();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("ERM-SEAL");
    s.add("9017-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(ERM_OP_EPERM, 0, 0).err() == Some(ERM_SEALED);
    s.add("9017-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9017-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9017_all_checks_green() {
        let set = run_unx9017_checks();
        assert!(set.all_passed(), "UNX-F9017 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9017_state_stable() {
        let l = standard_erm();
        assert!(l.chain_ok());
        assert_eq!(l.state, 114);
        assert_eq!(l.state.count_ones(), 4);
    }
}
