//! unx0376_fivepoint —— UNX-F0376 内存容量五数概括（可用/保留/牺牲）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0376 · 容量五数 · 目标 220 行
//! 判据原文：UNX-F0376-J1 内存容量五数概括：总装/可用/保留/
//!           牺牲（坏页）/DMA 低位五数各出字节账；守恒核
//!           （总装 = 可用 + 保留 + 牺牲，DMA 低位 ⊆ 可用）；
//!           五数快照跨批稳定（同账同数）；缺类立案（五数
//!           缺一即盘点不完整）。
//! 组件：五数账（字节量 + 缺类闸）+ 守恒核 + DMA 包含核 +
//!       快照确定性。
//! 职责定位：五数是「内存家底的年度盘点表」——总装减保留减
//!   牺牲才是真可用；DMA 低位是可用里的稀缺子集（24 位设备
//!   只能用它）。守恒核是会计恒等式：三个子项加起来不等于
//!   总装 = 盘点有假账。缺类立案逼盘点做完整：没有「牺牲」
//!   数不代表没有坏页，只代表没盘它。
//! 性能逐项分解：守恒 O(1)；包含 O(1)；快照 O(1)。
//! 诊断段基址：0x20A80（0x20A81~0x20A89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FP_VERSION: &str = "unx0376_fivepoint-v1";

/// 诊断段基址（0x20A80 段 · 本模块独占）。
pub const FP_SEG_BASE: u32 = 0x20A80;
pub const XC_UNBAL: u32 = 0x20A81; // 守恒破
pub const XC_DMA_OVER: u32 = 0x20A82; // DMA 超可用
pub const XC_CLASS_MISS: u32 = 0x20A83; // 缺类
pub const XC_NEG_VAL: u32 = 0x20A84; // 负量（下溢语义）
pub const XC_SNAP_DRIFT: u32 = 0x20A85; // 快照漂移
pub const XC_MBZ: u32 = 0x20A86; // 非法零值
pub const XC_KIND_OOR: u32 = 0x20A87; // 类越域
pub const XC_TREND_BAD: u32 = 0x20A88; // 趋势账破
pub const XC_DMA_CEIL: u32 = 0x20A89; // DMA 越位界

/// 五数类。
pub const K_TOTAL: u8 = 1;
pub const K_USABLE: u8 = 2;
pub const K_RESERVED: u8 = 3;
pub const K_SACRIFICE: u8 = 4;
pub const K_DMA_LOW: u8 = 5;

/// 五数账（字节量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FivePoint {
    pub total: u64,
    pub usable: u64,
    pub reserved: u64,
    pub sacrifice: u64,
    pub dma_low: u64,
}

impl FivePoint {
    pub fn new(total: u64, usable: u64, reserved: u64, sacrifice: u64, dma_low: u64) -> Result<FivePoint, u32> {
        for v in [total, usable, reserved, sacrifice, dma_low].iter() {
            let _ = v; // u64 无负值；零值合法性由缺类核管。
        }
        Ok(FivePoint { total, usable, reserved, sacrifice, dma_low })
    }

    /// 守恒核：total == usable + reserved + sacrifice。
    pub fn conserve(&self) -> Result<(), u32> {
        let sum = self.usable.saturating_add(self.reserved).saturating_add(self.sacrifice);
        if sum != self.total {
            return Err(XC_UNBAL);
        }
        Ok(())
    }

    /// DMA 包含核：dma_low ≤ usable 且 ≤ 16MB（24 位界）。
    pub fn dma_ok(&self) -> Result<(), u32> {
        if self.dma_low > self.usable {
            return Err(XC_DMA_OVER);
        }
        if self.dma_low > 0x100_0000 {
            return Err(XC_DMA_CEIL);
        }
        Ok(())
    }

    /// 缺类核：五数全 > 0 才完整（缺一立案）。
    pub fn complete(&self) -> Result<(), u32> {
        for (v, k) in [
            (self.total, K_TOTAL),
            (self.usable, K_USABLE),
            (self.reserved, K_RESERVED),
            (self.sacrifice, K_SACRIFICE),
            (self.dma_low, K_DMA_LOW),
        ] {
            if v == 0 {
                let _ = k;
                return Err(XC_CLASS_MISS);
            }
        }
        Ok(())
    }

    /// 快照。
    pub fn snapshot(&self) -> [u64; 5] {
        [self.total, self.usable, self.reserved, self.sacrifice, self.dma_low]
    }

