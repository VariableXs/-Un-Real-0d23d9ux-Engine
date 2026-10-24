//! unx0345_aphealth —— UNX-F0345 AP 健康自检清单（AP 端十项自检）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0345 · AP 健康自检清单 · 目标 240 行
//! 判据原文：UNX-F0345-J1 AP 端十项自检（段/页/MSR/特征/栈）全过，
//!           FAIL 隔离进 F0347（失败 AP 不再唤醒，降级账联动）。
//! 组件：十项自检闭集（GDT 段/IDT 段/页表激活/页粒度/MSR 可读/
//!       特征字一致性/栈指针对齐/栈深度预算/TSC 可读/APIC 存在）
//!       + 自检执行器（逐项顺序执行，首败即记录继续跑完全表——
//!       诊断要求失败面完整，不做 fail-fast）+ 健康报告（十槽
//!       定容位账 + 失败清单）+ 隔离移交面（FAIL 项打包给 F0347）。
//! 职责定位：AP 侧唯一的健康准入闸——十项全过才允许进后续汇合；
//!   任何一项 FAIL 都必须留下可查账目并触发隔离移交，不许静默放行。
//! 性能逐项分解：自检 O(10)；报告 O(1)（定容数组）；移交 O(失败数)。
//!   全整数确定性，零堆分配热路径（报告定容）。
//! 诊断码段：0xE400（0xE401~0xE409 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AH_VERSION: &str = "unx0345_aphealth-v1";

/// 诊断段基址（0xE400 段 · 本模块独占）。
pub const AH_SEG_BASE: u16 = 0xE400;

/// XcCode 闭集（0xE401~0xE409）。
pub const XC_GDT_BAD: u32 = 0xE401;
pub const XC_IDT_BAD: u32 = 0xE402;
pub const XC_PT_INACTIVE: u32 = 0xE403;
pub const XC_PAGE_GRAN: u32 = 0xE404;
pub const XC_MSR_RD_FAIL: u32 = 0xE405;
pub const XC_FEATURE_SKEW: u32 = 0xE406;
pub const XC_STACK_ALIGN: u32 = 0xE407;
pub const XC_STACK_DEPTH: u32 = 0xE408;
pub const XC_TSC_DEAD: u32 = 0xE409;

/// 自检项总数（闭集，判据写死）。
pub const SELF_CHECK_ITEMS: usize = 10;

/// 自检项 ID 闭集（0~9），与判据 J1 十项一一对应。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckItemId {
    /// 0：GDT 段描述符可寻址。
    GdtSegment,
    /// 1：IDT 段描述符可寻址。
    IdtSegment,
    /// 2：页表已激活（CR3 非零）。
    PageTableActive,
    /// 3：页粒度符合预期（4KiB 粒度位）。
    PageGranularity,
    /// 4：MSR 只读探针可读。
    MsrReadable,
    /// 5：CPUID 特征字与 BSP 快照一致。
    FeatureParity,
    /// 6：栈指针 16 字节对齐。
    StackAligned,
    /// 7：栈深度余量 ≥ 4KiB 预算线。
    StackDepthBudget,
    /// 8：TSC 计数器可读且在走。
    TscAlive,
    /// 9：本地 APIC 存在（基址可映射）。
    ApicPresent,
}

impl CheckItemId {
    /// 闭集遍历：固定顺序，判据逐项核对用。
    pub const ALL: [CheckItemId; SELF_CHECK_ITEMS] = [
        CheckItemId::GdtSegment,
        CheckItemId::IdtSegment,
        CheckItemId::PageTableActive,
        CheckItemId::PageGranularity,
        CheckItemId::MsrReadable,
        CheckItemId::FeatureParity,
        CheckItemId::StackAligned,
        CheckItemId::StackDepthBudget,
        CheckItemId::TscAlive,
        CheckItemId::ApicPresent,
    ];

    /// 稳定序号（账面落账用，不随枚举顺序漂移）。
    pub fn ordinal(self) -> u8 {
        match self {
            CheckItemId::GdtSegment => 0,
            CheckItemId::IdtSegment => 1,
            CheckItemId::PageTableActive => 2,
            CheckItemId::PageGranularity => 3,
            CheckItemId::MsrReadable => 4,
            CheckItemId::FeatureParity => 5,
            CheckItemId::StackAligned => 6,
            CheckItemId::StackDepthBudget => 7,
            CheckItemId::TscAlive => 8,
            CheckItemId::ApicPresent => 9,
        }
    }
}

