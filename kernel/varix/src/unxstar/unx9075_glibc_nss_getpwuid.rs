//! UNX-F9075 glibc NSS/getpwuid 消费钩子（文件族联动）（0x62920 段）
//!
//! 锚点：UNX-F9075-J1 钩子账（NSS/getpwuid 文件族联动四命令全落账 +
//! oldset 回读链 + 渲染导出面）；J2 钩子语义（四态折叠核 + 钩子命令号→
//! 折叠臂映射 + 终值守恒）；J3 封账印（钩子指纹 + 封印后只读 + 注入核）
//! 三判据全绿。
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

/// 诊断段（0x62920 独占 0x20）。
pub const SEG_BASE: u32 = 0x62920;
pub const NS_BAD_CMD: u32 = SEG_BASE + 0x01; // 非法命令（非四态之一）
pub const NS_ARG_OOB: u32 = SEG_BASE + 0x02; // 参数越界（0 或 >63）
pub const NS_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const NS_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印
pub const NS_CHAIN: u32 = SEG_BASE + 0x05; // 账链断裂（回读不守恒）

/// NSS/getpwuid 联动四命令闭集。
pub const NS_LOOKUP: u32 = 0; // 条目查表（累加臂）
pub const NS_PARSE: u32 = 1; // 行解析搅动（异或臂）
pub const NS_CACHE: u32 = 2; // 缓存并入（位并入臂）
pub const NS_EVICT: u32 = 3; // 逐出清账（位清除臂）
pub const NS_CMD_COUNT: u32 = 4;

/// 命令名渲染。
pub fn ns_cmd_name(cmd: u32) -> &'static str {
    match cmd {
        NS_LOOKUP => "LOOKUP",
        NS_PARSE => "PARSE",
        NS_CACHE => "CACHE",
        NS_EVICT => "EVICT",
        _ => "BADCMD",
    }
}

/// 四态折叠核：累加 / 异或搅动 / 位并入 / 位清除。
pub fn ns_fold(prev: u64, cmd: u32, arg: u64) -> u64 {
    match cmd {
        NS_LOOKUP => prev.wrapping_add(arg),
        NS_PARSE => prev ^ arg,
        NS_CACHE => prev | arg,
        NS_EVICT => prev & !arg,
        _ => prev,
    }
}

/// 单笔钩子账。
#[derive(Clone)]
pub struct NssOp {
    pub cmd: u32,
    pub arg: u64,
    pub old_acc: u64,
    pub new_acc: u64,
}

impl NssOp {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            ns_cmd_name(self.cmd),
            self.arg,
            self.old_acc,
            self.new_acc
        )
    }
}

/// glibc NSS 消费钩子账本。
pub struct NssHook {
    ops: Vec<NssOp>,
    acc: u64,
    sealed: bool,
}

impl NssHook {
    pub fn new() -> Self {
        NssHook {
            ops: Vec::new(),
            acc: 0,
            sealed: false,
        }
    }

