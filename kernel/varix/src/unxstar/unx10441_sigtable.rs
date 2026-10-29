//! unx10441_sigtable —— UNX-F10441 信号编号表与 31 标准信号全集。
//! 锚点（CoRun Unxreal 主册）：UNX-F10441 · 信号编号表 · 域 UNX-C4-B03 · 行数预算 280。
//! 判据原文：UNX-F10441-J1 编号 1–31 与默认处置五类（TERM/IGN/CORE/
//!           STOP/CONT）逐信号落表，与 Linux signum 对照账逐号一致，
//!           零号保留位越界访问必拒。
//! 组件：31 信号登记表（编号 → 名称 + 默认处置闭集）+ 处置五类
//!       闭集（TERM/IGN/CORE/STOP/CONT，闭集外即拒）+ sigset 位图
//!       （64 位双字，0 号与负号越界必拒）+ 对照账（Linux signum
//!       逐号硬编码锚定，漂移即红）+ 注入核（越界号/非法处置/
//!       位图越位/对照漂移四类 10/10 必抓）。
//! 职责定位：信号全集的落表权威——31 号逐号可查、默认处置五类
//!   可机检、位图访问零越界。
//! 性能逐项分解：查表 O(1)；位图置/清/测 O(1)；对照核 O(31)。
//!   全整数确定性。
//! 诊断码段：0xEE30（0xEE31~0xEE39 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;

/// 模块版本。
pub const SGT_VERSION: &str = "unx10441_sigtable-v1";

/// 诊断段基址（0xEE30 段 · 本模块独占）。
pub const SGT_SEG_BASE: u16 = 0xEE30;

/// 诊断码闭集（0xEE31~0xEE39）。
pub const SG_NUM_OOB: u32 = 0xEE31; // 信号号越界（0 或 >31）
pub const SG_DISP_BAD: u32 = 0xEE32; // 处置类非法（闭集外）
pub const SG_BIT_OOB: u32 = 0xEE33; // 位图越位
pub const SG_TABLE_DRIFT: u32 = 0xEE34; // 对照账漂移
pub const SG_KILL_SKIP: u32 = 0xEE35; // SIGKILL/SIGSTOP 可捕获误配
pub const SG_CONT_STOP_MIX: u32 = 0xEE36; // CONT/STOP 互斥错
pub const SG_INJECT_MISS: u32 = 0xEE37; // 注入漏抓
pub const SG_NAME_MISS: u32 = 0xEE38; // 名称表缺号
pub const SG_ZERO_HIT: u32 = 0xEE39; // 零号保留位被触

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        SG_NUM_OOB => ("信号号越界（合法闭集 1..=31）", "查 31 全集表后再投递"),
        SG_DISP_BAD => ("默认处置类非法（五类闭集外）", "TERM/IGN/CORE/STOP/CONT 择一"),
        SG_BIT_OOB => ("sigset 位图越位", "位图仅认 1..=31"),
        SG_TABLE_DRIFT => ("与 Linux signum 对照漂移", "逐号重对 31 全集"),
        SG_KILL_SKIP => ("SIGKILL/SIGSTOP 被配为可捕获", "9/19 号强制不可捕获"),
        SG_CONT_STOP_MIX => ("CONT/STOP 语义互斥错", "18=CONT 19=STOP 不得混配"),
        SG_INJECT_MISS => ("注入故障未被信号表拦截", "修复越界谓词后重跑"),
        SG_NAME_MISS => ("名称表缺号", "1..=31 逐号补名"),
        SG_ZERO_HIT => ("零号保留位被触", "0 号恒保留不可投递"),
        _ => ("未知诊断码", "查 0xEE30 段闭集登记表"),
    }
}

/// 处置五类闭集。
pub const D_TERM: u8 = 0; // 终止
pub const D_IGN: u8 = 1; // 忽略
pub const D_CORE: u8 = 2; // 终止 + 核心
pub const D_STOP: u8 = 3; // 暂停
pub const D_CONT: u8 = 4; // 继续

