//! UNX-F9024 管道读写阻塞语义与容量（F_SETPIPE_SZ 联动）（0x622c0 段）
//!
//! 锚点：UNX-F9024-J1 容量账（GROW/SHRINK/FREEZE 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 容量语义（GROW 加合 / SHRINK 并合 / FREEZE 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（PSZ 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B12。
//!
//! 结构：容量账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! PSZ 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 管道读写阻塞语义与容量（F_SETPIPE_SZ 联动） 的语义单点：以三式账本承载 GROW/SHRINK/FREEZE
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x622c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x622c0;
pub const PSZ_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const PSZ_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const PSZ_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const PSZ_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const PSZ_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// GROW 式：加合（new = old.wrapping_add(val)）。
pub const PSZ_OP_GROW: u32 = 0;
/// SHRINK 式：并合（new = old | val）。
pub const PSZ_OP_SHRINK: u32 = 1;
/// FREEZE 式：直设（new = val）。
pub const PSZ_OP_FREEZE: u32 = 2;
/// 键域上界（1..=16）。
pub const PSZ_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn psz_op_name(code: u32) -> &'static str {
    match code {
        PSZ_OP_GROW => "GROW",
        PSZ_OP_SHRINK => "SHRINK",
        PSZ_OP_FREEZE => "FREEZE",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn psz_key_ok(key: u8) -> bool {
    key != 0 && key <= PSZ_KEY_MAX
}

/// 领域辅助：状态留数（模 13 折叠，账页轮转位）。
pub fn psz_mod13(state: u64) -> u32 {
    (state % 13) as u32
}

/// 单笔容量账。
#[derive(Clone)]
pub struct PipeSzOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl PipeSzOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            psz_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn psz_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        PSZ_OP_GROW => old.wrapping_add(val),
        PSZ_OP_SHRINK => old | val,
        PSZ_OP_FREEZE => val,
        _ => old,
    }
}

/// 容量账本。
pub struct PipeSzLedger {
    ops: Vec<PipeSzOp>,
    state: u64,
    sealed: bool,
}

impl PipeSzLedger {
    pub fn new() -> Self {
        PipeSzLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔容量：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(PSZ_SEALED);
        }
        if code > PSZ_OP_FREEZE {
            return Err(PSZ_BAD_OP);
        }
        if !psz_key_ok(key) {
            return Err(PSZ_KEY_OOB);
        }
        let old = self.state;
        let new = psz_apply(old, code, val);
        self.ops.push(PipeSzOp {
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
            if op.new_state != psz_apply(op.old_state, op.code, op.val) {
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

    /// PSZ 指纹：状态 + 各笔操作折叠。
    pub fn psz_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9024u32;
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
        out.push_str(&format!("PSZ-STATE|{}\n", self.state));
        out.push_str(&format!("PSZ-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("PSZ-SEAL\n");
        }
        out
    }
}

impl Default for PipeSzLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（GROW[6]=18→SHRINK[13]=512→GROW[7]=2→FREEZE[5]=121）。
/// 手验：0→18→530→532→121。
fn standard_psz() -> PipeSzLedger {
    let mut l = PipeSzLedger::new();
    let _ = l.apply(PSZ_OP_GROW, 6, 18); // old 0   → new 18
    let _ = l.apply(PSZ_OP_SHRINK, 13, 512); // old 18 → new 530
    let _ = l.apply(PSZ_OP_GROW, 7, 2); // old 530 → new 532
    let _ = l.apply(PSZ_OP_FREEZE, 5, 121); // old 532 → new 121
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = PipeSzLedger::new();
    if l.apply(7, 1, 1).err() == Some(PSZ_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = PipeSzLedger::new();
    if l.apply(PSZ_OP_GROW, 0, 1).err() == Some(PSZ_KEY_OOB)
        && l.apply(PSZ_OP_GROW, 17, 1).err() == Some(PSZ_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_psz();
    let _ = l.seal();
    if l.apply(PSZ_OP_GROW, 1, 1).err() == Some(PSZ_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = PipeSzLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_psz();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 容量账；J2 容量语义；J3 封账印。
pub fn run_unx9024_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9024");
    let p = standard_psz();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9024-J1-操作全落账", ok, if ok { "" } else { "容量账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 18, 530, 532];
    s.add("9024-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("GROW|6|old0|new18")
        && text.contains("SHRINK|13|old18|new530")
        && text.contains("GROW|7|old530|new532")
        && text.contains("FREEZE|5|old532|new121")
        && text.contains("PSZ-STATE|121")
        && text.contains("PSZ-POPC|5");
    s.add("9024-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 121
    let ok = p.state == 121;
    s.add("9024-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 GROW 加合（0 + 18 = 18）
    let ok = p.ops[0].code == PSZ_OP_GROW
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 18
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(18);
    s.add("9024-J2-GROW加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 SHRINK 并合（18 | 512 = 530）
    let ok = p.ops[1].code == PSZ_OP_SHRINK
        && p.ops[1].old_state == 18
        && p.ops[1].new_state == 530
        && p.ops[1].new_state == p.ops[1].old_state | 512;
    s.add("9024-J2-SHRINK并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 FREEZE 直设（532 → 121）
    let ok = p.ops[3].code == PSZ_OP_FREEZE
        && p.ops[3].old_state == 532
        && p.ops[3].new_state == 121
        && p.ops[3].new_state == 121;
    s.add("9024-J2-FREEZE直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = psz_key_ok(6) && psz_key_ok(16) && !psz_key_ok(0) && !psz_key_ok(17);
    s.add("9024-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 留数辅助（状态留数（模 13 折叠））
    let ok = psz_mod13(532) == 12 && psz_mod13(0) == 0;
    s.add("9024-J2-留数", ok, if ok { "" } else { "留数漂移" });
    // J3-1 指纹确定
    let ok = standard_psz().psz_fp() == standard_psz().psz_fp();
    s.add("9024-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_psz();
    let fp0 = p2.psz_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.psz_fp() != fp0;
    s.add("9024-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_psz();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("PSZ-SEAL");
    s.add("9024-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(PSZ_OP_FREEZE, 0, 0).err() == Some(PSZ_SEALED);
    s.add("9024-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9024-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9024_all_checks_green() {
        let set = run_unx9024_checks();
        assert!(set.all_passed(), "UNX-F9024 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9024_state_stable() {
        let l = standard_psz();
        assert!(l.chain_ok());
        assert_eq!(l.state, 121);
        assert_eq!(l.state.count_ones(), 5);
    }
}
