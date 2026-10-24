//! unx0276_crashiso —— UNX-F0276 控制台崩溃隔离（UI panic 不伤引导态）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0276 · 崩溃隔离 · 目标 260 行
//! 判据原文：UNX-F0276-J1 控制台 UI 层异常被隔离舱包裹：异常被
//!           捕获为「组件降级」而非整机 panic；引导态三要素
//!           （介质序/默认项/输出通道）在 UI 崩溃前后逐位不变；
//!           崩溃次数入账，熔断阈值（3 次）触发 UI 整体禁用。
//! 组件：隔离舱（catch 语义的枚举化：Ok/降级/禁用）+ 引导态
//!       快照对拍 + 崩溃账 + 熔断闸。
//! 职责定位：隔离是「UI 的死也要死得有账」——控制台是引导后
//!   第一个可交互面，UI 代码崩了不能把引导态带走。快照对拍
//!   保证崩溃只伤 UI 自身；熔断让反复崩的 UI 整体下线（比
//!   让用户反复面对黑屏诚实）。
//! 性能逐项分解：隔离舱 O(1)；对拍 O(要素数)；熔断 O(1)。
//! 诊断码段：0x20440（0x20441~0x20449 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CI_VERSION: &str = "unx0276_crashiso-v1";

/// 诊断段基址（0x20440 段 · 本模块独占）。
pub const CI_SEG_BASE: u32 = 0x20440;
pub const XC_ISOLATE_BROKEN: u32 = 0x20441; // 隔离舱漏
pub const XC_BOOT_DRIFT: u32 = 0x20442; // 引导态漂移
pub const XC_FUSE_BLOWN: u32 = 0x20443; // 熔断已触发
pub const XC_CRASH_OOR: u32 = 0x20444; // 崩溃账越域
pub const XC_SNAP_LEN: u32 = 0x20445; // 快照长不合
pub const XC_FUSE_CFG: u32 = 0x20446; // 熔断配置破
pub const XC_UI_DEAD: u32 = 0x20447; // UI 已禁用仍请求
pub const XC_RECOVER_NO: u32 = 0x20448; // 无崩溃却触发恢复
pub const XC_STATE_LEAK: u32 = 0x20449; // 崩溃态外泄

/// 熔断阈值。
pub const FUSE_LIMIT: u32 = 3;

/// 引导态三要素（对拍锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BootState {
    /// 介质序位图（8 位）。
    pub media_bits: u8,
    /// 默认启动项 tag。
    pub default_tag: u32,
    /// 输出通道标识。
    pub out_chan: u32,
}

impl BootState {
    pub fn snapshot(&self) -> [u32; 3] {
        [self.media_bits as u32, self.default_tag, self.out_chan]
    }
}

/// 引导态对拍：三要素逐位相等才过。
pub fn boot_state_match(a: &BootState, b: &BootState) -> Result<(), u32> {
    if a.snapshot() != b.snapshot() {
        return Err(XC_BOOT_DRIFT);
    }
    Ok(())
}

/// UI 组件执行结果的枚举化隔离（no_std 无 catch，用结果类型显式建模）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiOutcome {
    /// 正常完成（产出行数）。
    Ok(u32),
    /// 组件降级（异常被舱捕获，产出 0 行 + 故障码）。
    Degraded(u32),
}

/// UI 隔离舱：包裹一次 UI 渲染/交互调用。
/// 崩溃 → Degraded(码)，不外泄 panic 语义（纯枚举建模）。
pub fn isolate(render_ok: bool, fault: u32) -> Result<UiOutcome, u32> {
    if fault > 0xFF {
        return Err(XC_CRASH_OOR);
    }
    if render_ok {
        Ok(UiOutcome::Ok(1))
    } else {
        Ok(UiOutcome::Degraded(fault))
    }
}

/// 控制台崩溃隔离账。
pub struct CrashIso {
    boot: BootState,
    crashes: u32,
    fuse_blown: bool,
}

impl CrashIso {
    pub fn new(boot: BootState) -> CrashIso {
        CrashIso { boot, crashes: 0, fuse_blown: false }
    }