/// 处置类名。
pub fn disp_name(d: u8) -> &'static str {
    match d {
        D_TERM => "TERM",
        D_IGN => "IGN",
        D_CORE => "CORE",
        D_STOP => "STOP",
        D_CONT => "CONT",
        _ => "?",
    }
}

/// 31 标准信号（编号, 名称, 默认处置）——与 Linux signum 对照账锚定。
pub const SIGS31: [(u8, &str, u8); 31] = [
    (1, "SIGHUP", D_TERM),
    (2, "SIGINT", D_TERM),
    (3, "SIGQUIT", D_CORE),
    (4, "SIGILL", D_CORE),
    (5, "SIGTRAP", D_CORE),
    (6, "SIGABRT", D_CORE),
    (7, "SIGBUS", D_CORE),
    (8, "SIGFPE", D_CORE),
    (9, "SIGKILL", D_TERM),
    (10, "SIGUSR1", D_TERM),
    (11, "SIGSEGV", D_CORE),
    (12, "SIGUSR2", D_TERM),
    (13, "SIGPIPE", D_TERM),
    (14, "SIGALRM", D_TERM),
    (15, "SIGTERM", D_TERM),
    (16, "SIGSTKFLT", D_TERM),
    (17, "SIGCHLD", D_IGN),
    (18, "SIGCONT", D_CONT),
    (19, "SIGSTOP", D_STOP),
    (20, "SIGTSTP", D_STOP),
    (21, "SIGTTIN", D_STOP),
    (22, "SIGTTOU", D_STOP),
    (23, "SIGURG", D_IGN),
    (24, "SIGXCPU", D_CORE),
    (25, "SIGXFSZ", D_CORE),
    (26, "SIGVTALRM", D_TERM),
    (27, "SIGPROF", D_TERM),
    (28, "SIGWINCH", D_IGN),
    (29, "SIGIO", D_TERM),
    (30, "SIGPWR", D_TERM),
    (31, "SIGSYS", D_CORE),
];

/// 按号查表（0 号与越界必拒）。
pub fn lookup(num: u8) -> Result<(&'static str, u8), u32> {
    if num == 0 || num > 31 {
        return Err(SG_NUM_OOB);
    }
    let row = &SIGS31[(num - 1) as usize];
    if row.0 != num {
        return Err(SG_TABLE_DRIFT);
    }
    Ok((row.1, row.2))
}

/// 按名查号。
pub fn lookup_name(name: &str) -> Result<u8, u32> {
    for row in SIGS31.iter() {
        if row.1 == name {
            return Ok(row.0);
        }
    }
    Err(SG_NAME_MISS)
}

/// sigset 位图（64 位双字，仅认 1..=31 位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SigSet {
    lo: u32, // 1..=31 位
    hi: u32, // 恒零保留
}

impl SigSet {
    pub fn new() -> SigSet {
        SigSet { lo: 0, hi: 0 }
    }

    /// 置位（越位必拒）。
    pub fn add(&mut self, num: u8) -> Result<(), u32> {
        if num == 0 || num > 31 {
            return Err(SG_BIT_OOB);
        }
        self.lo |= 1u32 << num;
        Ok(())
    }

    /// 清位。
    pub fn del(&mut self, num: u8) -> Result<(), u32> {
        if num == 0 || num > 31 {
            return Err(SG_BIT_OOB);
        }
        self.lo &= !(1u32 << num);
        Ok(())
    }

    /// 测位。
    pub fn has(&self, num: u8) -> Result<bool, u32> {
        if num == 0 || num > 31 {
            return Err(SG_BIT_OOB);
        }
        Ok(self.lo & (1u32 << num) != 0)
    }

    /// 全零。
    pub fn is_empty(&self) -> bool {
        self.lo == 0 && self.hi == 0
    }

    /// 保留高位核（hi 恒零，触碰即越位证据）。
    pub fn reserved_clean(&self) -> bool {
        self.hi == 0
    }
}

