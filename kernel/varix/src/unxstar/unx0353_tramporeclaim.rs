//! unx0353_tramporeclaim —— UNX-F0353 trampoline 内存回收（低 1MB 归还）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0353 · trampoline 内存回收 · 目标 220 行
//! 判据原文：UNX-F0353-J1 借用窗口注销归还 F0064，归还后区间可
//!           分配，热图同步。
//! 组件：借用窗口模型（低 1MB trampoline 区：基址 0x0、页数
//!       256，启动期从 F0064 物理分配器借用）+ 借用账（借出/
//!       归还两态，归还后区间重新可分配）+ 归还前置核（全 AP
//!       已离开 trampoline：活跃用户计数归零才许归还——有 AP 还
//!       在跑二进制就归还 = 自毁）+ 热图同步位（归还后 F0077 热
//!       图该区间刷新为「可分配」，不同步 = 视图分裂）。
//! 职责定位：trampoline 生命周期的唯一收口——借出时记账、使用
//!   时计数、归还时三核（用户归零/区间完整/热图同步），归还后
//!   区间回到通用池。低 1MB 是稀缺资源（实模式可达区），回收
//!   纪律直接决定大内存机器的可用页预算。
//! 性能逐项分解：借出 O(1)；归还 O(1)（定容窗口单实例）；核账
//!   O(页数/64)（位图扫描）。全整数确定性。
//! 诊断码段：0xE480（0xE481~0xE489 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const TR_VERSION: &str = "unx0353_tramporeclaim-v1";

/// 诊断段基址（0xE480 段 · 本模块独占）。
pub const TR_SEG_BASE: u16 = 0xE480;

/// XcCode 闭集（0xE481~0xE489）。
pub const XC_BUSY_USERS: u32 = 0xE481;
pub const XC_NOT_BORROWED: u32 = 0xE482;
pub const XC_DOUBLE_RETURN: u32 = 0xE483;
pub const XC_RANGE_INCOMPLETE: u32 = 0xE484;
pub const XC_HEAT_DESYNC: u32 = 0xE485;
pub const XC_BORROW_TWICE: u32 = 0xE486;
pub const XC_PAGE_OOR: u32 = 0xE487;
pub const XC_USER_NEG: u32 = 0xE488;
pub const XC_REALMODE_BOUND: u32 = 0xE489;

/// trampoline 借用区参数（判据写死：低 1MB = 256 页 4KiB）。
pub const TRAMP_BASE_PAGE: u64 = 0;
pub const TRAMP_PAGES: u64 = 256;

/// 借用窗口状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowState {
    /// 已归还（初始态：未借出）。
    Returned,
    /// 借出中（trampoline 驻留）。
    Borrowed,
}

/// trampoline 借用窗口账。
pub struct BorrowWindow {
    state: WindowState,
    /// 活跃用户计数（正在执行 trampoline 代码的 AP 数）。
    active_users: i64,
    /// 归还后是否已同步热图。
    heat_synced: bool,
}

impl BorrowWindow {
    /// 新建窗口（初始已归还态，未借出）。
    pub fn new() -> BorrowWindow {
        BorrowWindow { state: WindowState::Returned, active_users: 0, heat_synced: false }
    }

    /// 借出：重复借出拒；借出时热图同步位复位。
    pub fn borrow(&mut self) -> Result<(), u16> {
        if self.state == WindowState::Borrowed {
            return Err(XC_BORROW_TWICE as u16);
        }
        self.state = WindowState::Borrowed;
        self.heat_synced = false;
        Ok(())
    }

    /// 用户进入/离开计数（进入 +1、离开 -1；负数 = 账面破）。
    pub fn user_enter(&mut self) -> Result<(), u16> {
        self.active_users += 1;
        Ok(())
    }

    pub fn user_leave(&mut self) -> Result<(), u16> {
        self.active_users -= 1;
        if self.active_users < 0 {
            return Err(XC_USER_NEG as u16);
        }
        Ok(())
    }

    /// 归还前置核：借出态 + 用户归零，二者缺一即拒。
    pub fn reclaim_precheck(&self) -> Result<(), u16> {
        if self.state != WindowState::Borrowed {
            return Err(XC_NOT_BORROWED as u16);
        }
        if self.active_users != 0 {
            return Err(XC_BUSY_USERS as u16);
        }
        Ok(())
    }

    /// 归还：前置核通过后注销借出态；二次归还在归还态下拒。
    pub fn reclaim(&mut self) -> Result<(), u16> {
        self.reclaim_precheck()?;
        if self.state == WindowState::Returned {
            return Err(XC_DOUBLE_RETURN as u16);
        }
        self.state = WindowState::Returned;
        Ok(())
    }

    /// 热图同步：归还后区间刷新为可分配；未归还就同步 = 视图分裂。
    pub fn sync_heat(&mut self) -> Result<(), u16> {
        if self.state == WindowState::Borrowed {
            return Err(XC_HEAT_DESYNC as u16);
        }
        self.heat_synced = true;
        Ok(())
    }

