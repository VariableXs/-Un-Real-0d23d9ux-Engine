//! UNX-F9042 抽样组一：open/close/read/write 正反双判据（0x62500 段）
//!
//! 锚点：UNX-F9042-J1 文件账（OPEN/READ/WRITE 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 文件语义（OPEN 加合 / READ 并合 / WRITE 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（OCW 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B13。
//!
//! 结构：文件账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! OCW 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 抽样组一：open/close/read/write 正反双判据 的语义单点：以三式账本承载 OPEN/READ/WRITE
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62500 独占 0x20）。
pub const SEG_BASE: u32 = 0x62500;
pub const OCW_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const OCW_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const OCW_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const OCW_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const OCW_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// OPEN 式：加合（new = old.wrapping_add(val)）。
pub const OCW_OP_OPEN: u32 = 0;
/// READ 式：并合（new = old | val）。
pub const OCW_OP_READ: u32 = 1;
/// WRITE 式：直设（new = val）。
pub const OCW_OP_WRITE: u32 = 2;
/// 键域上界（1..=16）。
pub const OCW_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn ocw_op_name(code: u32) -> &'static str {
    match code {
        OCW_OP_OPEN => "OPEN",
        OCW_OP_READ => "READ",
        OCW_OP_WRITE => "WRITE",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn ocw_key_ok(key: u8) -> bool {
    key != 0 && key <= OCW_KEY_MAX
}

/// 领域辅助：值列求和（wrapping 折叠，批量面）。
pub fn ocw_sum(vals: &[u64]) -> u64 {
    vals.iter().fold(0u64, |a, &v| a.wrapping_add(v))
}

/// 单笔文件账。
#[derive(Clone)]
pub struct FdOpsOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl FdOpsOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            ocw_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn ocw_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        OCW_OP_OPEN => old.wrapping_add(val),
        OCW_OP_READ => old | val,
        OCW_OP_WRITE => val,
        _ => old,
    }
}

/// 文件账本。
pub struct FdOpsLedger {
    ops: Vec<FdOpsOp>,
    state: u64,
    sealed: bool,
}

impl FdOpsLedger {
    pub fn new() -> Self {
        FdOpsLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔文件：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(OCW_SEALED);
        }
        if code > OCW_OP_WRITE {
            return Err(OCW_BAD_OP);
        }
        if !ocw_key_ok(key) {
            return Err(OCW_KEY_OOB);
        }
        let old = self.state;
        let new = ocw_apply(old, code, val);
        self.ops.push(FdOpsOp {
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
            if op.new_state != ocw_apply(op.old_state, op.code, op.val) {
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

    /// OCW 指纹：状态 + 各笔操作折叠。
    pub fn ocw_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9042u32;
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
        out.push_str(&format!("OCW-STATE|{}\n", self.state));
        out.push_str(&format!("OCW-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("OCW-SEAL\n");
        }
        out
    }
}

impl Default for FdOpsLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（OPEN[8]=12→READ[7]=512→OPEN[13]=6→WRITE[3]=139）。
/// 手验：0→12→524→530→139。
fn standard_ocw() -> FdOpsLedger {
    let mut l = FdOpsLedger::new();
    let _ = l.apply(OCW_OP_OPEN, 8, 12); // old 0   → new 12
    let _ = l.apply(OCW_OP_READ, 7, 512); // old 12 → new 524
    let _ = l.apply(OCW_OP_OPEN, 13, 6); // old 524 → new 530
    let _ = l.apply(OCW_OP_WRITE, 3, 139); // old 530 → new 139
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = FdOpsLedger::new();
    if l.apply(7, 1, 1).err() == Some(OCW_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = FdOpsLedger::new();
    if l.apply(OCW_OP_OPEN, 0, 1).err() == Some(OCW_KEY_OOB)
        && l.apply(OCW_OP_OPEN, 17, 1).err() == Some(OCW_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_ocw();
    let _ = l.seal();
    if l.apply(OCW_OP_OPEN, 1, 1).err() == Some(OCW_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = FdOpsLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_ocw();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 文件账；J2 文件语义；J3 封账印。
pub fn run_unx9042_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9042");
    let p = standard_ocw();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9042-J1-操作全落账", ok, if ok { "" } else { "文件账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 12, 524, 530];
    s.add("9042-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("OPEN|8|old0|new12")
        && text.contains("READ|7|old12|new524")
        && text.contains("OPEN|13|old524|new530")
        && text.contains("WRITE|3|old530|new139")
        && text.contains("OCW-STATE|139")
        && text.contains("OCW-POPC|4");
    s.add("9042-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 139
    let ok = p.state == 139;
    s.add("9042-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 OPEN 加合（0 + 12 = 12）
    let ok = p.ops[0].code == OCW_OP_OPEN
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 12
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(12);
    s.add("9042-J2-OPEN加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 READ 并合（12 | 512 = 524）
    let ok = p.ops[1].code == OCW_OP_READ
        && p.ops[1].old_state == 12
        && p.ops[1].new_state == 524
        && p.ops[1].new_state == p.ops[1].old_state | 512;
    s.add("9042-J2-READ并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 WRITE 直设（530 → 139）
    let ok = p.ops[3].code == OCW_OP_WRITE
        && p.ops[3].old_state == 530
        && p.ops[3].new_state == 139
        && p.ops[3].new_state == 139;
    s.add("9042-J2-WRITE直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = ocw_key_ok(8) && ocw_key_ok(16) && !ocw_key_ok(0) && !ocw_key_ok(17);
    s.add("9042-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 求和辅助（值列求和（wrapping 折叠））
    let ok = ocw_sum(&[12, 6]) == 18 && ocw_sum(&[]) == 0;
    s.add("9042-J2-求和", ok, if ok { "" } else { "求和漂移" });
    // J3-1 指纹确定
    let ok = standard_ocw().ocw_fp() == standard_ocw().ocw_fp();
    s.add("9042-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_ocw();
    let fp0 = p2.ocw_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.ocw_fp() != fp0;
    s.add("9042-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_ocw();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("OCW-SEAL");
    s.add("9042-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(OCW_OP_WRITE, 0, 0).err() == Some(OCW_SEALED);
    s.add("9042-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9042-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9042_all_checks_green() {
        let set = run_unx9042_checks();
        assert!(set.all_passed(), "UNX-F9042 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9042_state_stable() {
        let l = standard_ocw();
        assert!(l.chain_ok());
        assert_eq!(l.state, 139);
        assert_eq!(l.state.count_ones(), 4);
    }
}
