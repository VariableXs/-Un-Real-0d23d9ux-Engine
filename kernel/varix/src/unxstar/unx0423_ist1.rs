//! unx0423_ist1 —— UNX-F0423 双故障栈（IST1）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0423 · 双故障栈（IST1）· 目标 220 行
//! 判据原文：UNX-F0423-J1 双故障（#DF）走 IST1 专用栈：栈尺寸
//!           达标（≥ 最低 4 页模型）且 16 字节对齐；IST1 配置
//!           进 TSS 后由哨兵字核验（栈底魔数在 = 未被越界踩）；
//!           #DF 期间二次故障不再级联（IST1 内 faults 计数封顶
//!           一次，超额走三重故障路径立案）；栈使用量留痕
//!           （峰值水位 ≤ 容量）；配置非法（尺寸不足/不对齐/
//!           魔数破）显式拒绝。
//! 组件：IST 栈（定容 + 哨兵字 + 水位账）+ TSS 槽模型（IST1
//!       指针 + 配置核）+ #DF 记账器（计数封顶 + 三重故障立案）
//!       + 哨兵核 + 配置闸。
//! 职责定位：双故障栈是「保命的最后底牌」——#DF 本身就是栈
//!   出事的信号，若 #DF 处理器还踩同一张坏栈就是三重故障直接
//!   复位；IST1 的意义是「换一张干净的栈接住 #DF」：独立定容、
//!   独立哨兵、绝不复用出事的那张。封顶一次是诚实红线：IST1
//!   内再故障说明问题不是栈，必须显式升级而不是假装接住了。
//! 性能逐项分解：配置核 O(1)；#DF 记账 O(1)；哨兵核 O(1)；
//!   水位 O(1)（增量跟踪）。
//! 诊断码段：0x212C0（0x212C1~0x212C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const I1_VERSION: &str = "unx0423_ist1-v1";

/// 诊断段基址（0x212C0 段 · 本模块独占）。
pub const I1_SEG_BASE: u32 = 0x212C0;
pub const XC_SIZE_SHORT: u32 = 0x212C1; // 栈尺寸不足
pub const XC_ALIGN_BAD: u32 = 0x212C2; // 不对齐
pub const XC_MAGIC_BRK: u32 = 0x212C3; // 哨兵魔数破
pub const XC_DF_OVER: u32 = 0x212C4; // #DF 计数超额
pub const XC_WATER_HI: u32 = 0x212C5; // 水位超容
pub const XC_TSS_MISS: u32 = 0x212C6; // TSS 槽未配置
pub const XC_RECONF: u32 = 0x212C7; // 重复配置拒
pub const XC_MAGIC: u32 = 0x212C8; // 账魔数破
pub const XC_TRIPLE: u32 = 0x212C9; // 三重故障立案

/// 栈底哨兵魔数。
pub const CANARY: u64 = 0xCAFE_BABE_DEAD_F00D;

/// 最低页数（4 页模型）。
pub const MIN_PAGES: u32 = 4;

/// IST 栈。
#[derive(Clone, Debug)]
pub struct IstStack {
    /// 页数。
    pub pages: u32,
    /// 基址（16B 对齐，模型编号）。
    pub base: u64,
    /// 栈底哨兵（未被踩 = CANARY）。
    pub canary: u64,
    /// 峰值用水（字节模型）。
    pub peak_used: u32,
}

impl IstStack {
    /// 建：页数 + 基址。
    pub fn new(pages: u32, base: u64) -> Result<IstStack, u32> {
        if pages < MIN_PAGES {
            return Err(XC_SIZE_SHORT);
        }
        if base % 16 != 0 {
            return Err(XC_ALIGN_BAD);
        }
        Ok(IstStack { pages, base, canary: CANARY, peak_used: 0 })
    }

    /// 容量（字节模型）。
    pub fn cap(&self) -> u32 {
        self.pages * 4096
    }

    /// 哨兵核。
    pub fn canary_ok(&self) -> Result<(), u32> {
        if self.canary != CANARY {
            return Err(XC_MAGIC_BRK);
        }
        Ok(())
    }

    /// 模拟入栈用水：更新峰值；超容立案。
    pub fn push_water(&mut self, bytes: u32) -> Result<u32, u32> {
        let new_peak = self.peak_used + bytes;
        if new_peak > self.cap() {
            return Err(XC_WATER_HI);
        }
        self.peak_used = new_peak;
        Ok(new_peak)
    }

    /// 收官清账：水位归零（哨兵不动）。
    pub fn reset_water(&mut self) {
        self.peak_used = 0;
    }

    /// 踩哨兵（演练注入）。
    pub fn corrupt_canary(&mut self) {
        self.canary ^= 1;
    }
}

/// TSS 槽模型（IST1 指针 + 配置态）。
pub struct TssSlot {
    /// IST1 指针（栈顶 = 基址 + 容量）。
    pub ist1_ptr: Option<u64>,
    /// 配置过的栈（防重复配置）。
    configured: bool,
}