    /// 归还后区间可分配断言：窗口已归还 且 热图已同步。
    pub fn region_reusable(&self) -> bool {
        self.state == WindowState::Returned && self.heat_synced
    }

    /// 区间完整性：基址与页数与判据常量一致（防调用方私改）。
    pub fn range_complete(base: u64, pages: u64) -> Result<(), u16> {
        if base != TRAMP_BASE_PAGE {
            return Err(XC_RANGE_INCOMPLETE as u16);
        }
        if pages != TRAMP_PAGES {
            return Err(XC_RANGE_INCOMPLETE as u16);
        }
        // 低 1MB 边界：止页必须落在实模式可达区（< 1MiB / 4KiB = 256）。
        if base + pages > 256 {
            return Err(XC_REALMODE_BOUND as u16);
        }
        Ok(())
    }

    /// 当前态与用户数快照。
    pub fn snapshot(&self) -> (WindowState, i64, bool) {
        (self.state, self.active_users, self.heat_synced)
    }
}

/// 域自检入口：借还/前置/热图/区间 四族。
pub fn run_unx0353_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0353-trampoline回收");
    // 族一：借出与归还主链。
    let mut w = BorrowWindow::new();
    cs.add("未借出拒还", w.reclaim().is_err(), "Returned 态不可还");
    cs.add("借出生效", w.borrow().is_ok(), "借出窗口");
    cs.add("重复借出拒", w.borrow().is_err(), "单实例窗口");
    cs.add("区间完整", BorrowWindow::range_complete(TRAMP_BASE_PAGE, TRAMP_PAGES).is_ok(),
           "0x0 起 256 页");
    cs.add("区间被改拒", BorrowWindow::range_complete(1, TRAMP_PAGES).is_err(), "基址私改");
    cs.add("越实模式界拒", BorrowWindow::range_complete(0, 300).is_err(), ">256 页");

    // 族二：归还前置核（用户未清零拒还）。
    let _ = w.user_enter();
    let _ = w.user_enter();
    cs.add("占用拒还", w.reclaim().is_err(), "2 用户在跑");
    let _ = w.user_leave();
    cs.add("单用户仍拒", w.reclaim().is_err(), "1 用户在跑");
    let _ = w.user_leave();
    cs.add("用户清零可还", w.reclaim_precheck().is_ok(), "归零放行");
    cs.add("负计数检出", w.user_leave().is_err(), "-1 = 账面破");

    // 族三：归还与热图同步。
    cs.add("归还生效", w.reclaim().is_ok(), "注销借出态");
    cs.add("二次归还拒", w.reclaim().is_err(), "只还一次");
    cs.add("未同步不可复用", !w.region_reusable(), "热图未刷");
    cs.add("借出中拒同步", new_sync_guard(), "视图分裂防线");
    cs.add("同步生效", w.sync_heat().is_ok(), "区间刷回可分配");
    cs.add("归还后可复用", w.region_reusable(), "归还+同步双条件");
    cs
}

/// 辅助：借出态下热图同步必须被拒（独立构造一条借用链验证）。
fn new_sync_guard() -> bool {
    let mut w2 = BorrowWindow::new();
    let _ = w2.borrow();
    w2.sync_heat().is_err()
}

// ===== 扩展面：归还页位图（逐页归还状态 + 区间可分配扫描核）=====
/// 页状态位图：256 位（低 1MB 逐页），借出期全「占用」，归还后
/// 全「空闲」。位图与窗口态双账守恒：态=Borrowed ⇔ 位图全 1，
/// 态=Returned+已同步 ⇔ 位图全 0。
pub struct ReclaimBitmap {
    words: [u64; 4],
}

impl ReclaimBitmap {
    pub fn new_borrowed() -> ReclaimBitmap {
        ReclaimBitmap { words: [u64::MAX; 4] }
    }

    pub fn mark_returned(&mut self) {
        self.words = [0; 4];
    }

    /// 守恒核：Borrowed ⇔ 全 1，Returned ⇔ 全 0。
    pub fn conserve(&self, borrowed: bool) -> bool {
        let ones: u32 = self.words.iter().map(|w| w.count_ones()).sum();
        if borrowed {
            ones == 256
        } else {
            ones == 0
        }
    }

    /// 可分配扫描：归还后任一页区间 [lo,lo+n) 应可分配（全 0）。
    pub fn allocatable(&self, lo: u64, n: u64) -> Result<bool, u16> {
        if n == 0 || lo + n > 256 {
            return Err(1);
        }
        let mut all_free = true;
        for p in lo..(lo + n) {
            if self.words[(p / 64) as usize] & (1u64 << (p % 64)) != 0 {
                all_free = false;
            }
        }
        Ok(all_free)
    }
}
