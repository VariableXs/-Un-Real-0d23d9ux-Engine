//! UNX-F9132 mincore 语义档（驻留位向量）（0x63040 段）
//!
//! 锚点：UNX-F9132-J1 账面（四命令全落账 + oldset 回读链 + 渲染导出
//! 面）；J2 语义（四态折叠核 + 命令号→折叠臂映射 + 终值守恒）；J3 封账
//! 印（指纹 + 封印后只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B17（内存族面）。
//!
//! 结构：64 位账本 + 四态命令分派核（非法命令拒 + 参数越界拒）+ oldset
//! 回读链 + 账面指纹 + 注入核（非法命令/参数越界/印后改账/空账封印/
//! 账链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63040 独占 0x20）。
pub const SEG_BASE: u32 = 0x63040;
pub const MC_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const MC_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const MC_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const MC_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const MC_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// 四命令闭集。
pub const MC_CMD0: u32 = 0; // MINCORE（累加臂）
pub const MC_CMD1: u32 = 1; // RESBIT（异或搅动臂）
pub const MC_CMD2: u32 = 2; // VEC（位并入臂）
pub const MC_CMD3: u32 = 3; // CLRMN（位清除臂）
pub const MC_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn mc_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        MC_CMD0 => "MINCORE",
        MC_CMD1 => "RESBIT",
        MC_CMD2 => "VEC",
        MC_CMD3 => "CLRMN",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn mc_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        MC_CMD0 => prev.wrapping_add(arg),
        MC_CMD1 => prev ^ arg,
        MC_CMD2 => prev | arg,
        MC_CMD3 => prev & !arg,
        _ => prev,
    }
}

/// 扩展探针：位域往返守恒（并入后清除必回原值）。
pub fn mc_roundtrip(prev: u64, bits: u64) -> bool {
    mc_fold(mc_fold(prev, MC_CMD2, bits), MC_CMD3, bits) == prev
}

/// 单笔账。
#[derive(Clone)]
pub struct MincoreOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl MincoreOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            mc_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// UNX-F9132 账本。
pub struct MincoreLedger {
    ops: Vec<MincoreOp>,
    acc: u64,
    sealed: bool,
}

