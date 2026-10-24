//! unx0347_apisoacct —— UNX-F0347 失败 AP 隔离（坏核降级账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0347 · 失败 AP 隔离 · 目标 260 行
//! 判据原文：UNX-F0347-J1 失败 AP 隔离（不再唤醒），降级账与
//!           F0333 分类联动，系统单核续行。
//! 组件：隔离账（永久隔离位图：入账 AP 永不再唤醒，重复隔离
//!       幂等）+ 降级分类账（与 F0333 六类失败分类逐类联动：
//!       哨兵未置位/三连 INIT 无应答/自检 FAIL/MSR 异常/栈探测
//!       失败/微码失配）+ 续行判定面（活跃核数 ≥ 1 即可单核续行，
//!       0 核是停机事件不是隔离事件）+ 唤醒拒绝闸（对已隔离 AP
//!       的再唤醒请求 100% 拒绝并留痕）。
//! 职责定位：坏核的唯一隔离权威——隔离动作不可逆（位图只置位
//!   不清位），分类联动保证每个隔离都有 F0333 语义标签，续行
//!   判定把「隔离多少核」与「系统还能不能跑」分开回答。
//! 性能逐项分解：隔离 O(1)（位图置位）；分类联动 O(1)；
//!   拒绝闸 O(1)；续行判定 O(1)。全整数确定性，零堆热路径。
//! 诊断码段：0xE420（0xE421~0xE429 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AI_VERSION: &str = "unx0347_apisoacct-v1";

/// 诊断段基址（0xE420 段 · 本模块独占）。
pub const AI_SEG_BASE: u16 = 0xE420;

/// XcCode 闭集（0xE421~0xE429）。
pub const XC_NOT_QUARANTINED: u32 = 0xE421;
pub const XC_CLASS_UNKNOWN: u32 = 0xE422;
pub const XC_WAKE_REFUSED: u32 = 0xE423;
pub const XC_ZERO_CORES: u32 = 0xE424;
pub const XC_BITMAP_FULL: u32 = 0xE425;
pub const XC_UNISO_FAIL: u32 = 0xE426;
pub const XC_CLASS_DRIFT: u32 = 0xE427;
pub const XC_LEDGER_CLOSED: u32 = 0xE428;
pub const XC_APID_OOR: u32 = 0xE429;

/// 隔离位图容量（最大 AP 数，与 F0064 位图同宽纪律）。
pub const ISO_BITMAP_WORDS: usize = 4;
pub const MAX_AP: u32 = (ISO_BITMAP_WORDS * 64) as u32;

/// F0333 失败分类六类闭集（本模块只消费，不自造类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailClass {
    /// 哨兵未置位（F0352 注入主路径）。
    SentinelMissing,
    /// 三连 INIT 无应答。
    InitNoReply,
    /// 十项自检 FAIL（F0345 移交）。
    SelfCheckFail,
    /// MSR 读异常。
    MsrFault,
    /// 栈探测失败。
    StackProbeFail,
    /// 微码修订失配（F0355 联动）。
    UcodeMismatch,
}

impl FailClass {
    /// 分类稳定码（账面落账用，闭集 0~5）。
    pub fn code(self) -> u8 {
        match self {
            FailClass::SentinelMissing => 0,
            FailClass::InitNoReply => 1,
            FailClass::SelfCheckFail => 2,
            FailClass::MsrFault => 3,
            FailClass::StackProbeFail => 4,
            FailClass::UcodeMismatch => 5,
        }
    }

    /// 从 F0333 分类码还原（越界码拒收）。
    pub fn from_code(c: u8) -> Result<FailClass, u16> {
        match c {
            0 => Ok(FailClass::SentinelMissing),
            1 => Ok(FailClass::InitNoReply),
            2 => Ok(FailClass::SelfCheckFail),
            3 => Ok(FailClass::MsrFault),
            4 => Ok(FailClass::StackProbeFail),
            5 => Ok(FailClass::UcodeMismatch),
            _ => Err(XC_CLASS_UNKNOWN as u16),
        }
    }
}

/// 单条隔离账目：AP 编号 + F0333 分类码 + 隔离时的活跃核数快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IsoEntry {
    pub ap_id: u32,
    pub class: FailClass,
    /// 隔离发生时的活跃核数（含 BSP）。
    pub active_at_iso: u32,
}

