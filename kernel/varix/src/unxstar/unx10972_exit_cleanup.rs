//! UNX-F10972 退出清理联测（异常退出模式恢复保证）（0x1d900 段）
//!
//! 卡片判据（UNX-F10972-J1）：
//! - 异常退出后终端模式恢复（10/10）、资源零泄漏（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模异常退出清理链：应用启动时
//! 登记（模式快照 + fd 集合 + 缓冲池），异常退出时清理器逐项
//! 还原：模式逐位恢复、fd 全关闭、缓冲全释放；泄漏账逐项核销。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10972-10900)*0x20 = 0x1d900）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d900;

/// XC_EBADF：fd 无效
pub const XC_EBADF: u32 = SEG_BASE + 0x01;
/// XC_EINVAL：模式位越界
pub const XC_EINVAL: u32 = SEG_BASE + 0x02;
/// XC_ELEAK：泄漏检出（fd/缓冲未清）
pub const XC_ELEAK: u32 = SEG_BASE + 0x03;
/// XC_EDRIFT：模式恢复漂移（自检面）
pub const XC_EDRIFT: u32 = SEG_BASE + 0x04;
/// XC_ETWICE：重复清理
pub const XC_ETWICE: u32 = SEG_BASE + 0x05;
/// XC_ENOREG：未登记即清理
pub const XC_ENOREG: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 模式位有效掩码
pub const MODE_MASK: u32 = 0x00ff;
/// fd 表容量
const FD_CAP: usize = 8;
/// 缓冲池槽位数
const POOL_SLOTS: usize = 4;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 清理登记条目
#[derive(Clone, Debug, PartialEq)]
pub struct CleanupReg {
    /// 登记时的模式位
    pub saved_flags: u32,
    /// 登记的 fd 集合（true=开启）
    pub fds: Vec<bool>,
    /// 缓冲池占用位
    pub pool: Vec<bool>,
}

