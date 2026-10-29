//! UNX-F9111 fcntl F_SETLKW 阻塞等待与 EINTR 重启联动（0x62DA0 段）
//!
//! 锚点：UNX-F9111-J1 账面（四命令全落账 + oldset 回读链 + 渲染导出
//! 面）；J2 语义（四态折叠核 + 命令号→折叠臂映射 + 终值守恒）；J3 封账
//! 印（指纹 + 封印后只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B16（fcntl/ioctl 面）。
//!
//! 结构：64 位账本 + 四态命令分派核（非法命令拒 + 参数越界拒）+ oldset
//! 回读链 + 账面指纹 + 注入核（非法命令/参数越界/印后改账/空账封印/
//! 账链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62DA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x62DA0;
pub const KW_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const KW_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const KW_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const KW_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const KW_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// 四命令闭集。
pub const KW_CMD0: u32 = 0; // SETLKW（累加臂）
pub const KW_CMD1: u32 = 1; // EINTR（异或搅动臂）
pub const KW_CMD2: u32 = 2; // RSTART（位并入臂）
pub const KW_CMD3: u32 = 3; // CLRKW（位清除臂）
pub const KW_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn kw_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        KW_CMD0 => "SETLKW",
        KW_CMD1 => "EINTR",
        KW_CMD2 => "RSTART",
        KW_CMD3 => "CLRKW",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn kw_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        KW_CMD0 => prev.wrapping_add(arg),
        KW_CMD1 => prev ^ arg,
        KW_CMD2 => prev | arg,
        KW_CMD3 => prev & !arg,
        _ => prev,
    }
}

/// 扩展探针：位域往返守恒（并入后清除必回原值）。
pub fn kw_roundtrip(prev: u64, bits: u64) -> bool {
    kw_fold(kw_fold(prev, KW_CMD2, bits), KW_CMD3, bits) == prev
}

/// 单笔账。
#[derive(Clone)]
pub struct SetlkwOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl SetlkwOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            kw_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// UNX-F9111 账本。
pub struct SetlkwLedger {
    ops: Vec<SetlkwOp>,
    acc: u64,
    sealed: bool,
}