/// 失败 AP 隔离总账。
pub struct IsoLedger {
    /// 永久隔离位图（只置位不清位——隔离不可逆）。
    bitmap: [u64; ISO_BITMAP_WORDS],
    /// 隔离账目（按隔离时序追加）。
    entries: Vec<IsoEntry>,
    /// 活跃核数（含 BSP；BSP 恒活跃不计入位图）。
    active_cores: u32,
}

impl IsoLedger {
    /// 新建：cores 为含 BSP 的总核数，BSP（AP 0）恒活跃。
    pub fn new(cores: u32) -> Result<IsoLedger, u16> {
        if cores == 0 {
            return Err(XC_ZERO_CORES as u16);
        }
        if cores > MAX_AP {
            return Err(XC_APID_OOR as u16);
        }
        Ok(IsoLedger { bitmap: [0; ISO_BITMAP_WORDS], entries: Vec::new(), active_cores: cores })
    }

    /// AP 编号合法性：1..=cores-1（BSP 不许隔离——BSP 坏了是停机不是降级）。
    fn ap_in_range(&self, ap_id: u32) -> bool {
        ap_id >= 1 && ap_id < self.active_cores && ap_id < MAX_AP
    }

    /// 隔离一个 AP：位图置位 + 分类账追加。重复隔离幂等（返回 Ok 不重记）。
    pub fn quarantine(&mut self, ap_id: u32, class: FailClass) -> Result<bool, u16> {
        if !self.ap_in_range(ap_id) {
            return Err(XC_APID_OOR as u16);
        }
        let word = (ap_id / 64) as usize;
        let bit = ap_id % 64;
        if self.bitmap[word] & (1u64 << bit) != 0 {
            return Ok(false);
        }
        self.bitmap[word] |= 1u64 << bit;
        self.entries.push(IsoEntry { ap_id, class, active_at_iso: self.active_cores });
        self.active_cores -= 1;
        Ok(true)
    }

    /// 是否已隔离。
    pub fn is_quarantined(&self, ap_id: u32) -> bool {
        if ap_id >= MAX_AP {
            return false;
        }
        self.bitmap[(ap_id / 64) as usize] & (1u64 << (ap_id % 64)) != 0
    }

    /// 唤醒拒绝闸：对已隔离 AP 的再唤醒请求 100% 拒绝（判据红线）。
    pub fn wake_request(&self, ap_id: u32) -> Result<(), u16> {
        if self.is_quarantined(ap_id) {
            return Err(XC_WAKE_REFUSED as u16);
        }
        Ok(())
    }

    /// 续行判定：活跃核 ≥ 1 可续行；0 核是停机事件。
    pub fn can_continue(&self) -> Result<bool, u16> {
        if self.active_cores == 0 {
            return Err(XC_ZERO_CORES as u16);
        }
        Ok(true)
    }

    /// 活跃核数快照。
    pub fn active_cores(&self) -> u32 {
        self.active_cores
    }

    /// 降级分类账：与 F0333 六类逐类联动——每条隔离都有分类。
    pub fn class_breakdown(&self) -> [u32; 6] {
        let mut out = [0u32; 6];
        for e in self.entries.iter() {
            out[e.class.code() as usize] += 1;
        }
        out
    }

    /// 账面守恒：位图置位数 == 账目条数（两条记录通道必须同源）。
    pub fn conserve(&self) -> bool {
        let bits: u32 = self.bitmap.iter().map(|w| w.count_ones()).sum();
        bits as usize == self.entries.len()
    }

    /// 隔离时序账目只读视图。
    pub fn entries(&self) -> &[IsoEntry] {
        &self.entries
    }
}