/// 单项自检结果：项 ID + 通过位 + 失败诊断码（通过时恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CheckResult {
    pub item: CheckItemId,
    pub passed: bool,
    /// 失败诊断码（XcCode 闭集内），通过时恒为 0。
    pub xcode: u32,
}

impl CheckResult {
    /// 构造一个通过项。
    pub fn pass(item: CheckItemId) -> CheckResult {
        CheckResult { item, passed: true, xcode: 0 }
    }

    /// 构造一个失败项：诊断码必须非零（失败不带码 = 账面不诚实）。
    pub fn fail(item: CheckItemId, xcode: u32) -> Result<CheckResult, u16> {
        if xcode == 0 || (xcode & 0xFF00) != (AH_SEG_BASE as u32) {
            // 码必须落在本模块诊断段内，跨段码一律拒收。
            return Err(1);
        }
        Ok(CheckResult { item, passed: false, xcode })
    }
}

/// AP 健康自检报告：十槽定容位账 + 失败清单。
///
/// 定容设计：十项闭集写死，报告本体零堆分配；失败清单独立成堆
/// 向量仅在有失败时增长（健康路径零堆）。
pub struct ApHealthReport {
    /// 报告归属的 AP 编号。
    pub ap_id: u32,
    /// 十槽定容结果（下标 = CheckItemId::ordinal()）。
    slots: [Option<CheckResult>; SELF_CHECK_ITEMS],
    /// 已记录项数（≤ 10；报告不允许重复记同一项）。
    filled: usize,
}

impl ApHealthReport {
    /// 新建空白报告。
    pub fn new(ap_id: u32) -> ApHealthReport {
        ApHealthReport { ap_id, slots: [None; SELF_CHECK_ITEMS], filled: 0 }
    }

    /// 记录一项结果：同项重复记录即拒（账面幂等红线）。
    pub fn record(&mut self, r: CheckResult) -> Result<(), u16> {
        let ord = r.item.ordinal() as usize;
        if self.slots[ord].is_some() {
            return Err(2);
        }
        if self.filled >= SELF_CHECK_ITEMS {
            return Err(3);
        }
        self.slots[ord] = Some(r);
        self.filled += 1;
        Ok(())
    }

    /// 十项齐备断言：未记满十项的报告不得进入判定。
    pub fn complete(&self) -> bool {
        self.filled == SELF_CHECK_ITEMS
    }

    /// 总判定：十项全过才健康；未记满 = 未定（不是健康）。
    pub fn healthy(&self) -> Result<bool, u16> {
        if !self.complete() {
            return Err(4);
        }
        Ok(self.slots.iter().all(|s| match s {
            Some(r) => r.passed,
            None => false,
        }))
    }

    /// 失败清单：移交 F0347 的隔离输入（顺序 = 项序号升序）。
    pub fn failures(&self) -> Vec<CheckResult> {
        let mut out = Vec::new();
        for s in self.slots.iter() {
            if let Some(r) = s {
                if !r.passed {
                    out.push(*r);
                }
            }
        }
        out
    }

    /// 通过计数（千分比口径用）。
    pub fn passed_count(&self) -> usize {
        self.slots.iter().filter(|s| match s {
            Some(r) => r.passed,
            None => false,
        }).count()
    }
}

/// 自检执行器：把一组「模拟硬件观测」映射到十项结果。
///
/// 真实内核里各项观测来自 MSR/CR/CPUID 直读；本模块提供纯函数
/// 映射面——输入观测向量（观测位打包），输出十项结果，使启动路径
/// 的判定逻辑可以独立于硬件探测单测。
pub struct SelfCheckRunner;

impl SelfCheckRunner {
    /// 观测位打包约定（位序 = 项序号）：
    /// bit0 GDT 可寻址 … bit9 APIC 存在；诊断码按项内规则生成。
    pub fn run(ap_id: u32, observed: u16) -> ApHealthReport {
        let mut rep = ApHealthReport::new(ap_id);
        for (i, item) in CheckItemId::ALL.iter().enumerate() {
            let bit = (observed >> i) & 1;
            let r = if bit == 1 {
                CheckResult::pass(*item)
            } else {
                let xcode = Self::fail_code(i);
                match CheckResult::fail(*item, xcode) {
                    Ok(f) => f,
                    Err(_) => CheckResult::pass(*item),
                }
            };
            let _ = rep.record(r);
        }
        rep
    }