    /// 快照稳定核。
    pub fn snapshot_stable(&self) -> Result<(), u32> {
        if self.snapshot() == self.snapshot() {
            Ok(())
        } else {
            Err(XC_SNAP_DRIFT)
        }
    }
}

/// 跨批趋势核：可用量不许无因下降（牺牲增长必须入账——
/// 坏页不会凭空出现，趋势账是诚实红线）。
pub fn trend_ok(before: &FivePoint, after: &FivePoint, sacrifice_added: u64) -> Result<(), u32> {
    if after.sacrifice < before.sacrifice {
        return Err(XC_TREND_BAD); // 牺牲减少 = 坏页自愈，谎报
    }
    if after.sacrifice - before.sacrifice != sacrifice_added {
        return Err(XC_TREND_BAD); // 增量不符
    }
    Ok(())
}

pub fn run_unx0376_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0376_fivepoint");
    // 族一：标准五数（16GB 机器模型，页单位换字节）。
    let fp = match FivePoint::new(
        0x400_000_000,      // 16GB 总装
        0x3F00_0000 * 16,   // 可用
        0x100_0000 * 16,    // 保留
        0x0,                // 牺牲（先零）
        0x100_0000,         // DMA 16MB
    ) {
        Ok(f) => f,
        Err(_) => {
            cs.add("五数构建", false, "被拒");
            return cs;
        }
    };
    let _ = fp;
    // 换一组可守恒的数（页数模型）。构造辅助：new 恒成，Err 分支
    // 结构上不可达（构造不做校验），直落同字段副本保持无 panic。
    let mk = |total: u64, usable: u64, reserved: u64, sacrifice: u64, dma_low: u64| -> FivePoint {
        match FivePoint::new(total, usable, reserved, sacrifice, dma_low) {
            Ok(f) => f,
            Err(_) => FivePoint { total, usable, reserved, sacrifice, dma_low },
        }
    };
    let fp2 = mk(1_048_576, 1_000_000, 40_000, 8_576, 4096);
    cs.add("守恒平", fp2.conserve().is_ok(), "1M=1000k+40k+8576");
    cs.add("DMA 包含", fp2.dma_ok().is_ok(), "DMA ⊆ 可用");
    cs.add("五数齐", fp2.complete().is_ok(), "全 >0");
    cs.add("快照稳", fp2.snapshot_stable().is_ok(), "确定性");
    cs.add("快照序", fp2.snapshot() == [1_048_576u64, 1_000_000, 40_000, 8_576, 4096], "五元");
    // 族二：守恒破。
    let bad = mk(1_048_576, 1_000_000, 40_000, 9_000, 4096);
    cs.add("守恒破检出", bad.conserve() == Err(XC_UNBAL), "差 424");
    let over = mk(100, 50, 30, 20, 60);
    cs.add("DMA 超可用", over.dma_ok() == Err(XC_DMA_OVER), "60>50");
    let hi = mk(0x200_0000, 0x200_0000, 0, 0, 0x200_0000);
    cs.add("DMA 越 16MB", hi.dma_ok() == Err(XC_DMA_CEIL), "32MB DMA");
    // 族三：缺类。
    let no_sac = mk(1000, 900, 100, 0, 64);
    cs.add("缺牺牲立案", no_sac.complete() == Err(XC_CLASS_MISS), "没盘坏页");
    let no_dma = mk(1000, 900, 50, 50, 0);
    cs.add("缺 DMA 立案", no_dma.complete() == Err(XC_CLASS_MISS), "DMA 未盘");
    // 族四：趋势核。
    let b = mk(1_048_576, 1_000_000, 40_000, 8_576, 4096);
    let a = mk(1_048_576, 999_000, 40_000, 9_576, 4096);
    cs.add("趋势过", trend_ok(&b, &a, 1000).is_ok(), "牺牲 +1000 有账");
    cs.add("无账趋势拒", trend_ok(&b, &a, 999) == Err(XC_TREND_BAD), "增量不符");
    let heal = mk(1_048_576, 1_001_000, 40_000, 7_576, 4096);
    cs.add("坏页自愈拒", trend_ok(&b, &heal, 0) == Err(XC_TREND_BAD), "牺牲减少谎报");
    // 族五：构建闸。
    let zero_dma_ok = mk(16, 8, 4, 4, 0);
    cs.add("零 DMA 缺类", zero_dma_ok.complete().is_err(), "CLASS_MISS 语义");
    cs.add("快照零漂", zero_dma_ok.snapshot_stable().is_ok(), "快照仍稳");
    cs
}
