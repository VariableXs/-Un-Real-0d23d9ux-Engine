//! UNX-F9076 glibc printf/stdio 消费核（write 缓冲行为）（0x62940 段）
//!
//! 锚点：UNX-F9076-J1 缓冲账（printf/stdio write 缓冲四命令全落账 +
//! oldset 回读链 + 渲染导出面）；J2 缓冲语义（四态折叠核 + 缓冲命令号→
//! 折叠臂映射 + 终值守恒）；J3 封账印（缓冲指纹 + 封印后只读 + 注入核）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面）。
//!
//! 结构：64 位缓冲账本 + 四态命令分派核（非法命令拒 + 参数越界拒）+
//! oldset 回读链 + 缓冲指纹 + 注入核（非法命令/参数越界/印后改账/空账
//! 封印/账链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62940 独占 0x20）。
pub const SEG_BASE: u32 = 0x62940;
pub const ST_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const ST_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const ST_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const ST_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const ST_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// printf/stdio write 缓冲四命令闭集。
pub const ST_WRITE: u32 = 0; // 缓冲写入（累加臂）
pub const ST_FLUSH: u32 = 1; // 冲刷搅动（异或臂）
pub const ST_SEEK: u32 = 2; // 文件位并（位并入臂）
pub const ST_TRUNC: u32 = 3; // 截断清账（位清除臂）
pub const ST_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn st_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        ST_WRITE => "WRITE",
        ST_FLUSH => "FLUSH",
        ST_SEEK => "SEEK",
        ST_TRUNC => "TRUNC",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn st_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        ST_WRITE => prev.wrapping_add(arg),
        ST_FLUSH => prev ^ arg,
        ST_SEEK => prev | arg,
        ST_TRUNC => prev & !arg,
        _ => prev,
    }
}

/// 单笔缓冲账。
#[derive(Clone)]
pub struct StdioOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl StdioOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            st_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// glibc printf/stdio 缓冲账本。
pub struct StdioLedger {
    ops: Vec<StdioOp>,
    acc: u64,
    sealed: bool,
}

impl StdioLedger {
    pub fn new() -> Self {
        StdioLedger {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次缓冲命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(ST_SEALED);
        }
        if cmd >= ST_CMD_COUNT {
            return Err(ST_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(ST_ARG_OOB);
        }
        let old = self.acc;
        let new = st_fold(old, cmd, arg);
        self.ops.push(StdioOp {
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
            if op.new_acc != st_fold(op.old_acc, op.cmd, op.arg) {
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

    /// 缓冲指纹。
    pub fn st_fp(&self) -> u32 {
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
        out.push_str(&format!("STDIO-ACC|{}\n", self.acc));
        out.push_str(&format!("STDIO-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("STDIO-SEAL\n");
        }
        out
    }
}

impl Default for StdioLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（WRITE[14]→FLUSH[9]→SEEK[28]→TRUNC[5]）。
/// 手验：0→14→7→31→26（14^9=7，7|28=31，31&!5=26）。
fn standard_stdio() -> StdioLedger {
    let mut t = StdioLedger::new();
    let _ = t.apply(ST_WRITE, 14); // old 0  → new 14
    let _ = t.apply(ST_FLUSH, 9); // old 14 → new 7
    let _ = t.apply(ST_SEEK, 28); // old 7  → new 31
    let _ = t.apply(ST_TRUNC, 5); // old 31 → new 26
    t
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令
    let mut t = StdioLedger::new();
    if t.apply(ST_CMD_COUNT, 1).err() == Some(ST_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut t = StdioLedger::new();
    if t.apply(ST_WRITE, 0).err() == Some(ST_ARG_OOB)
        && t.apply(ST_WRITE, 64).err() == Some(ST_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut t = standard_stdio();
    let _ = t.seal();
    if t.apply(ST_WRITE, 7).err() == Some(ST_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut t = StdioLedger::new();
    if !t.seal() && !t.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂
    let mut t = standard_stdio();
    t.ops[1].old_acc += 1;
    if !t.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 缓冲账；J2 缓冲语义；J3 封账印。
pub fn run_unx9076_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9076");
    let p = standard_stdio();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9076-J1-操作全落账", ok, if ok { "" } else { "缓冲账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 14, 7, 31];
    s.add("9076-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("WRITE|14|old0|new14")
        && text.contains("FLUSH|9|old14|new7")
        && text.contains("SEEK|28|old7|new31")
        && text.contains("TRUNC|5|old31|new26")
        && text.contains("STDIO-ACC|26")
        && text.contains("STDIO-OPS|4");
    s.add("9076-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 26
    let ok = p.acc == 26;
    s.add("9076-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 WRITE 累加臂（0+14=14）
    let ok = p.ops[0].cmd == ST_WRITE
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 14
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9076-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 FLUSH 异或臂（14^9=7）
    let ok = p.ops[1].cmd == ST_FLUSH
        && p.ops[1].old_acc == 14
        && p.ops[1].new_acc == 7
        && p.ops[1].new_acc == (p.ops[1].old_acc ^ p.ops[1].arg);
    s.add("9076-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-4 SEEK 并入臂（7|28=31）
    let ok = p.ops[2].cmd == ST_SEEK
        && p.ops[2].old_acc == 7
        && p.ops[2].new_acc == 31
        && p.ops[2].new_acc == (p.ops[2].old_acc | p.ops[2].arg);
    s.add("9076-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-5 TRUNC 清除臂（31&!5=26）
    let ok = p.ops[3].cmd == ST_TRUNC
        && p.ops[3].old_acc == 31
        && p.ops[3].new_acc == 26
        && p.ops[3].new_acc == (p.ops[3].old_acc & !p.ops[3].arg);
    s.add("9076-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J2-6 折叠核补臂（255&!15=240）
    let ok = st_fold(255, ST_TRUNC, 15) == 240;
    s.add("9076-J2-折叠补臂", ok, if ok { "" } else { "补臂漂移" });
    // J3-1 指纹确定
    let ok = standard_stdio().st_fp() == standard_stdio().st_fp();
    s.add("9076-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_stdio();
    let fp0 = p2.st_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.st_fp() != fp0;
    s.add("9076-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_stdio();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("STDIO-SEAL");
    s.add("9076-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(ST_TRUNC, 1).err() == Some(ST_SEALED);
    s.add("9076-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9076-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9076_all_checks_green() {
        let set = run_unx9076_checks();
        assert!(set.all_passed(), "UNX-F9076 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9076_acc_stable() {
        let t = standard_stdio();
        assert!(t.chain_ok());
        assert_eq!(t.acc, 26);
        assert_eq!(t.oldsets(), vec![0u64, 14, 7, 31]);
    }
}
