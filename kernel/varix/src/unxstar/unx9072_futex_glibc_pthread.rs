//! UNX-F9072 futex 语义档（glibc pthread 锁消费本体）（0x628C0 段）
//!
//! 锚点：UNX-F9072-J1 锁账（futex 四命令全落账 + oldset 回读链 + 渲染
//! 导出面）；J2 锁语义（四态折叠核 + 锁命令号→折叠臂映射 + 终值守恒）；
//! J3 封账印（锁指纹 + 封印后只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面）。
//!
//! 结构：64 位锁账本 + 四态命令分派核（非法命令拒 + 参数越界拒）+
//! oldset 回读链 + 锁指纹 + 注入核（非法命令/参数越界/印后改账/空账
//! 封印/账链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x628C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x628C0;
pub const FT_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const FT_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const FT_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const FT_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const FT_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// futex 四命令闭集。
pub const FT_WAIT: u32 = 0; // 阻塞等待（累加臂）
pub const FT_WAKE: u32 = 1; // 唤醒（异或搅动臂）
pub const FT_CMP_REQUEUE: u32 = 2; // 比较重排（位并入臂）
pub const FT_UNLOCK: u32 = 3; // 解锁清账（位清除臂）
pub const FT_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn ft_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        FT_WAIT => "WAIT",
        FT_WAKE => "WAKE",
        FT_CMP_REQUEUE => "REQUEUE",
        FT_UNLOCK => "UNLOCK",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn ft_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        FT_WAIT => prev.wrapping_add(arg),
        FT_WAKE => prev ^ arg,
        FT_CMP_REQUEUE => prev | arg,
        FT_UNLOCK => prev & !arg,
        _ => prev,
    }
}

/// 单笔锁账。
#[derive(Clone)]
pub struct FutexOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl FutexOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            ft_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// futex 锁消费账本。
pub struct FutexLedger {
    ops: Vec<FutexOp>,
    acc: u64,
    sealed: bool,
}

impl FutexLedger {
    pub fn new() -> Self {
        FutexLedger {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次 futex 命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(FT_SEALED);
        }
        if cmd >= FT_CMD_COUNT {
            return Err(FT_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(FT_ARG_OOB);
        }
        let old = self.acc;
        let new = ft_fold(old, cmd, arg);
        self.ops.push(FutexOp {
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
            if op.new_acc != ft_fold(op.old_acc, op.cmd, op.arg) {
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

    /// 锁指纹。
    pub fn ft_fp(&self) -> u32 {
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
        out.push_str(&format!("FUTEX-ACC|{}\n", self.acc));
        out.push_str(&format!("FUTEX-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("FUTEX-SEAL\n");
        }
        out
    }
}

impl Default for FutexLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（WAIT[10]→WAKE[7]→REQUEUE[34]→UNLOCK[2]）。
/// 手验：0→10→13→47→45（10^7=13，13|34=47，47&!2=45）。
fn standard_futex() -> FutexLedger {
    let mut f = FutexLedger::new();
    let _ = f.apply(FT_WAIT, 10); // old 0  → new 10
    let _ = f.apply(FT_WAKE, 7); // old 10 → new 13
    let _ = f.apply(FT_CMP_REQUEUE, 34); // old 13 → new 47
    let _ = f.apply(FT_UNLOCK, 2); // old 47 → new 45
    f
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令
    let mut f = FutexLedger::new();
    if f.apply(FT_CMD_COUNT, 1).err() == Some(FT_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut f = FutexLedger::new();
    if f.apply(FT_WAIT, 0).err() == Some(FT_ARG_OOB)
        && f.apply(FT_WAIT, 64).err() == Some(FT_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut f = standard_futex();
    let _ = f.seal();
    if f.apply(FT_WAIT, 7).err() == Some(FT_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut f = FutexLedger::new();
    if !f.seal() && !f.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂
    let mut f = standard_futex();
    f.ops[1].old_acc += 1;
    if !f.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 锁账；J2 锁语义；J3 封账印。
pub fn run_unx9072_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9072");
    let p = standard_futex();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9072-J1-操作全落账", ok, if ok { "" } else { "锁账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 10, 13, 47];
    s.add("9072-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("WAIT|10|old0|new10")
        && text.contains("WAKE|7|old10|new13")
        && text.contains("REQUEUE|34|old13|new47")
        && text.contains("UNLOCK|2|old47|new45")
        && text.contains("FUTEX-ACC|45")
        && text.contains("FUTEX-OPS|4");
    s.add("9072-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 45
    let ok = p.acc == 45;
    s.add("9072-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 WAIT 累加臂（0+10=10）
    let ok = p.ops[0].cmd == FT_WAIT
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 10
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9072-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 WAKE 异或臂（10^7=13）
    let ok = p.ops[1].cmd == FT_WAKE
        && p.ops[1].old_acc == 10
        && p.ops[1].new_acc == 13
        && p.ops[1].new_acc == (p.ops[1].old_acc ^ p.ops[1].arg);
    s.add("9072-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-4 REQUEUE 并入臂（13|34=47）
    let ok = p.ops[2].cmd == FT_CMP_REQUEUE
        && p.ops[2].old_acc == 13
        && p.ops[2].new_acc == 47
        && p.ops[2].new_acc == (p.ops[2].old_acc | p.ops[2].arg);
    s.add("9072-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-5 UNLOCK 清除臂（47&!2=45）
    let ok = p.ops[3].cmd == FT_UNLOCK
        && p.ops[3].old_acc == 47
        && p.ops[3].new_acc == 45
        && p.ops[3].new_acc == (p.ops[3].old_acc & !p.ops[3].arg);
    s.add("9072-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J2-6 折叠核补臂（255&!15=240）
    let ok = ft_fold(255, FT_UNLOCK, 15) == 240;
    s.add("9072-J2-折叠补臂", ok, if ok { "" } else { "补臂漂移" });
    // J3-1 指纹确定
    let ok = standard_futex().ft_fp() == standard_futex().ft_fp();
    s.add("9072-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_futex();
    let fp0 = p2.ft_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.ft_fp() != fp0;
    s.add("9072-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_futex();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("FUTEX-SEAL");
    s.add("9072-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(FT_UNLOCK, 1).err() == Some(FT_SEALED);
    s.add("9072-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9072-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9072_all_checks_green() {
        let set = run_unx9072_checks();
        assert!(set.all_passed(), "UNX-F9072 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9072_acc_stable() {
        let f = standard_futex();
        assert!(f.chain_ok());
        assert_eq!(f.acc, 45);
        assert_eq!(f.oldsets(), vec![0u64, 10, 13, 47]);
    }
}