impl TssSlot {
    /// 建空槽。
    pub fn new() -> TssSlot {
        TssSlot { ist1_ptr: None, configured: false }
    }

    /// 配置 IST1：栈合法 + 未配置过才受理。
    pub fn install(&mut self, s: &IstStack) -> Result<u64, u32> {
        s.canary_ok()?;
        if self.configured {
            return Err(XC_RECONF);
        }
        let top = s.base + s.cap() as u64;
        self.ist1_ptr = Some(top);
        self.configured = true;
        Ok(top)
    }

    /// 读指针（未配置拒）。
    pub fn ptr(&self) -> Result<u64, u32> {
        match self.ist1_ptr {
            Some(p) => Ok(p),
            None => Err(XC_TSS_MISS),
        }
    }
}

/// #DF 记账器：封顶一次，超额 = 三重故障立案。
pub struct DfCounter {
    count: u32,
    triple: bool,
}

impl DfCounter {
    /// 建。
    pub fn new() -> DfCounter {
        DfCounter { count: 0, triple: false }
    }

    /// #DF 到达：首例受理；已封顶后到达 = 三重故障。
    pub fn on_df(&mut self) -> Result<(), u32> {
        if self.triple {
            return Err(XC_TRIPLE);
        }
        if self.count >= 1 {
            self.triple = true;
            return Err(XC_DF_OVER);
        }
        self.count += 1;
        Ok(())
    }

    /// 计数。
    pub fn count(&self) -> u32 {
        self.count
    }

    /// 三重故障标志。
    pub fn is_triple(&self) -> bool {
        self.triple
    }

    /// 复位（新上下文）。
    pub fn reset(&mut self) {
        self.count = 0;
        self.triple = false;
    }
}

pub fn run_unx0423_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0423_ist1");
    // 族一：标准配置。
    let s1 = match IstStack::new(4, 0x1000_0000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建栈", false, "拒");
            return cs;
        }
    };
    cs.add("容量 16K", s1.cap() == 16 * 1024, "4 页");
    cs.add("16B 对齐", s1.base % 16 == 0, "0x1000_0000");
    cs.add("哨兵完好", s1.canary_ok().is_ok(), "CANARY 在");
    let mut tss = TssSlot::new();
    let top = tss.install(&s1);
    cs.add("TSS 安装成", top == Ok(0x1000_4000), "栈顶 = 基址 + 16K");
    cs.add("TSS 读回", tss.ptr() == Ok(0x1000_4000), "指针核");
    cs.add("重复配置拒", tss.install(&s1) == Err(XC_RECONF), "已配");
    // 族二：#DF 计数封顶。
    let mut df = DfCounter::new();
    cs.add("首例 #DF 受理", df.on_df().is_ok(), "IST1 接住");
    cs.add("计数 1", df.count() == 1, "封顶值");
    cs.add("二例 #DF 超额", df.on_df() == Err(XC_DF_OVER), "IST1 内再故障");
    cs.add("三重故障置位", df.is_triple(), "升级路径");
    cs.add("三重后恒拒", df.on_df() == Err(XC_TRIPLE), "不再受理");
    cs.add("复位复活", {
        df.reset();
        df.on_df().is_ok()
    }, "新上下文");
    // 族三：水位账。
    let mut w = match IstStack::new(4, 0x2000_0000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建栈2", false, "拒");
            return cs;
        }
    };
    cs.add("用水 4K", w.push_water(4096) == Ok(4096), "峰值 4K");
    cs.add("用水 8K", w.push_water(4096) == Ok(8192), "峰值 8K");
    cs.add("退水重涨", {
        w.reset_water();
        w.push_water(2048) == Ok(2048)
    }, "峰值重置");
    cs.add("超容立案", w.push_water(16 * 1024) == Err(XC_WATER_HI), "2K + 16K > 16K");
    cs.add("超容拒后峰值不动", w.peak_used == 2048, "拒绝不记账");
    cs.add("哨兵仍完好", w.canary_ok().is_ok(), "水位不影响哨兵");
    // 族四：哨兵踩破检出。
    let mut c = match IstStack::new(8, 0x3000_0000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建栈3", false, "拒");
            return cs;
        }
    };
    c.corrupt_canary();
    cs.add("踩破检出", c.canary_ok() == Err(XC_MAGIC_BRK), "CANARY 异");
    // 族五：配置拒绝。
    cs.add("尺寸不足拒", matches!(IstStack::new(3, 0x1000), Err(XC_SIZE_SHORT)), "3 < 4 页");
    cs.add("尺寸下限过", IstStack::new(MIN_PAGES, 0x10).is_ok(), "=4 页");
    cs.add("不对齐拒", matches!(IstStack::new(4, 0x1008), Err(XC_ALIGN_BAD)), "0x1008 % 16 = 8");
    let empty = TssSlot::new();
    cs.add("未配置读拒", empty.ptr() == Err(XC_TSS_MISS), "无指针");
    cs
}