impl MincoreLedger {
    pub fn new() -> Self {
        MincoreLedger {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    /// Ok(old) 即 oldset 回读。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(MC_SEALED);
        }
        if cmd >= MC_CMD_COUNT {
            return Err(MC_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(MC_ARG_OOB);
        }
        let old = self.acc;
        let new = mc_fold(old, cmd, arg);
        self.ops.push(MincoreOp {
            cmd,
            arg,
            old_acc: old,
            new_acc: new,
        });
        self.acc = new;
        Ok(old)
    }

    /// 账链守恒核：首笔 old = 0，逐笔 old = 前笔 new，
    /// 每笔 new 可由四态核重算复现，末笔 new = 当前终值。
    pub fn chain_ok(&self) -> bool {
        let mut expect: u64 = 0;
        for op in &self.ops {
            if op.old_acc != expect {
                return false;
            }
            if op.new_acc != mc_fold(op.old_acc, op.cmd, op.arg) {
                return false;
            }
            expect = op.new_acc;
        }
        self.acc == expect
    }

    /// oldset 逐笔回读序列。
    pub fn oldsets(&self) -> Vec<u64> {
        self.ops.iter().map(|op| op.old_acc).collect()
    }

    /// 峰值回读（账面链上最大 new_acc）。
    pub fn peak_acc(&self) -> u64 {
        self.ops.iter().map(|op| op.new_acc).max().unwrap_or(0)
    }

    /// 回读均值 ×100（oldset 定点两位均值）。
    pub fn mean_acc_x100(&self) -> u64 {
        if self.ops.is_empty() {
            return 0;
        }
        let sum: u64 = self.ops.iter().map(|op| op.old_acc).sum();
        sum * 100 / self.ops.len() as u64
    }

    /// 账面指纹：终值 + 各笔折叠。
    pub fn mc_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ (self.acc as u32);
        h = h.wrapping_mul(0x0100_0193) ^ ((self.acc >> 32) as u32);
        for op in &self.ops {
            h = h.wrapping_mul(0x0100_0193) ^ op.cmd;
            h = h.wrapping_mul(0x0100_0193) ^ (op.arg as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (op.old_acc as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (op.new_acc as u32);
            h ^= ((op.old_acc >> 32) as u32) ^ ((op.new_acc >> 32) as u32);
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
        out.push_str(&format!("MINCORE-ACC|{}\n", self.acc));
        out.push_str(&format!("MINCORE-PEAK|{}\n", self.peak_acc()));
        out.push_str(&format!("MINCORE-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("MINCORE-SEAL\n");
        }
        out
    }
}

impl Default for MincoreLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（MINCORE[3]→MINCORE[11]→RESBIT[21]→VEC[6]）。
/// 手验：0→3→14→27→31（14^21=27，27|6=31）。
fn standard_ledger() -> MincoreLedger {
    let mut l = MincoreLedger::new();
    let _ = l.apply(MC_CMD0, 3); // old 0    → new 3
    let _ = l.apply(MC_CMD0, 11); // old 3   → new 14
    let _ = l.apply(MC_CMD1, 21); // old 14  → new 27
    let _ = l.apply(MC_CMD2, 6); // old 27  → new 31
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令（CMD_COUNT 越界）
    let mut l = MincoreLedger::new();
    if l.apply(MC_CMD_COUNT, 1).err() == Some(MC_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut l = MincoreLedger::new();
    if l.apply(MC_CMD0, 0).err() == Some(MC_ARG_OOB)
        && l.apply(MC_CMD0, 64).err() == Some(MC_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.apply(MC_CMD0, 7).err() == Some(MC_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = MincoreLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂（篡改中段 oldset）
    let mut l = standard_ledger();
    l.ops[1].old_acc += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 账面；J2 语义；J3 封账印。
pub fn run_unx9132_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9132");
    let p = standard_ledger();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9132-J1-操作全落账", ok, if ok { "" } else { "账面不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 3, 14, 27];
    s.add("9132-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("MINCORE|3|old0|new3")
        && text.contains("MINCORE|11|old3|new14")
        && text.contains("RESBIT|21|old14|new27")
        && text.contains("VEC|6|old27|new31")
        && text.contains("MINCORE-ACC|31")
        && text.contains("MINCORE-PEAK|31")
        && text.contains("MINCORE-OPS|4");
    s.add("9132-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 31
    let ok = p.acc == 31;
    s.add("9132-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 CMD0 累加臂（0+3=3）
    let ok = p.ops[0].cmd == MC_CMD0
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 3
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9132-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 累加接力（3+11=14）
    let ok = p.ops[1].old_acc == 3 && p.ops[1].new_acc == 14;
    s.add("9132-J2-累加接力", ok, if ok { "" } else { "接力漂移" });
    // J2-4 CMD1 异或臂（14^21=27）
    let ok = p.ops[2].cmd == MC_CMD1
        && p.ops[2].old_acc == 14
        && p.ops[2].new_acc == 27
        && p.ops[2].new_acc == (p.ops[2].old_acc ^ p.ops[2].arg);
    s.add("9132-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-5 CMD2 并入臂（27|6=31）
    let ok = p.ops[3].cmd == MC_CMD2
        && p.ops[3].old_acc == 27
        && p.ops[3].new_acc == 31
        && p.ops[3].new_acc == (p.ops[3].old_acc | p.ops[3].arg);
    s.add("9132-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-6 CMD3 清除臂（63&!21=42）
    let ok = mc_fold(63, MC_CMD3, 21) == 42;
    s.add("9132-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J3-1 指纹确定
    let ok = standard_ledger().mc_fp() == standard_ledger().mc_fp();
    s.add("9132-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_ledger();
    let fp0 = p2.mc_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.mc_fp() != fp0;
    s.add("9132-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_ledger();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("MINCORE-SEAL");
    s.add("9132-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(MC_CMD3, 1).err() == Some(MC_SEALED);
    s.add("9132-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9132-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9132_all_checks_green() {
        let set = run_unx9132_checks();
        assert!(set.all_passed(), "UNX-F9132 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9132_acc_stable() {
        let l = standard_ledger();
        assert!(l.chain_ok());
        assert_eq!(l.acc, 31);
        assert_eq!(l.oldsets(), vec![0u64, 3, 14, 27]);
    }
}