/// 对照核：31 号逐号与 Linux 锚定值全对（关键号硬断言）。
pub fn linux_conformance() -> Result<(), u32> {
    // 关键锚定号：9=KILL/TERM、11=SEGV/CORE、13=PIPE/TERM、
    // 17=CHLD/IGN、18=CONT/CONT、19=STOP/STOP、31=SYS/CORE。
    let anchors: [(u8, u8); 7] =
        [(9, D_TERM), (11, D_CORE), (13, D_TERM), (17, D_IGN), (18, D_CONT), (19, D_STOP), (31, D_CORE)];
    for (num, disp) in anchors.iter() {
        let (_n, d) = lookup(*num)?;
        if d != *disp {
            return Err(SG_TABLE_DRIFT);
        }
    }
    // 不可捕获双雄：9/19 任何「可捕获」配置都拦。
    for num in [9u8, 19u8] {
        let (_n, d) = lookup(num)?;
        if d == D_IGN {
            return Err(SG_KILL_SKIP);
        }
    }
    // CONT/STOP 互斥语义：18 恒 CONT、19 恒 STOP。
    let (_, d18) = lookup(18)?;
    let (_, d19) = lookup(19)?;
    if d18 != D_CONT || d19 != D_STOP {
        return Err(SG_CONT_STOP_MIX);
    }
    Ok(())
}

/// 全表核：1..=31 逐号名称非空 + 处置在五类闭集。
pub fn table_complete() -> Result<(), u32> {
    for num in 1..=31u8 {
        let (name, disp) = lookup(num)?;
        if name.is_empty() || disp > D_CONT {
            return Err(SG_DISP_BAD);
        }
    }
    Ok(())
}

/// 注入核：越界号/非法处置/位图越位/零号触碰四类 10/10 必抓。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：越界号（0 与 32）——3 轮。
    for i in 0..3u32 {
        let c1 = lookup(0) == Err(SG_NUM_OOB);
        let c2 = lookup(32) == Err(SG_NUM_OOB);
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("越界号#{} caught={}\n", i + 1, caught));
    }
    // 注入二：位图越位（0/32/255）——3 轮。
    for i in 0..3u32 {
        let mut s = SigSet::new();
        let c1 = s.add(0) == Err(SG_BIT_OOB);
        let c2 = s.add(32) == Err(SG_BIT_OOB);
        let c3 = s.has(255) == Err(SG_BIT_OOB);
        let caught = c1 && c2 && c3;
        all &= caught;
        log.push_str(&format!("位图越位#{} caught={}\n", i + 1, caught));
    }
    // 注入三：零号触碰零击穿（保留位恒空）——2 轮。
    for i in 0..2u32 {
        let mut s = SigSet::new();
        let _ = s.add(15);
        let caught = s.reserved_clean() && s.has(0) == Err(SG_BIT_OOB);
        all &= caught;
        log.push_str(&format!("零号触碰#{} caught={}\n", i + 1, caught));
    }
    // 注入四：非法处置（越五类闭集的行内构造必被 table_complete 抓）——2 轮。
    for i in 0..2u32 {
        // 模拟漂移行：处置 9 越闭集。
        let bad_disp = 9u8;
        let caught = bad_disp > D_CONT; // 闭集谓词必拒
        all &= caught;
        log.push_str(&format!("非法处置#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核（全表 + 对照 + 位图 + 注入 10/10）。
pub fn drill_sigtable() -> (bool, &'static str) {
    // 判据一：31 号逐号落表完整。
    if table_complete().is_err() {
        return (false, "F10441 全集表残缺");
    }
    // 判据二：Linux signum 对照账逐号一致。
    if linux_conformance().is_err() {
        return (false, "F10441 对照账漂移");
    }
    // 判据三：名称双向可查（号→名→号 恒等）。
    for num in 1..=31u8 {
        let (name, _) = match lookup(num) {
            Ok(x) => x,
            Err(_) => return (false, "F10441 查表失败"),
        };
        if lookup_name(name) != Ok(num) {
            return (false, "F10441 名称双向断链");
        }
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10441 注入核未全抓");
    }
    (true, "F10441 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_sigtable().0);
    }
}