/// 域自检入口：隔离/拒绝/分类/守恒/续行 五族。
pub fn run_unx0347_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0347-AP隔离");
    // 族一：隔离基本流（4 核账，隔离 2 个 AP）。
    let mut led = match IsoLedger::new(4) {
        Ok(l) => l,
        Err(_) => {
            cs.add("账本构造", false, "4 核构造失败");
            return cs;
        }
    };
    let q1 = led.quarantine(1, FailClass::SentinelMissing);
    cs.add("首隔离生效", matches!(q1, Ok(true)), "AP1 哨兵未置位隔离");
    cs.add("重复隔离幂等", matches!(led.quarantine(1, FailClass::SentinelMissing), Ok(false)),
           "位图已置位不重记");
    cs.add("活跃核递减", led.active_cores() == 3, "4→3");
    cs.add("账面守恒", led.conserve(), "位图数==账目数");

    // 族二：唤醒拒绝闸（100% 拒绝红线）。
    cs.add("隔离 AP 拒唤醒", led.wake_request(1).is_err(), "WAKE_REFUSED");
    cs.add("健康 AP 放行", led.wake_request(2).is_ok(), "未隔离放行");
    cs.add("BSP 越界拒", led.quarantine(0, FailClass::SelfCheckFail).is_err(),
           "BSP 坏=停机非隔离");

    // 族三：F0333 分类联动（六类闭集 + 越界拒收）。
    let _ = led.quarantine(2, FailClass::UcodeMismatch);
    let bd = led.class_breakdown();
    cs.add("分类落账", bd[0] == 1 && bd[5] == 1, "哨兵1+微码1");
    cs.add("分类码越界拒", FailClass::from_code(6).is_err(), "闭集外拒收");
    cs.add("分类码还原", FailClass::from_code(3) == Ok(FailClass::MsrFault), "3=MSR 异常");

    // 族四：续行判定。
    cs.add("可续行", matches!(led.can_continue(), Ok(true)), "3 核续行");
    let mut solo_ok = false;
    if let Ok(mut solo) = IsoLedger::new(2) {
        let _ = solo.quarantine(1, FailClass::InitNoReply);
        solo_ok = matches!(solo.can_continue(), Ok(true));
    }
    cs.add("单核续行", solo_ok, "仅 BSP 也续行");
    cs.add("零核=停机事件", IsoLedger::new(0).is_err(), "0 核账本构造即拒");

    // 族五：六类逐类隔离（分类全覆盖核）。
    let classes = [FailClass::SentinelMissing, FailClass::InitNoReply,
                   FailClass::SelfCheckFail, FailClass::MsrFault,
                   FailClass::StackProbeFail, FailClass::UcodeMismatch];
    let mut full_ok = false;
    let mut breakdown_ok = false;
    let mut full_all_ok = true;
    if let Ok(mut full) = IsoLedger::new(7) {
        for (i, c) in classes.iter().enumerate() {
            if full.quarantine((i + 1) as u32, *c).is_err() {
                full_all_ok = false;
            }
        }
        full_ok = full_all_ok;
        breakdown_ok = full.class_breakdown().iter().all(|&v| v == 1);
    }
    cs.add("六类全覆盖", full_ok && breakdown_ok, "六类各一 AP");
    cs
}

// ===== 扩展面：降级续行预算（隔离数红线 + 剩余算力千分比）=====
/// 续行预算：隔离红线（超过半数 AP 隔离即停机预兆，显性立案）
/// 与剩余算力千分比（(活跃-1)/(总-1)，BSP 不计生产算力）。
///
/// 「单核续行」是降级承诺的下限不是目标——预算面把「还能不能
/// 跑」升级为「还能跑多少」，供 F0008 降质链在启动期就拿到
/// 算力画像，避免运行期才发现产能不足。
pub struct ContinueBudget {
    pub total_cores: u32,
}

impl ContinueBudget {
    /// 隔离红线：隔离数 ≥ 总核数一半（BSP 除外）= 停机预兆。
    pub fn iso_redline(&self, iso_count: u32) -> Result<(), u16> {
        let non_bsp = self.total_cores - 1;
        if non_bsp == 0 {
            return Err(1);
        }
        if iso_count * 2 >= non_bsp {
            return Err(2);
        }
        Ok(())
    }

    /// 剩余算力千分比：活跃 AP 数 / 总 AP 数（BSP 不计）。
    pub fn remaining_permille(&self, active_aps: u32) -> Result<u32, u16> {
        let total_aps = self.total_cores - 1;
        if total_aps == 0 {
            return Err(1);
        }
        if active_aps > total_aps {
            return Err(3);
        }
        Ok((active_aps * 1000) / total_aps)
    }
}
