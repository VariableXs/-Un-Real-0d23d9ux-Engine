//! UNX-F9074 glibc dlopen 消费钩子（ELF/mmap 联动）（0x62900 段）
//!
//! 锚点：UNX-F9074-J1 钩子账（dlopen/mmap 联动四命令全落账 + oldset 回
//! 读链 + 渲染导出面）；J2 钩子语义（四态折叠核 + 钩子命令号→折叠臂映射
//! + 终值守恒）；J3 封账印（钩子指纹 + 封印后只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B14（glibc 消费面）。
//!
//! 结构：64 位钩子账本 + 四态命令分派核（非法命令拒 + 参数越界拒）+
//! oldset 回读链 + 钩子指纹 + 注入核（非法命令/参数越界/印后改账/空账
//! 封印/账链断裂必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62900 独占 0x20）。
pub const SEG_BASE: u32 = 0x62900;
pub const DL_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const DL_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const DL_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const DL_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const DL_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// dlopen/mmap 联动四命令闭集。
pub const DL_OPEN: u32 = 0; // 装载登记（累加臂）
pub const DL_RELOC: u32 = 1; // 重定位搅动（异或臂）
pub const DL_MMAP_SEG: u32 = 2; // 段映射并入（位并入臂）
pub const DL_UNMAP: u32 = 3; // 卸载清账（位清除臂）
pub const DL_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn dl_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        DL_OPEN => "OPEN",
        DL_RELOC => "RELOC",
        DL_MMAP_SEG => "MMAP",
        DL_UNMAP => "UNMAP",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn dl_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        DL_OPEN => prev.wrapping_add(arg),
        DL_RELOC => prev ^ arg,
        DL_MMAP_SEG => prev | arg,
        DL_UNMAP => prev & !arg,
        _ => prev,
    }
}

/// 单笔钩子账。
#[derive(Clone)]
pub struct DlOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl DlOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            dl_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// glibc dlopen 钩子账本。
pub struct DlopenHook {
    ops: Vec<DlOp>,
    acc: u64,
    sealed: bool,
}

impl DlopenHook {
    pub fn new() -> Self {
        DlopenHook {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次钩子命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(DL_SEALED);
        }
        if cmd >= DL_CMD_COUNT {
            return Err(DL_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(DL_ARG_OOB);
        }
        let old = self.acc;
        let new = dl_fold(old, cmd, arg);
        self.ops.push(DlOp {
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
            if op.new_acc != dl_fold(op.old_acc, op.cmd, op.arg) {
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

    /// 钩子指纹。
    pub fn dl_fp(&self) -> u32 {
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
        out.push_str(&format!("DLOPEN-ACC|{}\n", self.acc));
        out.push_str(&format!("DLOPEN-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("DLOPEN-SEAL\n");
        }
        out
    }
}

impl Default for DlopenHook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（OPEN[3]→OPEN[11]→RELOC[21]→MMAP[6]）。
/// 手验：0→3→14→27→31（14^21=27，27|6=31）。
fn standard_hook() -> DlopenHook {
    let mut d = DlopenHook::new();
    let _ = d.apply(DL_OPEN, 3); // old 0  → new 3
    let _ = d.apply(DL_OPEN, 11); // old 3  → new 14
    let _ = d.apply(DL_RELOC, 21); // old 14 → new 27
    let _ = d.apply(DL_MMAP_SEG, 6); // old 27 → new 31
    d
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令
    let mut d = DlopenHook::new();
    if d.apply(DL_CMD_COUNT, 1).err() == Some(DL_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut d = DlopenHook::new();
    if d.apply(DL_OPEN, 0).err() == Some(DL_ARG_OOB)
        && d.apply(DL_OPEN, 64).err() == Some(DL_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut d = standard_hook();
    let _ = d.seal();
    if d.apply(DL_OPEN, 7).err() == Some(DL_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut d = DlopenHook::new();
    if !d.seal() && !d.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂
    let mut d = standard_hook();
    d.ops[1].old_acc += 1;
    if !d.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 钩子账；J2 钩子语义；J3 封账印。
pub fn run_unx9074_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9074");
    let p = standard_hook();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9074-J1-操作全落账", ok, if ok { "" } else { "钩子账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 3, 14, 27];
    s.add("9074-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("OPEN|3|old0|new3")
        && text.contains("OPEN|11|old3|new14")
        && text.contains("RELOC|21|old14|new27")
        && text.contains("MMAP|6|old27|new31")
        && text.contains("DLOPEN-ACC|31")
        && text.contains("DLOPEN-OPS|4");
    s.add("9074-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 31
    let ok = p.acc == 31;
    s.add("9074-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 OPEN 累加臂（0+3=3）
    let ok = p.ops[0].cmd == DL_OPEN
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 3
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9074-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 累加接力（3+11=14）
    let ok = p.ops[1].old_acc == 3 && p.ops[1].new_acc == 14;
    s.add("9074-J2-累加接力", ok, if ok { "" } else { "接力漂移" });
    // J2-4 RELOC 异或臂（14^21=27）
    let ok = p.ops[2].cmd == DL_RELOC
        && p.ops[2].old_acc == 14
        && p.ops[2].new_acc == 27
        && p.ops[2].new_acc == (p.ops[2].old_acc ^ p.ops[2].arg);
    s.add("9074-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-5 MMAP 并入臂（27|6=31）
    let ok = p.ops[3].cmd == DL_MMAP_SEG
        && p.ops[3].old_acc == 27
        && p.ops[3].new_acc == 31
        && p.ops[3].new_acc == (p.ops[3].old_acc | p.ops[3].arg);
    s.add("9074-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-6 UNMAP 清除臂（63&!21=42）
    let ok = dl_fold(63, DL_UNMAP, 21) == 42;
    s.add("9074-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J3-1 指纹确定
    let ok = standard_hook().dl_fp() == standard_hook().dl_fp();
    s.add("9074-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_hook();
    let fp0 = p2.dl_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.dl_fp() != fp0;
    s.add("9074-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_hook();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("DLOPEN-SEAL");
    s.add("9074-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(DL_UNMAP, 1).err() == Some(DL_SEALED);
    s.add("9074-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9074-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9074_all_checks_green() {
        let set = run_unx9074_checks();
        assert!(set.all_passed(), "UNX-F9074 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9074_acc_stable() {
        let d = standard_hook();
        assert!(d.chain_ok());
        assert_eq!(d.acc, 31);
        assert_eq!(d.oldsets(), vec![0u64, 3, 14, 27]);
    }
}