impl CleanupReg {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.saved_flags as u64).wrapping_mul(0x100000001b3);
        for f in &self.fds {
            h = (h ^ (*f as u64)).wrapping_mul(0x100000001b3);
        }
        for p in &self.pool {
            h = (h ^ (*p as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 退出清理联测台
#[derive(Clone, Debug)]
pub struct ExitCleanup {
    /// 当前模式位
    pub flags: u32,
    /// fd 开启位表
    pub fds: Vec<bool>,
    /// 缓冲池占用位
    pub pool: Vec<bool>,
    /// 清理登记
    pub reg: Option<CleanupReg>,
    /// 是否已清理
    pub cleaned: bool,
    /// 清理次数
    pub n_cleanup: u32,
    /// 错误计数
    pub n_ebadf: u32,
    pub n_einval: u32,
}

impl ExitCleanup {
    pub fn new() -> Self {
        ExitCleanup {
            flags: 0x0f,
            fds: vec![false; FD_CAP],
            pool: vec![false; POOL_SLOTS],
            reg: None,
            cleaned: false,
            n_cleanup: 0,
            n_ebadf: 0,
            n_einval: 0,
        }
    }

    /// 登记：快照当前模式/fd/池位
    pub fn register(&mut self) -> Result<(), u32> {
        if self.reg.is_some() {
            return Err(XC_ETWICE);
        }
        self.reg = Some(CleanupReg {
            saved_flags: self.flags,
            fds: self.fds.clone(),
            pool: self.pool.clone(),
        });
        Ok(())
    }

    /// 改模式位（运行期污染）
    pub fn set_flags(&mut self, flags: u32) -> Result<(), u32> {
        if flags & !MODE_MASK != 0 {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(XC_EINVAL);
        }
        self.flags = flags;
        Ok(())
    }

    /// 开 fd
    pub fn open_fd(&mut self, fd: i32) -> Result<(), u32> {
        if fd < 0 || fd as usize >= FD_CAP {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(XC_EBADF);
        }
        if self.fds[fd as usize] {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(XC_EINVAL);
        }
        self.fds[fd as usize] = true;
        Ok(())
    }

    /// 占缓冲槽
    pub fn pool_take(&mut self, slot: usize) -> Result<(), u32> {
        if slot >= POOL_SLOTS {
            return Err(XC_EINVAL);
        }
        if self.pool[slot] {
            return Err(XC_EINVAL);
        }
        self.pool[slot] = true;
        Ok(())
    }

    /// 清理：模式逐位恢复 + fd 全关 + 池全放
    pub fn cleanup(&mut self) -> Result<(), u32> {
        if self.cleaned {
            return Err(XC_ETWICE);
        }
        match self.reg.take() {
            None => Err(XC_ENOREG),
            Some(r) => {
                self.flags = r.saved_flags;
                self.fds = vec![false; FD_CAP];
                self.pool = vec![false; POOL_SLOTS];
                self.cleaned = true;
                self.n_cleanup = self.n_cleanup.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 零泄漏自检：fd 全关且池全空
    pub fn no_leak(&self) -> bool {
        self.fds.iter().all(|f| !f) && self.pool.iter().all(|p| !p)
    }

    /// 模式恢复自检
    pub fn mode_ok(&self, expect: u32) -> bool {
        self.flags == expect
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_cleanup: u32) -> bool {
        self.n_cleanup == expect_cleanup && self.no_leak()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.flags as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.n_cleanup as u64).wrapping_mul(0x100000001b3);
        h = (h ^ (self.cleaned as u64)).wrapping_mul(0x100000001b3);
        if let Some(r) = &self.reg {
            h = (h ^ r.fold()).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "exitclean:flags={:#x} fds={} pool={} cleaned={}",
            self.flags,
            self.fds.iter().filter(|f| **f).count(),
            self.pool.iter().filter(|p| **p).count(),
            self.cleaned
        )
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    // 路1：未登记清理 → ENOREG
    let r1 = matches!(ExitCleanup::new().cleanup(), Err(x) if x == XC_ENOREG);
    // 路2：越界模式位 → EINVAL
    let mut d = ExitCleanup::new();
    let r2 = matches!(d.set_flags(0x100), Err(x) if x == XC_EINVAL);
    // 路3：坏 fd → EBADF
    let r3 = matches!(d.open_fd(-2), Err(x) if x == XC_EBADF);
    // 路4：重复登记 → ETWICE
    let _ = d.register();
    let r4 = matches!(d.register(), Err(x) if x == XC_ETWICE);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮一轮完整生命周期（登记→污染→异常退出清理），
/// 模式恢复 + 零泄漏记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let mut d = ExitCleanup::new();
        // 先登记（快照初始模式 0x0f）
        if d.register().is_err() {
            return good;
        }
        // 污染：改模式 + 开 3 个 fd + 占 2 个池槽
        let polluted = ((r % 200) as u32) + 1;
        let _ = d.set_flags(polluted);
        let _ = d.open_fd(0);
        let _ = d.open_fd(3);
        let _ = d.open_fd(5);
        let _ = d.pool_take(1);
        let _ = d.pool_take(3);
        // 异常退出清理
        if d.cleanup().is_ok()
            && d.mode_ok(0x0f)
            && d.no_leak()
            && d.ledger_ok(1)
        {
            good = good.wrapping_add(1);
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10972_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10972");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "ENOREG/EINVAL/EBADF/ETWICE 坏样本全部命中");

    // J1 异常退出模式恢复 10/10
    let mut mode10 = 0u32;
    for _ in 0..10 {
        let mut d = ExitCleanup::new();
        let _ = d.register();
        let _ = d.set_flags(0x77);
        let _ = d.open_fd(2);
        if d.cleanup().is_ok() && d.mode_ok(0x0f) && d.no_leak() {
            mode10 = mode10.wrapping_add(1);
        }
    }
    s.add("J1-模式恢复10/10", mode10 == 10, "异常退出后模式逐位恢复十轮全对");

    // 资源零泄漏 10/10
    let mut leak10 = 0u32;
    for _ in 0..10 {
        let mut d = ExitCleanup::new();
        let _ = d.register();
        for f in 0..FD_CAP as i32 {
            let _ = d.open_fd(f);
        }
        for p in 0..POOL_SLOTS {
            let _ = d.pool_take(p);
        }
        if d.cleanup().is_ok() && d.no_leak() && d.fds.iter().all(|f| !f) && d.pool.iter().all(|p| !p) {
            leak10 = leak10.wrapping_add(1);
        }
    }
    s.add("J2-资源零泄漏10/10", leak10 == 10, "全量 fd/池占用清理后零泄漏");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮生命周期", good == 1000, "千轮登记-污染-清理全对");

    // 泄漏检出面：不清理则必报
    let mut d2 = ExitCleanup::new();
    let _ = d2.register();
    let _ = d2.open_fd(1);
    let leak_face = !d2.no_leak();
    s.add("J4-泄漏检出面", leak_face, "未清理时泄漏账可见");

    // 错误码分段
    let codes = [XC_EBADF, XC_EINVAL, XC_ELEAK, XC_EDRIFT, XC_ETWICE, XC_ENOREG];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "六码全部落在 0x1d900 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "六码两两互异");

    // 账目对平
    let mut d3 = ExitCleanup::new();
    let _ = d3.register();
    let _ = d3.cleanup();
    s.add("J7-账目对平", d3.ledger_ok(1) && d3.cleaned, "一次清理且 cleaned 置位");

    // 二次清理必拒
    let twice = matches!(d3.cleanup(), Err(x) if x == XC_ETWICE);
    s.add("J8-二次清理拒", twice, "清理后登记已取走再清必拒");

    // 判定序：ENOREG 先于状态
    let seq_ok = matches!(ExitCleanup::new().cleanup(), Err(x) if x == XC_ENOREG);
    s.add("J9-判定序", seq_ok, "未登记先报 ENOREG");

    // fold 稳定
    let h1 = d3.fold_all();
    let h2 = d3.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = d3.render();
    s.add(
        "J11-渲染非空",
        r.starts_with("exitclean:") && r.contains("cleaned=true"),
        "摘要含清理状态",
    );

    // 池槽越界面
    let over = matches!(ExitCleanup::new().pool_take(POOL_SLOTS), Err(x) if x == XC_EINVAL);
    s.add("J12-池槽越界", over, "槽位越界报 EINVAL");

    // 动态判据
    s.add_dyn(
        "J13-清理账",
        d3.n_cleanup == 1,
        &crate::alloc::format!("cleanup={}", d3.n_cleanup),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-终态账",
        d3.no_leak() && d3.mode_ok(0x0f),
        &crate::alloc::format!("flags={:#x}", d3.flags),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10972_exit_cleanup_flow() {
        let mut d = ExitCleanup::new();
        assert!(d.register().is_ok());
        let _ = d.set_flags(0x99);
        let _ = d.open_fd(4);
        let _ = d.pool_take(0);
        assert!(d.cleanup().is_ok());
        assert!(d.mode_ok(0x0f));
        assert!(d.no_leak());
        assert!(matches!(d.cleanup(), Err(x) if x == XC_ETWICE));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10972_checks_green() {
        let s = run_unx10972_checks();
        assert!(s.all_passed(), "UNX-F10972 判据未全绿");
    }
}
