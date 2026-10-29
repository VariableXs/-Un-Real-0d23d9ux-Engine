//! UNX-F9079 glibc 版本锚定账本（2.31/2.35 行为差异登记）（0x629A0 段）
//!
//! 锚点：UNX-F9079-J1 锚定账（版本差异四命令全落账 + oldset 回读链 +
//! 渲染导出面）；J2 锚定语义（四态折叠核 + 差异命令号→折叠臂映射 + 终
//! 值守恒）；J3 封账印（锚定指纹 + 封印后只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面）。
//!
//! 结构：64 位锚定账本 + 四态命令分派核（非法命令拒 + 参数越界拒）+
//! oldset 回读链 + 锚定指纹 + 注入核（非法命令/参数越界/印后改账/空账
//! 封印/账链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x629A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x629A0;
pub const GV_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const GV_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const GV_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const GV_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const GV_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// 2.31/2.35 行为差异登记四命令闭集。
pub const GV_V231: u32 = 0; // 2.31 基线登记（累加臂）
pub const GV_V235: u32 = 1; // 2.35 差异搅动（异或臂）
pub const GV_DELTA: u32 = 2; // 差异位并入（位并入臂）
pub const GV_ANCHOR: u32 = 3; // 锚定清账（位清除臂）
pub const GV_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn gv_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        GV_V231 => "V231",
        GV_V235 => "V235",
        GV_DELTA => "DELTA",
        GV_ANCHOR => "ANCHOR",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn gv_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        GV_V231 => prev.wrapping_add(arg),
        GV_V235 => prev ^ arg,
        GV_DELTA => prev | arg,
        GV_ANCHOR => prev & !arg,
        _ => prev,
    }
}

/// 单笔锚定账。
#[derive(Clone)]
pub struct VerOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl VerOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            gv_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// glibc 版本锚定账本。
pub struct VerLedger {
    ops: Vec<VerOp>,
    acc: u64,
    sealed: bool,
}

impl VerLedger {
    pub fn new() -> Self {
        VerLedger {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次锚定命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(GV_SEALED);
        }
        if cmd >= GV_CMD_COUNT {
            return Err(GV_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(GV_ARG_OOB);
        }
        let old = self.acc;
        let new = gv_fold(old, cmd, arg);
        self.ops.push(VerOp {
            cmd,
            arg,
            old_acc: old,
            new_acc: new,
        });
        self.acc = new;
        Ok(old)
    }

    /// 账链守恒核：首笔 old = 0，逐笔 old = 前笔 new，末笔 new = 终值。
    pub fn chain_ok(&self) -> bool {
        let mut expect: u64 = 0;
        for op in &self.ops {
            if op.old_acc != expect {
                return false;
            }
            if op.new_acc != gv_fold(op.old_acc, op.cmd, op.arg) {
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

    /// 锚定指纹。
    pub fn gv_fp(&self) -> u32 {
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
        out.push_str(&format!("GLIBCVER-ACC|{}\n", self.acc));
        out.push_str(&format!("GLIBCVER-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("GLIBCVER-SEAL\n");
        }
        out
    }
}

impl Default for VerLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（V231[12]→V231[9]→V235[31]→DELTA[5]）。
/// 手验：0→12→21→10→15（21^31=10，10|5=15）。
fn standard_ver() -> VerLedger {
    let mut v = VerLedger::new();
    let _ = v.apply(GV_V231, 12); // old 0  → new 12
    let _ = v.apply(GV_V231, 9); // old 12 → new 21
    let _ = v.apply(GV_V235, 31); // old 21 → new 10
    let _ = v.apply(GV_DELTA, 5); // old 10 → new 15
    v
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令
    let mut v = VerLedger::new();
    if v.apply(GV_CMD_COUNT, 1).err() == Some(GV_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut v = VerLedger::new();
    if v.apply(GV_V231, 0).err() == Some(GV_ARG_OOB)
        && v.apply(GV_V231, 64).err() == Some(GV_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut v = standard_ver();
    let _ = v.seal();
    if v.apply(GV_V231, 7).err() == Some(GV_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut v = VerLedger::new();
    if !v.seal() && !v.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂
    let mut v = standard_ver();
    v.ops[1].old_acc += 1;
    if !v.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 锚定账；J2 锚定语义；J3 封账印。
pub fn run_unx9079_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9079");
    let p = standard_ver();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9079-J1-操作全落账", ok, if ok { "" } else { "锚定账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 12, 21, 10];
    s.add("9079-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("V231|12|old0|new12")
        && text.contains("V231|9|old12|new21")
        && text.contains("V235|31|old21|new10")
        && text.contains("DELTA|5|old10|new15")
        && text.contains("GLIBCVER-ACC|15")
        && text.contains("GLIBCVER-OPS|4");
    s.add("9079-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 15
    let ok = p.acc == 15;
    s.add("9079-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 V231 累加臂（0+12=12）
    let ok = p.ops[0].cmd == GV_V231
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 12
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9079-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 累加接力（12+9=21）
    let ok = p.ops[1].old_acc == 12 && p.ops[1].new_acc == 21;
    s.add("9079-J2-累加接力", ok, if ok { "" } else { "接力漂移" });
    // J2-4 V235 异或臂（21^31=10）
    let ok = p.ops[2].cmd == GV_V235
        && p.ops[2].old_acc == 21
        && p.ops[2].new_acc == 10
        && p.ops[2].new_acc == (p.ops[2].old_acc ^ p.ops[2].arg);
    s.add("9079-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-5 DELTA 并入臂（10|5=15）
    let ok = p.ops[3].cmd == GV_DELTA
        && p.ops[3].old_acc == 10
        && p.ops[3].new_acc == 15
        && p.ops[3].new_acc == (p.ops[3].old_acc | p.ops[3].arg);
    s.add("9079-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-6 ANCHOR 清除臂（63&!21=42）
    let ok = gv_fold(63, GV_ANCHOR, 21) == 42;
    s.add("9079-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J3-1 指纹确定
    let ok = standard_ver().gv_fp() == standard_ver().gv_fp();
    s.add("9079-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_ver();
    let fp0 = p2.gv_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.gv_fp() != fp0;
    s.add("9079-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_ver();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("GLIBCVER-SEAL");
    s.add("9079-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(GV_ANCHOR, 1).err() == Some(GV_SEALED);
    s.add("9079-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9079-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9079_all_checks_green() {
        let set = run_unx9079_checks();
        assert!(set.all_passed(), "UNX-F9079 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9079_acc_stable() {
        let v = standard_ver();
        assert!(v.chain_ok());
        assert_eq!(v.acc, 15);
        assert_eq!(v.oldsets(), vec![0u64, 12, 21, 10]);
    }
}
