//! UNX-F9071 glibc pthread 消费核（clone/futex 挂点）（0x628A0 段）
//!
//! 锚点：UNX-F9071-J1 消费账（pthread clone/futex 挂点四命令全落账 +
//! oldset 回读链 + 渲染导出面）；J2 消费语义（四态折叠核 + 挂点命令号→
//! 折叠臂映射 + 终值守恒）；J3 封账印（消费指纹 + 封印后只读 + 注入核）
//! 三判据全绿。
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

/// 诊断段（0x628A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x628A0;
pub const PX_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const PX_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const PX_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const PX_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const PX_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// pthread clone/futex 挂点四命令闭集。
pub const PX_CLONE: u32 = 0; // 线程创建挂点（累加臂）
pub const PX_FUTEX_WAIT: u32 = 1; // 阻塞等待挂点（异或搅动臂）
pub const PX_FUTEX_WAKE: u32 = 2; // 唤醒投递挂点（位并入臂）
pub const PX_DETACH: u32 = 3; // 分离清账挂点（位清除臂）
pub const PX_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn px_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        PX_CLONE => "CLONE",
        PX_FUTEX_WAIT => "FWAIT",
        PX_FUTEX_WAKE => "FWAKE",
        PX_DETACH => "DETACH",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn px_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        PX_CLONE => prev.wrapping_add(arg),
        PX_FUTEX_WAIT => prev ^ arg,
        PX_FUTEX_WAKE => prev | arg,
        PX_DETACH => prev & !arg,
        _ => prev,
    }
}

/// 单笔消费账。
#[derive(Clone)]
pub struct ThdOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl ThdOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            px_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// glibc pthread 消费账本。
pub struct PthreadConsumer {
    ops: Vec<ThdOp>,
    acc: u64,
    sealed: bool,
}

impl PthreadConsumer {
    pub fn new() -> Self {
        PthreadConsumer {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次挂点命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(PX_SEALED);
        }
        if cmd >= PX_CMD_COUNT {
            return Err(PX_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(PX_ARG_OOB);
        }
        let old = self.acc;
        let new = px_fold(old, cmd, arg);
        self.ops.push(ThdOp {
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
            if op.new_acc != px_fold(op.old_acc, op.cmd, op.arg) {
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
    pub fn px_fp(&self) -> u32 {
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
        out.push_str(&format!("PTHREAD-ACC|{}\n", self.acc));
        out.push_str(&format!("PTHREAD-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("PTHREAD-SEAL\n");
        }
        out
    }
}

impl Default for PthreadConsumer {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（CLONE[5]→CLONE[4]→FWAIT[17]→FWAKE[22]）。
/// 手验：0→5→9→24→30（9^17=24，24|22=30）。
fn standard_thread() -> PthreadConsumer {
    let mut t = PthreadConsumer::new();
    let _ = t.apply(PX_CLONE, 5); // old 0 → new 5
    let _ = t.apply(PX_CLONE, 4); // old 5 → new 9
    let _ = t.apply(PX_FUTEX_WAIT, 17); // old 9  → new 24
    let _ = t.apply(PX_FUTEX_WAKE, 22); // old 24 → new 30
    t
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令
    let mut t = PthreadConsumer::new();
    if t.apply(PX_CMD_COUNT, 1).err() == Some(PX_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut t = PthreadConsumer::new();
    if t.apply(PX_CLONE, 0).err() == Some(PX_ARG_OOB)
        && t.apply(PX_CLONE, 64).err() == Some(PX_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut t = standard_thread();
    let _ = t.seal();
    if t.apply(PX_CLONE, 7).err() == Some(PX_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut t = PthreadConsumer::new();
    if !t.seal() && !t.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂
    let mut t = standard_thread();
    t.ops[1].old_acc += 1;
    if !t.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 消费账；J2 消费语义；J3 封账印。
pub fn run_unx9071_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9071");
    let p = standard_thread();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9071-J1-操作全落账", ok, if ok { "" } else { "消费账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 5, 9, 24];
    s.add("9071-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("CLONE|5|old0|new5")
        && text.contains("CLONE|4|old5|new9")
        && text.contains("FWAIT|17|old9|new24")
        && text.contains("FWAKE|22|old24|new30")
        && text.contains("PTHREAD-ACC|30")
        && text.contains("PTHREAD-OPS|4");
    s.add("9071-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 30
    let ok = p.acc == 30;
    s.add("9071-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 CLONE 累加臂（0+5=5）
    let ok = p.ops[0].cmd == PX_CLONE
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 5
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9071-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 累加接力（5+4=9）
    let ok = p.ops[1].old_acc == 5 && p.ops[1].new_acc == 9;
    s.add("9071-J2-累加接力", ok, if ok { "" } else { "接力漂移" });
    // J2-4 FWAIT 异或臂（9^17=24）
    let ok = p.ops[2].cmd == PX_FUTEX_WAIT
        && p.ops[2].old_acc == 9
        && p.ops[2].new_acc == 24
        && p.ops[2].new_acc == (p.ops[2].old_acc ^ p.ops[2].arg);
    s.add("9071-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-5 FWAKE 并入臂（24|22=30）
    let ok = p.ops[3].cmd == PX_FUTEX_WAKE
        && p.ops[3].old_acc == 24
        && p.ops[3].new_acc == 30
        && p.ops[3].new_acc == (p.ops[3].old_acc | p.ops[3].arg);
    s.add("9071-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-6 DETACH 清除臂（63&!21=42）
    let ok = px_fold(63, PX_DETACH, 21) == 42;
    s.add("9071-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J3-1 指纹确定
    let ok = standard_thread().px_fp() == standard_thread().px_fp();
    s.add("9071-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_thread();
    let fp0 = p2.px_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.px_fp() != fp0;
    s.add("9071-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_thread();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("PTHREAD-SEAL");
    s.add("9071-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(PX_DETACH, 1).err() == Some(PX_SEALED);
    s.add("9071-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9071-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9071_all_checks_green() {
        let set = run_unx9071_checks();
        assert!(set.all_passed(), "UNX-F9071 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9071_acc_stable() {
        let t = standard_thread();
        assert!(t.chain_ok());
        assert_eq!(t.acc, 30);
        assert_eq!(t.oldsets(), vec![0u64, 5, 9, 24]);
    }
}
