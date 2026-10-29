//! UNX-F9073 glibc 信号消费核（sigaction/raise 包装）（0x628E0 段）
//!
//! 锚点：UNX-F9073-J1 消费账（sigaction/raise 包装四命令全落账 + oldset
//! 回读链 + 渲染导出面）；J2 消费语义（四态折叠核 + 包装命令号→折叠臂
//! 映射 + 终值守恒）；J3 封账印（消费指纹 + 封印后只读 + 注入核）三判据
//! 全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面）。
//!
//! 结构：64 位消费账本 + 四态命令分派核（非法命令拒 + 参数越界拒）+
//! oldset 回读链 + 消费指纹 + 注入核（非法命令/参数越界/印后改账/空账
//! 封印/账链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x628E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x628E0;
pub const GS_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const GS_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const GS_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const GS_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const GS_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// sigaction/raise 包装四命令闭集。
pub const GS_ACTION: u32 = 0; // 处置登记（累加臂）
pub const GS_RAISE: u32 = 1; // 投递搅动（异或臂）
pub const GS_MASK: u32 = 2; // 掩码并入（位并入臂）
pub const GS_DISCARD: u32 = 3; // 丢弃清账（位清除臂）
pub const GS_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn gs_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        GS_ACTION => "ACTION",
        GS_RAISE => "RAISE",
        GS_MASK => "MASK",
        GS_DISCARD => "DISCARD",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn gs_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        GS_ACTION => prev.wrapping_add(arg),
        GS_RAISE => prev ^ arg,
        GS_MASK => prev | arg,
        GS_DISCARD => prev & !arg,
        _ => prev,
    }
}

/// 单笔消费账。
#[derive(Clone)]
pub struct SigConsumeOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl SigConsumeOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            gs_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// glibc 信号消费账本。
pub struct SigConsumer {
    ops: Vec<SigConsumeOp>,
    acc: u64,
    sealed: bool,
}

impl SigConsumer {
    pub fn new() -> Self {
        SigConsumer {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次包装命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(GS_SEALED);
        }
        if cmd >= GS_CMD_COUNT {
            return Err(GS_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(GS_ARG_OOB);
        }
        let old = self.acc;
        let new = gs_fold(old, cmd, arg);
        self.ops.push(SigConsumeOp {
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
            if op.new_acc != gs_fold(op.old_acc, op.cmd, op.arg) {
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

    /// 消费指纹。
    pub fn gs_fp(&self) -> u32 {
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
        out.push_str(&format!("GLIBCSIG-ACC|{}\n", self.acc));
        out.push_str(&format!("GLIBCSIG-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("GLIBCSIG-SEAL\n");
        }
        out
    }
}

impl Default for SigConsumer {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（ACTION[12]→ACTION[9]→RAISE[31]→MASK[5]）。
/// 手验：0→12→21→10→15（21^31=10，10|5=15）。
fn standard_sig() -> SigConsumer {
    let mut g = SigConsumer::new();
    let _ = g.apply(GS_ACTION, 12); // old 0  → new 12
    let _ = g.apply(GS_ACTION, 9); // old 12 → new 21
    let _ = g.apply(GS_RAISE, 31); // old 21 → new 10
    let _ = g.apply(GS_MASK, 5); // old 10 → new 15
    g
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令
    let mut g = SigConsumer::new();
    if g.apply(GS_CMD_COUNT, 1).err() == Some(GS_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut g = SigConsumer::new();
    if g.apply(GS_ACTION, 0).err() == Some(GS_ARG_OOB)
        && g.apply(GS_ACTION, 64).err() == Some(GS_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut g = standard_sig();
    let _ = g.seal();
    if g.apply(GS_ACTION, 7).err() == Some(GS_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut g = SigConsumer::new();
    if !g.seal() && !g.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂
    let mut g = standard_sig();
    g.ops[1].old_acc += 1;
    if !g.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 消费账；J2 消费语义；J3 封账印。
pub fn run_unx9073_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9073");
    let p = standard_sig();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9073-J1-操作全落账", ok, if ok { "" } else { "消费账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 12, 21, 10];
    s.add("9073-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("ACTION|12|old0|new12")
        && text.contains("ACTION|9|old12|new21")
        && text.contains("RAISE|31|old21|new10")
        && text.contains("MASK|5|old10|new15")
        && text.contains("GLIBCSIG-ACC|15")
        && text.contains("GLIBCSIG-OPS|4");
    s.add("9073-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 15
    let ok = p.acc == 15;
    s.add("9073-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 ACTION 累加臂（0+12=12）
    let ok = p.ops[0].cmd == GS_ACTION
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 12
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9073-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 累加接力（12+9=21）
    let ok = p.ops[1].old_acc == 12 && p.ops[1].new_acc == 21;
    s.add("9073-J2-累加接力", ok, if ok { "" } else { "接力漂移" });
    // J2-4 RAISE 异或臂（21^31=10）
    let ok = p.ops[2].cmd == GS_RAISE
        && p.ops[2].old_acc == 21
        && p.ops[2].new_acc == 10
        && p.ops[2].new_acc == (p.ops[2].old_acc ^ p.ops[2].arg);
    s.add("9073-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-5 MASK 并入臂（10|5=15）
    let ok = p.ops[3].cmd == GS_MASK
        && p.ops[3].old_acc == 10
        && p.ops[3].new_acc == 15
        && p.ops[3].new_acc == (p.ops[3].old_acc | p.ops[3].arg);
    s.add("9073-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-6 DISCARD 清除臂（63&!21=42）
    let ok = gs_fold(63, GS_DISCARD, 21) == 42;
    s.add("9073-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J3-1 指纹确定
    let ok = standard_sig().gs_fp() == standard_sig().gs_fp();
    s.add("9073-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_sig();
    let fp0 = p2.gs_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.gs_fp() != fp0;
    s.add("9073-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_sig();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("GLIBCSIG-SEAL");
    s.add("9073-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(GS_DISCARD, 1).err() == Some(GS_SEALED);
    s.add("9073-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9073-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9073_all_checks_green() {
        let set = run_unx9073_checks();
        assert!(set.all_passed(), "UNX-F9073 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9073_acc_stable() {
        let g = standard_sig();
        assert!(g.chain_ok());
        assert_eq!(g.acc, 15);
        assert_eq!(g.oldsets(), vec![0u64, 12, 21, 10]);
    }
}