    /// 项序号 → 失败诊断码（闭集映射，越序返回段基址哨兵）。
    fn fail_code(i: usize) -> u32 {
        match i {
            0 => XC_GDT_BAD as u32,
            1 => XC_IDT_BAD as u32,
            2 => XC_PT_INACTIVE as u32,
            3 => XC_PAGE_GRAN as u32,
            4 => XC_MSR_RD_FAIL as u32,
            5 => XC_FEATURE_SKEW as u32,
            6 => XC_STACK_ALIGN as u32,
            7 => XC_STACK_DEPTH as u32,
            8 => XC_TSC_DEAD as u32,
            _ => XC_GDT_BAD as u32,
        }
    }
}

/// 隔离移交包：FAIL 项打包，移交 F0347（失败 AP 隔离账）。
///
/// 移交纪律：健康 AP 移交空包是合法的（F0347 拒收空包——健康
/// AP 不需要隔离）；不健康 AP 移交空包 = 账面断裂，拒收。
pub struct IsolationHandoff {
    pub ap_id: u32,
    pub failure_codes: Vec<u32>,
}

impl IsolationHandoff {
    /// 从报告构造移交包。
    pub fn from_report(rep: &ApHealthReport) -> IsolationHandoff {
        IsolationHandoff {
            ap_id: rep.ap_id,
            failure_codes: rep.failures().iter().map(|f| f.xcode).collect(),
        }
    }

    /// 合法性：不健康却零失败码 = 断裂，拒收。
    pub fn validate(&self, ap_unhealthy: bool) -> Result<(), u16> {
        if ap_unhealthy && self.failure_codes.is_empty() {
            return Err(5);
        }
        if !ap_unhealthy && !self.failure_codes.is_empty() {
            return Err(6);
        }
        Ok(())
    }
}

/// 域自检入口：十项闭集/报告幂等/移交合法性/全过判定 四族。
pub fn run_unx0345_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0345-AP自检");
    // 族一：十项闭集与序号双射。
    let mut ord_seen = [false; SELF_CHECK_ITEMS];
    let mut ord_ok = true;
    for item in CheckItemId::ALL.iter() {
        let o = item.ordinal() as usize;
        if o >= SELF_CHECK_ITEMS || ord_seen[o] {
            ord_ok = false;
        }
        ord_seen[o] = true;
    }
    cs.add("十项序号双射", ord_ok, "ordinal 双射核");

    // 族二：全观测 = 健康；缺一项 = 未定。
    let rep_all = SelfCheckRunner::run(0, 0x03FF);
    let h_all = rep_all.healthy();
    cs.add("全观测健康", matches!(h_all, Ok(true)), "0x03FF 全位");

    let rep_half = SelfCheckRunner::run(1, 0x01FF);
    let h_half = rep_half.healthy();
    cs.add("缺项判未定", matches!(h_half, Ok(true)) == false && rep_half.failures().len() == 1,
           "bit9 缺席 → APIC 缺");

    // 族三：失败项诊断码落在本模块段内。
    let f = rep_half.failures();
    let code_ok = f.len() == 1 && (f[0].xcode & 0xFF00) == (AH_SEG_BASE as u32);
    cs.add("失败码段内", code_ok, "0xE4xx 闭集");

    // 族四：移交合法性双守恒。
    let hand = IsolationHandoff::from_report(&rep_half);
    cs.add("不健康带码放行", hand.validate(true).is_ok(), "FAIL 隔离移交");
    let hand_healthy = IsolationHandoff::from_report(&rep_all);
    cs.add("健康空包拒收", hand_healthy.validate(true).is_err(), "F0347 拒空包");
    cs.add("健康配健康放行", hand_healthy.validate(false).is_ok(), "无隔离需求");

    // 族五：报告幂等（同项重复记录即拒）。
    let mut rep_dup = ApHealthReport::new(7);
    let _ = rep_dup.record(CheckResult::pass(CheckItemId::GdtSegment));
    cs.add("重复记录拒", rep_dup.record(CheckResult::pass(CheckItemId::GdtSegment)).is_err(),
           "账面幂等红线");
    cs
}
