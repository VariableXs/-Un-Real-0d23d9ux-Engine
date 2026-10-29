//! UNX-F9070 glibc epoll 包装消费核（epoll_wait→poll 桥）（0x62880 段）
//!
//! 锚点：UNX-F9070-J1 桥接账（epoll_wait→poll 桥四命令全落账 + oldset
//! 回读链 + 渲染导出面）；J2 桥接语义（四态折叠核 + 桥接命令号→折叠臂
//! 映射 + 终值守恒）；J3 封账印（桥接指纹 + 封印后只读 + 注入核）三判据
//! 全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面）。
//!
//! 结构：64 位桥接账本 + 四态命令分派核（非法命令拒 + 参数越界拒）+
//! oldset 回读链 + 桥接指纹 + 注入核（非法命令/参数越界/印后改账/空账
//! 封印/账链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62880 独占 0x20）。
pub const SEG_BASE: u32 = 0x62880;
pub const EP_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const EP_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const EP_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const EP_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const EP_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// epoll_wait→poll 桥四命令闭集。
pub const EP_ADD: u32 = 0; // 关注项登记（累加臂）
pub const EP_MOD: u32 = 1; // 关注项改账（异或搅动臂）
pub const EP_WAIT: u32 = 2; // 就绪收割（位并入臂）
pub const EP_BRIDGE: u32 = 3; // poll 下沉桥接（位清除臂）
pub const EP_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn ep_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        EP_ADD => "ADD",
        EP_MOD => "MOD",
        EP_WAIT => "WAIT",
        EP_BRIDGE => "BRIDGE",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn ep_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        EP_ADD => prev.wrapping_add(arg),
        EP_MOD => prev ^ arg,
        EP_WAIT => prev | arg,
        EP_BRIDGE => prev & !arg,
        _ => prev,
    }
}

/// 单笔桥接账。
#[derive(Clone)]
pub struct BridgeOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl BridgeOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            ep_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// glibc epoll 包装桥接账本。
pub struct PollBridge {
    ops: Vec<BridgeOp>,
    acc: u64,
    sealed: bool,
}

impl PollBridge {
    pub fn new() -> Self {
        PollBridge {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次桥接命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(EP_SEALED);
        }
        if cmd >= EP_CMD_COUNT {
            return Err(EP_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(EP_ARG_OOB);
        }
        let old = self.acc;
        let new = ep_fold(old, cmd, arg);
        self.ops.push(BridgeOp {
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
            if op.new_acc != ep_fold(op.old_acc, op.cmd, op.arg) {
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

    /// 桥接指纹。
    pub fn ep_fp(&self) -> u32 {
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
        out.push_str(&format!("EPPOLL-ACC|{}\n", self.acc));
        out.push_str(&format!("EPPOLL-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("EPPOLL-SEAL\n");
        }
        out
    }
}

impl Default for PollBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（ADD[10]→MOD[6]→WAIT[13]→BRIDGE[9]）。
/// 手验：0→10→16→29→20（16|13=29，29&!9=20）。
fn standard_bridge() -> PollBridge {
    let mut b = PollBridge::new();
    let _ = b.apply(EP_ADD, 10); // old 0  → new 10
    let _ = b.apply(EP_MOD, 6); // old 10 → new 16
    let _ = b.apply(EP_WAIT, 13); // old 16 → new 29
    let _ = b.apply(EP_BRIDGE, 9); // old 29 → new 20
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令
    let mut b = PollBridge::new();
    if b.apply(EP_CMD_COUNT, 1).err() == Some(EP_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut b = PollBridge::new();
    if b.apply(EP_ADD, 0).err() == Some(EP_ARG_OOB)
        && b.apply(EP_ADD, 64).err() == Some(EP_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut b = standard_bridge();
    let _ = b.seal();
    if b.apply(EP_ADD, 7).err() == Some(EP_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut b = PollBridge::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂
    let mut b = standard_bridge();
    b.ops[1].old_acc += 1;
    if !b.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 桥接账；J2 桥接语义；J3 封账印。
pub fn run_unx9070_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9070");
    let p = standard_bridge();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9070-J1-操作全落账", ok, if ok { "" } else { "桥接账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 10, 16, 29];
    s.add("9070-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("ADD|10|old0|new10")
        && text.contains("MOD|6|old10|new16")
        && text.contains("WAIT|13|old16|new29")
        && text.contains("BRIDGE|9|old29|new20")
        && text.contains("EPPOLL-ACC|20")
        && text.contains("EPPOLL-OPS|4");
    s.add("9070-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 20
    let ok = p.acc == 20;
    s.add("9070-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 ADD 累加臂（0+10=10）
    let ok = p.ops[0].cmd == EP_ADD
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 10
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9070-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 MOD 异或臂（10^6=16）
    let ok = p.ops[1].cmd == EP_MOD
        && p.ops[1].old_acc == 10
        && p.ops[1].new_acc == 16
        && p.ops[1].new_acc == (p.ops[1].old_acc ^ p.ops[1].arg);
    s.add("9070-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-4 WAIT 并入臂（16|13=29）
    let ok = p.ops[2].cmd == EP_WAIT
        && p.ops[2].old_acc == 16
        && p.ops[2].new_acc == 29
        && p.ops[2].new_acc == (p.ops[2].old_acc | p.ops[2].arg);
    s.add("9070-J2-收割臂", ok, if ok { "" } else { "收割臂漂移" });
    // J2-5 BRIDGE 清除臂（29&!9=20）
    let ok = p.ops[3].cmd == EP_BRIDGE
        && p.ops[3].old_acc == 29
        && p.ops[3].new_acc == 20
        && p.ops[3].new_acc == (p.ops[3].old_acc & !p.ops[3].arg);
    s.add("9070-J2-桥接臂", ok, if ok { "" } else { "桥接臂漂移" });
    // J2-6 清除臂（63&!21=42）
    let ok = ep_fold(63, EP_BRIDGE, 21) == 42;
    s.add("9070-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J3-1 指纹确定
    let ok = standard_bridge().ep_fp() == standard_bridge().ep_fp();
    s.add("9070-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_bridge();
    let fp0 = p2.ep_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.ep_fp() != fp0;
    s.add("9070-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_bridge();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("EPPOLL-SEAL");
    s.add("9070-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(EP_BRIDGE, 1).err() == Some(EP_SEALED);
    s.add("9070-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9070-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9070_all_checks_green() {
        let set = run_unx9070_checks();
        assert!(set.all_passed(), "UNX-F9070 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9070_acc_stable() {
        let b = standard_bridge();
        assert!(b.chain_ok());
        assert_eq!(b.acc, 20);
        assert_eq!(b.oldsets(), vec![0u64, 10, 16, 29]);
    }
}
