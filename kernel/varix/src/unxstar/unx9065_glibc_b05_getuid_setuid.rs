//! UNX-F9065 glibc 身份族消费核（B05 号面 getuid/setuid）（0x627e0 段）
//!
//! 锚点：UNX-F9065-J1 身份账（GETUID/SETUID/CHECK 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 身份语义（GETUID 加合 / SETUID 并合 / CHECK 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（GUI 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B14。
//!
//! 结构：身份账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! GUI 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 glibc 身份族消费核（B05 号面 getuid/setuid） 的语义单点：以三式账本承载 GETUID/SETUID/CHECK
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x627e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x627e0;
pub const GUI_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const GUI_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const GUI_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const GUI_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const GUI_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// GETUID 式：加合（new = old.wrapping_add(val)）。
pub const GUI_OP_GETUID: u32 = 0;
/// SETUID 式：并合（new = old | val）。
pub const GUI_OP_SETUID: u32 = 1;
/// CHECK 式：直设（new = val）。
pub const GUI_OP_CHECK: u32 = 2;
/// 键域上界（1..=16）。
pub const GUI_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn gui_op_name(code: u32) -> &'static str {
    match code {
        GUI_OP_GETUID => "GETUID",
        GUI_OP_SETUID => "SETUID",
        GUI_OP_CHECK => "CHECK",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn gui_key_ok(key: u8) -> bool {
    key != 0 && key <= GUI_KEY_MAX
}

/// 领域辅助：状态高位摘要（>> 4 截影，指纹前置压缩）。
pub fn gui_hi4(state: u64) -> u32 {
    (state >> 4) as u32
}

/// 单笔身份账。
#[derive(Clone)]
pub struct GlibcIdOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl GlibcIdOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            gui_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn gui_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        GUI_OP_GETUID => old.wrapping_add(val),
        GUI_OP_SETUID => old | val,
        GUI_OP_CHECK => val,
        _ => old,
    }
}

/// 身份账本。
pub struct GlibcIdLedger {
    ops: Vec<GlibcIdOp>,
    state: u64,
    sealed: bool,
}

impl GlibcIdLedger {
    pub fn new() -> Self {
        GlibcIdLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔身份：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(GUI_SEALED);
        }
        if code > GUI_OP_CHECK {
            return Err(GUI_BAD_OP);
        }
        if !gui_key_ok(key) {
            return Err(GUI_KEY_OOB);
        }
        let old = self.state;
        let new = gui_apply(old, code, val);
        self.ops.push(GlibcIdOp {
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
            if op.new_state != gui_apply(op.old_state, op.code, op.val) {
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

    /// GUI 指纹：状态 + 各笔操作折叠。
    pub fn gui_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9065u32;
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
        out.push_str(&format!("GUI-STATE|{}\n", self.state));
        out.push_str(&format!("GUI-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("GUI-SEAL\n");
        }
        out
    }
}

impl Default for GlibcIdLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（GETUID[15]=11→SETUID[10]=256→GETUID[2]=8→CHECK[4]=162）。
/// 手验：0→11→267→275→162。
fn standard_gui() -> GlibcIdLedger {
    let mut l = GlibcIdLedger::new();
    let _ = l.apply(GUI_OP_GETUID, 15, 11); // old 0   → new 11
    let _ = l.apply(GUI_OP_SETUID, 10, 256); // old 11 → new 267
    let _ = l.apply(GUI_OP_GETUID, 2, 8); // old 267 → new 275
    let _ = l.apply(GUI_OP_CHECK, 4, 162); // old 275 → new 162
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = GlibcIdLedger::new();
    if l.apply(7, 1, 1).err() == Some(GUI_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = GlibcIdLedger::new();
    if l.apply(GUI_OP_GETUID, 0, 1).err() == Some(GUI_KEY_OOB)
        && l.apply(GUI_OP_GETUID, 17, 1).err() == Some(GUI_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_gui();
    let _ = l.seal();
    if l.apply(GUI_OP_GETUID, 1, 1).err() == Some(GUI_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = GlibcIdLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_gui();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 身份账；J2 身份语义；J3 封账印。
pub fn run_unx9065_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9065");
    let p = standard_gui();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9065-J1-操作全落账", ok, if ok { "" } else { "身份账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 11, 267, 275];
    s.add("9065-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("GETUID|15|old0|new11")
        && text.contains("SETUID|10|old11|new267")
        && text.contains("GETUID|2|old267|new275")
        && text.contains("CHECK|4|old275|new162")
        && text.contains("GUI-STATE|162")
        && text.contains("GUI-POPC|3");
    s.add("9065-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 162
    let ok = p.state == 162;
    s.add("9065-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 GETUID 加合（0 + 11 = 11）
    let ok = p.ops[0].code == GUI_OP_GETUID
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 11
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(11);
    s.add("9065-J2-GETUID加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 SETUID 并合（11 | 256 = 267）
    let ok = p.ops[1].code == GUI_OP_SETUID
        && p.ops[1].old_state == 11
        && p.ops[1].new_state == 267
        && p.ops[1].new_state == p.ops[1].old_state | 256;
    s.add("9065-J2-SETUID并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 CHECK 直设（275 → 162）
    let ok = p.ops[3].code == GUI_OP_CHECK
        && p.ops[3].old_state == 275
        && p.ops[3].new_state == 162
        && p.ops[3].new_state == 162;
    s.add("9065-J2-CHECK直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = gui_key_ok(15) && gui_key_ok(16) && !gui_key_ok(0) && !gui_key_ok(17);
    s.add("9065-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 高位辅助（状态高位摘要（>> 4 截影））
    let ok = gui_hi4(267) == 16 && gui_hi4(0) == 0;
    s.add("9065-J2-高位", ok, if ok { "" } else { "高位漂移" });
    // J3-1 指纹确定
    let ok = standard_gui().gui_fp() == standard_gui().gui_fp();
    s.add("9065-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_gui();
    let fp0 = p2.gui_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.gui_fp() != fp0;
    s.add("9065-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_gui();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("GUI-SEAL");
    s.add("9065-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(GUI_OP_CHECK, 0, 0).err() == Some(GUI_SEALED);
    s.add("9065-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9065-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9065_all_checks_green() {
        let set = run_unx9065_checks();
        assert!(set.all_passed(), "UNX-F9065 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9065_state_stable() {
        let l = standard_gui();
        assert!(l.chain_ok());
        assert_eq!(l.state, 162);
        assert_eq!(l.state.count_ones(), 3);
    }
}