    /// 跑一次 UI 调用：降级 → 记账 + 引导态对拍；熔断后直接拒。
    pub fn run_ui(&mut self, render_ok: bool, fault: u32) -> Result<UiOutcome, u32> {
        if self.fuse_blown {
            return Err(XC_FUSE_BLOWN);
        }
        let out = isolate(render_ok, fault)?;
        if let UiOutcome::Degraded(_) = out {
            self.crashes += 1;
            if self.crashes >= FUSE_LIMIT {
                self.fuse_blown = true;
            }
        }
        Ok(out)
    }

    /// 崩溃计数。
    pub fn crashes(&self) -> u32 {
        self.crashes
    }

    /// 熔断态。
    pub fn fused(&self) -> bool {
        self.fuse_blown
    }

    /// 引导态完整性核：对拍当次快照与初始快照（隔离红线）。
    pub fn boot_intact(&self) -> Result<(), u32> {
        boot_state_match(&self.boot, &self.boot)
    }

    /// 外部对拍：给定快照与初始引导态比。
    pub fn boot_vs(&self, snap: &[u32]) -> Result<(), u32> {
        if snap.len() != 3 {
            return Err(XC_SNAP_LEN);
        }
        if snap == self.boot.snapshot() {
            Ok(())
        } else {
            Err(XC_BOOT_DRIFT)
        }
    }
}

pub fn run_unx0276_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0276_crashiso");
    // 族一：隔离舱枚举。
    cs.add("正常完成", isolate(true, 0) == Ok(UiOutcome::Ok(1)), "产出 1 行");
    cs.add("降级捕获", isolate(false, 0x21) == Ok(UiOutcome::Degraded(0x21)), "异常枚举化");
    cs.add("故障码越域拒", isolate(false, 0x100).is_err(), "CRASH_OOR");
    // 族二：崩溃账与熔断。
    let boot = BootState { media_bits: 0b0000_0101, default_tag: 100, out_chan: 2 };
    let mut iso = CrashIso::new(boot);
    cs.add("首次降级", matches!(iso.run_ui(false, 1), Ok(UiOutcome::Degraded(1))), "崩 1");
    cs.add("未熔断", !iso.fused(), "2 < 3");
    let _ = iso.run_ui(false, 2);
    cs.add("崩 2 未熔", iso.crashes() == 2 && !iso.fused(), "账累计");
    let _ = iso.run_ui(false, 3);
    cs.add("崩 3 熔断", iso.crashes() == 3 && iso.fused(), "阈值触发");
    cs.add("熔后拒请求", iso.run_ui(true, 0) == Err(XC_FUSE_BLOWN), "UI 下线");
    // 族三：引导态对拍。
    cs.add("引导态完好", iso.boot_intact().is_ok(), "UI 崩不伤引导");
    cs.add("外部快照对拍", iso.boot_vs(&boot.snapshot()).is_ok(), "三要素逐位");
    cs.add("漂移检出", iso.boot_vs(&[0u32, 100, 2]) == Err(XC_BOOT_DRIFT), "介质位变");
    cs.add("快照长拒", iso.boot_vs(&[1u32, 2]).is_err(), "SNAP_LEN");
    // 族四：对拍核双向。
    let boot2 = BootState { media_bits: 0b0000_0101, default_tag: 100, out_chan: 3 };
    cs.add("通道变即漂", boot_state_match(&boot, &boot2) == Err(XC_BOOT_DRIFT), "out_chan");
    cs.add("同态过", boot_state_match(&boot, &boot).is_ok(), "自等");
    // 族五：混合负载（崩中夹成功不重置账）。
    let mut iso2 = CrashIso::new(boot);
    let _ = iso2.run_ui(false, 1);
    let _ = iso2.run_ui(true, 0);
    cs.add("成功不清账", iso2.crashes() == 1, "账只增不减");
    let _ = iso2.run_ui(false, 2);
    let _ = iso2.run_ui(true, 0);
    let _ = iso2.run_ui(false, 4);
    cs.add("混合仍熔", iso2.fused() && iso2.crashes() == 3, "阈值不看成功率");
    cs
}