    /// 应用一次钩子命令：四态分派 + 非法命令拒 + 参数越界拒 + 印后拒。
    pub fn apply(&mut self, cmd: u32, arg: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(NS_SEALED);
        }
        if cmd >= NS_CMD_COUNT {
            return Err(NS_BAD_CMD);
        }
        if arg == 0 || arg > 63 {
            return Err(NS_ARG_OOB);
        }
        let old = self.acc;
        let new = ns_fold(old, cmd, arg);
        self.ops.push(NssOp {
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
            if op.new_acc != ns_fold(op.old_acc, op.cmd, op.arg) {
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
    pub fn ns_fp(&self) -> u32 {
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
        out.push_str(&format!("NSSPW-ACC|{}\n", self.acc));
        out.push_str(&format!("NSSPW-OPS|{}\n", self.ops.len()));
        if self.sealed {
            out.push_str("NSSPW-SEAL\n");
        }
        out
    }
}

impl Default for NssHook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四命令连发（LOOKUP[10]→PARSE[7]→CACHE[34]→EVICT[2]）。
/// 手验：0→10→13→47→45（10^7=13，13|34=47，47&!2=45）。
fn standard_nss() -> NssHook {
    let mut n = NssHook::new();
    let _ = n.apply(NS_LOOKUP, 10); // old 0  → new 10
    let _ = n.apply(NS_PARSE, 7); // old 10 → new 13
    let _ = n.apply(NS_CACHE, 34); // old 13 → new 47
    let _ = n.apply(NS_EVICT, 2); // old 47 → new 45
    n
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法命令
    let mut n = NssHook::new();
    if n.apply(NS_CMD_COUNT, 1).err() == Some(NS_BAD_CMD) {
        caught += 1;
    }
    // 注 2：参数越界（0 与 64 双向）
    let mut n = NssHook::new();
    if n.apply(NS_LOOKUP, 0).err() == Some(NS_ARG_OOB)
        && n.apply(NS_LOOKUP, 64).err() == Some(NS_ARG_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut n = standard_nss();
    let _ = n.seal();
    if n.apply(NS_LOOKUP, 7).err() == Some(NS_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut n = NssHook::new();
    if !n.seal() && !n.sealed() {
        caught += 1;
    }
    // 注 5：账链断裂
    let mut n = standard_nss();
    n.ops[1].old_acc += 1;
    if !n.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 钩子账；J2 钩子语义；J3 封账印。
pub fn run_unx9075_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9075");
    let p = standard_nss();
    // J1-1 四命令全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9075-J1-操作全落账", ok, if ok { "" } else { "钩子账不全" });
    // J1-2 oldset 逐笔回读
    let ok = p.oldsets() == vec![0u64, 10, 13, 47];
    s.add("9075-J1-oldset回读", ok, if ok { "" } else { "回读链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("LOOKUP|10|old0|new10")
        && text.contains("PARSE|7|old10|new13")
        && text.contains("CACHE|34|old13|new47")
        && text.contains("EVICT|2|old47|new45")
        && text.contains("NSSPW-ACC|45")
        && text.contains("NSSPW-OPS|4");
    s.add("9075-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终值 = 45
    let ok = p.acc == 45;
    s.add("9075-J2-终值", ok, if ok { "" } else { "终值漂移" });
    // J2-2 LOOKUP 累加臂（0+10=10）
    let ok = p.ops[0].cmd == NS_LOOKUP
        && p.ops[0].old_acc == 0
        && p.ops[0].new_acc == 10
        && p.ops[0].new_acc == p.ops[0].old_acc.wrapping_add(p.ops[0].arg);
    s.add("9075-J2-累加臂", ok, if ok { "" } else { "累加臂漂移" });
    // J2-3 PARSE 异或臂（10^7=13）
    let ok = p.ops[1].cmd == NS_PARSE
        && p.ops[1].old_acc == 10
        && p.ops[1].new_acc == 13
        && p.ops[1].new_acc == (p.ops[1].old_acc ^ p.ops[1].arg);
    s.add("9075-J2-异或臂", ok, if ok { "" } else { "异或臂漂移" });
    // J2-4 CACHE 并入臂（13|34=47）
    let ok = p.ops[2].cmd == NS_CACHE
        && p.ops[2].old_acc == 13
        && p.ops[2].new_acc == 47
        && p.ops[2].new_acc == (p.ops[2].old_acc | p.ops[2].arg);
    s.add("9075-J2-并入臂", ok, if ok { "" } else { "并入臂漂移" });
    // J2-5 EVICT 清除臂（47&!2=45）
    let ok = p.ops[3].cmd == NS_EVICT
        && p.ops[3].old_acc == 47
        && p.ops[3].new_acc == 45
        && p.ops[3].new_acc == (p.ops[3].old_acc & !p.ops[3].arg);
    s.add("9075-J2-清除臂", ok, if ok { "" } else { "清除臂漂移" });
    // J2-6 折叠核补臂（255&!15=240）
    let ok = ns_fold(255, NS_EVICT, 15) == 240;
    s.add("9075-J2-折叠补臂", ok, if ok { "" } else { "补臂漂移" });
    // J3-1 指纹确定
    let ok = standard_nss().ns_fp() == standard_nss().ns_fp();
    s.add("9075-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_nss();
    let fp0 = p2.ns_fp();
    p2.ops[0].new_acc += 1;
    let ok = p2.ns_fp() != fp0;
    s.add("9075-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_nss();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("NSSPW-SEAL");
    s.add("9075-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(NS_EVICT, 1).err() == Some(NS_SEALED);
    s.add("9075-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9075-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9075_all_checks_green() {
        let set = run_unx9075_checks();
        assert!(set.all_passed(), "UNX-F9075 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9075_acc_stable() {
        let n = standard_nss();
        assert!(n.chain_ok());
        assert_eq!(n.acc, 45);
        assert_eq!(n.oldsets(), vec![0u64, 10, 13, 47]);
    }
}
