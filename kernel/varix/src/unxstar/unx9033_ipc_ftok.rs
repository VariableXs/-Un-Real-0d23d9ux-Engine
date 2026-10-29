//! UNX-F9033 IPC 键空间与 ftok 语义（0x623e0 段）
//!
//! 锚点：UNX-F9033-J1 键值账（MKKEY/LOOK/DUP 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 键值语义（MKKEY 加合 / LOOK 并合 / DUP 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（FTK 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B12。
//!
//! 结构：键值账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! FTK 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 IPC 键空间与 ftok 语义 的语义单点：以三式账本承载 MKKEY/LOOK/DUP
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x623e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x623e0;
pub const FTK_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const FTK_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const FTK_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const FTK_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const FTK_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// MKKEY 式：加合（new = old.wrapping_add(val)）。
pub const FTK_OP_MKKEY: u32 = 0;
/// LOOK 式：并合（new = old | val）。
pub const FTK_OP_LOOK: u32 = 1;
/// DUP 式：直设（new = val）。
pub const FTK_OP_DUP: u32 = 2;
/// 键域上界（1..=16）。
pub const FTK_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn ftk_op_name(code: u32) -> &'static str {
    match code {
        FTK_OP_MKKEY => "MKKEY",
        FTK_OP_LOOK => "LOOK",
        FTK_OP_DUP => "DUP",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn ftk_key_ok(key: u8) -> bool {
    key != 0 && key <= FTK_KEY_MAX
}

/// 领域辅助：状态高位摘要（>> 4 截影，指纹前置压缩）。
pub fn ftk_hi4(state: u64) -> u32 {
    (state >> 4) as u32
}

/// 单笔键值账。
#[derive(Clone)]
pub struct FtokOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl FtokOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            ftk_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn ftk_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        FTK_OP_MKKEY => old.wrapping_add(val),
        FTK_OP_LOOK => old | val,
        FTK_OP_DUP => val,
        _ => old,
    }
}

/// 键值账本。
pub struct FtokLedger {
    ops: Vec<FtokOp>,
    state: u64,
    sealed: bool,
}

impl FtokLedger {
    pub fn new() -> Self {
        FtokLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔键值：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(FTK_SEALED);
        }
        if code > FTK_OP_DUP {
            return Err(FTK_BAD_OP);
        }
        if !ftk_key_ok(key) {
            return Err(FTK_KEY_OOB);
        }
        let old = self.state;
        let new = ftk_apply(old, code, val);
        self.ops.push(FtokOp {
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
            if op.new_state != ftk_apply(op.old_state, op.code, op.val) {
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

    /// FTK 指纹：状态 + 各笔操作折叠。
    pub fn ftk_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9033u32;
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
        out.push_str(&format!("FTK-STATE|{}\n", self.state));
        out.push_str(&format!("FTK-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("FTK-SEAL\n");
        }
        out
    }
}

impl Default for FtokLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（MKKEY[15]=15→LOOK[10]=64→MKKEY[2]=4→DUP[4]=130）。
/// 手验：0→15→79→83→130。
fn standard_ftk() -> FtokLedger {
    let mut l = FtokLedger::new();
    let _ = l.apply(FTK_OP_MKKEY, 15, 15); // old 0   → new 15
    let _ = l.apply(FTK_OP_LOOK, 10, 64); // old 15 → new 79
    let _ = l.apply(FTK_OP_MKKEY, 2, 4); // old 79 → new 83
    let _ = l.apply(FTK_OP_DUP, 4, 130); // old 83 → new 130
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = FtokLedger::new();
    if l.apply(7, 1, 1).err() == Some(FTK_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = FtokLedger::new();
    if l.apply(FTK_OP_MKKEY, 0, 1).err() == Some(FTK_KEY_OOB)
        && l.apply(FTK_OP_MKKEY, 17, 1).err() == Some(FTK_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_ftk();
    let _ = l.seal();
    if l.apply(FTK_OP_MKKEY, 1, 1).err() == Some(FTK_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = FtokLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_ftk();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 键值账；J2 键值语义；J3 封账印。
pub fn run_unx9033_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9033");
    let p = standard_ftk();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9033-J1-操作全落账", ok, if ok { "" } else { "键值账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 15, 79, 83];
    s.add("9033-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("MKKEY|15|old0|new15")
        && text.contains("LOOK|10|old15|new79")
        && text.contains("MKKEY|2|old79|new83")
        && text.contains("DUP|4|old83|new130")
        && text.contains("FTK-STATE|130")
        && text.contains("FTK-POPC|2");
    s.add("9033-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 130
    let ok = p.state == 130;
    s.add("9033-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 MKKEY 加合（0 + 15 = 15）
    let ok = p.ops[0].code == FTK_OP_MKKEY
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 15
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(15);
    s.add("9033-J2-MKKEY加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 LOOK 并合（15 | 64 = 79）
    let ok = p.ops[1].code == FTK_OP_LOOK
        && p.ops[1].old_state == 15
        && p.ops[1].new_state == 79
        && p.ops[1].new_state == p.ops[1].old_state | 64;
    s.add("9033-J2-LOOK并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 DUP 直设（83 → 130）
    let ok = p.ops[3].code == FTK_OP_DUP
        && p.ops[3].old_state == 83
        && p.ops[3].new_state == 130
        && p.ops[3].new_state == 130;
    s.add("9033-J2-DUP直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = ftk_key_ok(15) && ftk_key_ok(16) && !ftk_key_ok(0) && !ftk_key_ok(17);
    s.add("9033-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 高位辅助（状态高位摘要（>> 4 截影））
    let ok = ftk_hi4(79) == 4 && ftk_hi4(0) == 0;
    s.add("9033-J2-高位", ok, if ok { "" } else { "高位漂移" });
    // J3-1 指纹确定
    let ok = standard_ftk().ftk_fp() == standard_ftk().ftk_fp();
    s.add("9033-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_ftk();
    let fp0 = p2.ftk_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.ftk_fp() != fp0;
    s.add("9033-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_ftk();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("FTK-SEAL");
    s.add("9033-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(FTK_OP_DUP, 0, 0).err() == Some(FTK_SEALED);
    s.add("9033-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9033-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9033_all_checks_green() {
        let set = run_unx9033_checks();
        assert!(set.all_passed(), "UNX-F9033 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9033_state_stable() {
        let l = standard_ftk();
        assert!(l.chain_ok());
        assert_eq!(l.state, 130);
        assert_eq!(l.state.count_ones(), 2);
    }
}