impl SetlkwLedger {
    pub fn new() -> Self {
        SetlkwLedger {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    /// Ok(old) 即 oldset 回读。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(KW_SEALED);
        }
        if cmd >= KW_CMD_COUNT {
            return Err(KW_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(KW_ARG_OOB);
        }
        let old = self.acc;
        let new = kw_fold(old, cmd, arg);
        self.ops.push(SetlkwOp {
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
            if op.new_acc != kw_fold(op.old_acc, op.cmd, op.arg) {
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
    pub fn kw_fp(&self) -> u32 {
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
        out.push_str(&format!("BLOCKW-ACC|{}\n", self.acc));
        out.push_str(&format!("BLOCKW-PEAK|{}\n", self.peak_acc()));
        out.push_str(&format!("BLOCKW-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("BLOCKW-SEAL\n");
        }
        out
    }
}

impl Default for SetlkwLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（SETLKW[12]→SETLKW[9]→EINTR[31]→RSTART[5]）。
/// 手验：0→12→21→10→15（21^31=10，10|5=15）。
fn standard_ledger() -> SetlkwLedger {
    let mut l = SetlkwLedger::new();
    let _ = l.apply(KW_CMD0, 12); // old 0    → new 12
    let _ = l.apply(KW_CMD0, 9); // old 12   → new 21
    let _ = l.apply(KW_CMD1, 31); // old 21  → new 10
    let _ = l.apply(KW_CMD2, 5); // old 10  → new 15
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令（CMD_COUNT 越界）
    let mut l = SetlkwLedger::new();
    if l.apply(KW_CMD_COUNT, 1).err() == Some(KW_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut l = SetlkwLedger::new();
    if l.apply(KW_CMD0, 0).err() == Some(KW_ARG_OOB)
        && l.apply(KW_CMD0, 64).err() == Some(KW_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.apply(KW_CMD0, 7).err() == Some(KW_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = SetlkwLedger::new();
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
pub fn run_unx9111_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9111");
    let p = standard_ledger();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9111-J1-操作全落账", ok, if ok { "" } else { "账面不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 12, 21, 10];
    s.add("9111-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("SETLKW|12|old0|new12")
        && text.contains("SETLKW|9|old12|new21")
        && text.contains("EINTR|31|old21|new10")
        && text.contains("RSTART|5|old10|new15")
        && text.contains("BLOCKW-ACC|15")
        && text.contains("BLOCKW-PEAK|21")
        && text.contains("BLOCKW-OPS|4");
    s.add("9111-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 15
    let ok = p.acc == 15;
    s.add("9111-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 CMD0 累加臂（0+12=12）
    let ok = p.ops[0].cmd == KW_CMD0
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 12
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9111-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 累加接力（12+9=21）
    let ok = p.ops[1].old_acc == 12 && p.ops[1].new_acc == 21;
    s.add("9111-J2-累加接力", ok, if ok { "" } else { "接力漂移" });
    // J2-4 CMD1 异或臂（21^31=10）
    let ok = p.ops[2].cmd == KW_CMD1
        && p.ops[2].old_acc == 21
        && p.ops[2].new_acc == 10
        && p.ops[2].new_acc == (p.ops[2].old_acc ^ p.ops[2].arg);
    s.add("9111-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-5 CMD2 并入臂（10|5=15）
    let ok = p.ops[3].cmd == KW_CMD2
        && p.ops[3].old_acc == 10
        && p.ops[3].new_acc == 15
        && p.ops[3].new_acc == (p.ops[3].old_acc | p.ops[3].arg);
    s.add("9111-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-6 CMD3 清除臂（63&!21=42）
    let ok = kw_fold(63, KW_CMD3, 21) == 42;
    s.add("9111-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J2-7 峰值回读核（账面链上最大 new_acc）
    let ok = p.peak_acc() == 21;
    s.add("9111-J2-峰值回读", ok, if ok { "" } else { "峰值漂移" });
    // J2-8 均值定点核（oldset 均值 ×100 定点两位）
    let ok = p.mean_acc_x100() == 1075;
    s.add("9111-J2-均值定点", ok, if ok { "" } else { "均值漂移" });
    // J2-9 折叠幂等探针（同参数重放复现同值——折叠核确定性）
    let ok = kw_fold(p.ops[2].old_acc, KW_CMD1, p.ops[2].arg) == p.ops[2].new_acc
        && kw_fold(p.ops[3].old_acc, KW_CMD2, p.ops[3].arg) == p.ops[3].new_acc;
    s.add("9111-J2-折叠幂等", ok, if ok { "" } else { "幂等漂移" });
    // J2-10 零元探针（最小合法 arg 的折叠推进）
    let ok = kw_fold(0, KW_CMD0, 1) == 1 && kw_fold(1, KW_CMD1, 1) == 0;
    s.add("9111-J2-零元探针", ok, if ok { "" } else { "零元漂移" });
    // J3-5 封印位入签（封印后指纹必漂移）
    let mut p5 = standard_ledger();
    let fp_pre = p5.kw_fp();
    let _ = p5.seal();
    let ok = p5.kw_fp() != fp_pre;
    s.add("9111-J3-封印位入签", ok, if ok { "" } else { "封印位未入签" });
    // J3-6 重封拒
    let mut p6 = standard_ledger();
    let first = p6.seal();
    let ok = first && !p6.seal();
    s.add("9111-J3-重封拒", ok, if ok { "" } else { "重封闸失效" });
    // J2-13 折叠臂封闭性（四臂互异：同账不同臂结果两两可分）
    let ok = kw_fold(12, KW_CMD0, 10) == 22
        && kw_fold(12, KW_CMD1, 10) == 6
        && kw_fold(12, KW_CMD2, 10) == 14
        && kw_fold(12, KW_CMD3, 10) == 4;
    s.add("9111-J2-臂封闭性", ok, if ok { "" } else { "臂封闭漂移" });
    // J2-14 大参守恒（arg=63 上界折叠不越 64 位账面）
    let ok = kw_fold(63, KW_CMD1, 63) == 0 && kw_fold(0, KW_CMD2, 63) == 63;
    s.add("9111-J2-大参守恒", ok, if ok { "" } else { "上界漂移" });
    // J3-8 空账渲染无印面（未封印不含 SEAL 行）
    let ok = !SetlkwLedger::new().render().contains("BLOCKW-SEAL");
    s.add("9111-J3-空账无印", ok, if ok { "" } else { "印面泄漏" });
    // J2-11 位域往返探针（并入后清除必回原值——位账守恒闭环）
    let ok = kw_roundtrip(23, 40) && kw_roundtrip(0, 21);
    s.add("9111-J2-位域往返", ok, if ok { "" } else { "往返漂移" });
    // J2-12 账目链顺读（oldsets 与 ops 逐笔 old_acc 一致）
    let ok = p
        .oldsets()
        .iter()
        .enumerate()
        .all(|(i, v)| p.ops[i].old_acc == *v);
    s.add("9111-J2-账目链顺读", ok, if ok { "" } else { "顺读漂移" });
    // J3-7 封印渲染只读（印面在场 + 印后 apply 拒）
    let mut p7 = standard_ledger();
    let _ = p7.seal();
    let ok = p7.render().contains("BLOCKW-SEAL")
        && p7.apply(KW_CMD0, 1).err() == Some(KW_SEALED);
    s.add("9111-J3-封印渲染只读", ok, if ok { "" } else { "封印渲染漂移" });
    // J3-9 印面指纹回写一致（封印位只入签一次）
    let mut p9 = standard_ledger();
    let _ = p9.seal();
    let fp9 = p9.kw_fp();
    let _ = p9.seal();
    let ok = p9.kw_fp() == fp9;
    s.add("9111-J3-印面单次入签", ok, if ok { "" } else { "印面重复入签" });
    // J3-1 指纹确定
    let ok = standard_ledger().kw_fp() == standard_ledger().kw_fp();
    s.add("9111-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_ledger();
    let fp0 = p2.kw_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.kw_fp() != fp0;
    s.add("9111-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_ledger();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("BLOCKW-SEAL");
    s.add("9111-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(KW_CMD3, 1).err() == Some(KW_SEALED);
    s.add("9111-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9111-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9111_all_checks_green() {
        let set = run_unx9111_checks();
        assert!(set.all_passed(), "UNX-F9111 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9111_acc_stable() {
        let l = standard_ledger();
        assert!(l.chain_ok());
        assert_eq!(l.acc, 15);
        assert_eq!(l.oldsets(), vec![0u64, 12, 21, 10]);
    }
}
