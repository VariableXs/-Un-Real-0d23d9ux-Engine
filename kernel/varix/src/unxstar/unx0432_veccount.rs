//! unx0432_veccount —— UNX-F0432 中断 per-vector 计数总账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0432 · 中断 per-vector 计数总账 · 目标 240 行
//! 判据原文：UNX-F0432-J1 以向量为主键的 224 桶计数总账（域
//!           0x20~0xFF）：递增 O(1)、桶值只增不减、审计 =
//!           Σ桶 == 递增总次数（总账平）；桶归属账：每桶记
//!           最近一次来源标签（ISA/LVT/PCI/通用 四源），同
//!           向量换源合法（计数延续、标签更新）；零桶巡检：
//!           布局冻结区（A 区 16 桶）不许有零桶（16 线都该
//!           有心跳）；计数回绕核（u32 滚回立案）；快照导出
//!           （非零桶列表）与重放（同事件序同快照）。
//! 组件：224 桶账 + 四源标签 + 总账核对 + 零桶巡检 + 回绕
//!       哨兵 + 快照导出 + 重放核。
//! 职责定位：per-vector 计数是中断子系统的「心电图」——哪
//!   条线活着、哪条线死了、哪条线疯了全在桶里；总账不平说
//!   明有递增被漏记（通常是内核路径绕过了记账宏）；A 区零
//!   桶说明遗产线断了心跳；回绕不立案会让「计数大」变成「
//!   计数小」的假象。
//! 性能逐项分解：递增 O(1)；总账 O(224)（巡检周期摊销）；
//!   快照 O(224)；重放 O(事件数)。
//! 诊断码段：0x30380（0x30381~0x30389 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VC_VERSION: &str = "unx0432_veccount-v1";

/// 诊断段基址（0x30380 段 · 本模块独占）。
pub const VC_SEG_BASE: u32 = 0x30380;
pub const XC_VEC_OOR: u32 = 0x30381; // 向量越域
pub const XC_TALLY: u32 = 0x30382; // 总账不平
pub const XC_WRAP: u32 = 0x30383; // 计数回绕
pub const XC_ZERO_ZONE_A: u32 = 0x30384; // A 区零桶
pub const XC_SRC_OOR: u32 = 0x30385; // 来源标签越表
pub const XC_REPLAY: u32 = 0x30386; // 重放不一致
pub const XC_MAGIC: u32 = 0x30389; // 账魔数破

/// 向量域。
pub const VEC_LO: u16 = 0x20;
pub const VEC_HI: u16 = 0xFF;
/// A 区边界（遗产心跳区）。
pub const ZONE_A_END: u16 = 0x2F;

/// 四源标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IrqSrc {
    /// A 区遗产线。
    Isa = 1,
    /// B 区本地 LVT。
    Lvt = 2,
    /// C 区 PCI。
    Pci = 3,
    /// E 区通用。
    Generic = 4,
}

impl IrqSrc {
    /// 按向量自动归区。
    pub fn zone_of(v: u16) -> Result<Self, u32> {
        if v < VEC_LO || v > VEC_HI || v == 0x80 {
            return Err(XC_VEC_OOR);
        }
        if v <= 0x2F {
            Ok(IrqSrc::Isa)
        } else if v <= 0x4F {
            Ok(IrqSrc::Lvt)
        } else if v <= 0x7F {
            Ok(IrqSrc::Pci)
        } else {
            Ok(IrqSrc::Generic)
        }
    }
}

/// 桶：计数 + 最近来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bucket {
    /// 计数。
    pub hits: u32,
    /// 最近来源。
    pub last_src: u8, // 0=未触碰
}

/// 224 桶总账。
pub struct VecBook {
    /// 桶阵（下标 = 向量-0x20）。
    pub buckets: Vec<Bucket>,
    /// 递增总次数。
    pub total: u64,
    /// 事件流水（重放核用）。
    pub events: Vec<(u16, u8)>, // (向量, 源码)
}

impl VecBook {
    pub fn new() -> Self {
        VecBook { buckets: vec![Bucket { hits: 0, last_src: 0 }; 224], total: 0, events: Vec::new() }
    }

    /// 递增：向量核 + 回绕核 + 标签更新。
    pub fn bump(&mut self, v: u16, src: IrqSrc) -> Result<u32, u32> {
        if src as u8 > 4 {
            return Err(XC_SRC_OOR);
        }
        IrqSrc::zone_of(v)?;
        let idx = (v - VEC_LO) as usize;
        let b = &mut self.buckets[idx];
        if b.hits == u32::MAX {
            return Err(XC_WRAP);
        }
        b.hits += 1;
        b.last_src = src as u8;
        self.total += 1;
        self.events.push((v, src as u8));
        Ok(b.hits)
    }

    /// 桶读。
    pub fn hits(&self, v: u16) -> Result<u32, u32> {
        if v < VEC_LO || v > VEC_HI {
            return Err(XC_VEC_OOR);
        }
        Ok(self.buckets[(v - VEC_LO) as usize].hits)
    }

    /// 总账核对：Σ桶 == total。
    pub fn tally_ok(&self) -> Result<(), u32> {
        let sum: u64 = self.buckets.iter().map(|b| b.hits as u64).sum();
        if sum == self.total {
            Ok(())
        } else {
            Err(XC_TALLY)
        }
    }

    /// A 区零桶巡检：16 线全要有心跳。
    pub fn zone_a_alive(&self) -> Result<(), u32> {
        for v in VEC_LO..=ZONE_A_END {
            if self.buckets[(v - VEC_LO) as usize].hits == 0 {
                return Err(XC_ZERO_ZONE_A);
            }
        }
        Ok(())
    }

    /// 哨兵区（0x80）禁碰核：哨兵向量永不记账。
    pub fn sentinel_never(&self) -> bool {
        IrqSrc::zone_of(0x80).is_err() && self.hits(0x80).is_err()
    }

    /// 快照导出：非零桶 (向量, 计数) 列表。
    pub fn snapshot(&self) -> Vec<(u16, u32)> {
        let mut out = Vec::new();
        for i in 0..224 {
            if self.buckets[i].hits > 0 {
                out.push(((VEC_LO + i as u16), self.buckets[i].hits));
            }
        }
        out
    }

    /// 重放核：按事件流水在空账上重放，快照须逐项同。
    pub fn replay(&self) -> Result<(), u32> {
        let mut fresh = VecBook::new();
        for &(v, s) in self.events.iter() {
            let src = match s {
                1 => IrqSrc::Isa,
                2 => IrqSrc::Lvt,
                3 => IrqSrc::Pci,
                4 => IrqSrc::Generic,
                _ => return Err(XC_REPLAY),
            };
            if fresh.bump(v, src).is_err() {
                return Err(XC_REPLAY);
            }
        }
        if fresh.snapshot() != self.snapshot() {
            return Err(XC_REPLAY);
        }
        Ok(())
    }
}

/// 判据入口：UNX-F0432。
pub fn run_unx0432_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0432_veccount");
    // 族一：递增与桶读。
    let mut b = VecBook::new();
    cs.add("首跳 1", b.bump(0x20, IrqSrc::Isa) == Ok(1), "A 区首线");
    cs.add("二跳 2", b.bump(0x20, IrqSrc::Isa) == Ok(2), "同桶累加");
    cs.add("桶读对", b.hits(0x20) == Ok(2), "账面");
    cs.add("total=2", b.total == 2, "总递增");
    // 族二：域核。
    cs.add("异常区拒", b.bump(0x03, IrqSrc::Isa) == Err(XC_VEC_OOR), "#PF 区");
    cs.add("哨兵拒", b.bump(0x80, IrqSrc::Generic) == Err(XC_VEC_OOR), "D 区");
    cs.add("域尾过", b.bump(0xFF, IrqSrc::Generic) == Ok(1), "0xFF");
    cs.add("越尾拒", b.hits(0x100) == Err(XC_VEC_OOR), "0x100");
    // 族三：来源标签。
    let mut b2 = VecBook::new();
    let _ = b2.bump(0x50, IrqSrc::Pci);
    cs.add("PCI 标签", b2.buckets[(0x50 - VEC_LO) as usize].last_src == 3, "源 3");
    let _ = b2.bump(0x50, IrqSrc::Generic); // 换源合法
    cs.add("换源续计", b2.hits(0x50) == Ok(2), "计数延续");
    cs.add("标签更新", b2.buckets[(0x50 - VEC_LO) as usize].last_src == 4, "最近源覆盖");
    // 族四：自动归区。
    cs.add("A 区归 Isa", IrqSrc::zone_of(0x2F) == Ok(IrqSrc::Isa), "上界");
    cs.add("B 区归 Lvt", IrqSrc::zone_of(0x30) == Ok(IrqSrc::Lvt), "下界");
    cs.add("C 区归 Pci", IrqSrc::zone_of(0x7F) == Ok(IrqSrc::Pci), "上界");
    cs.add("E 区归 Generic", IrqSrc::zone_of(0x81) == Ok(IrqSrc::Generic), "过哨兵");
    cs.add("哨兵归区拒", IrqSrc::zone_of(0x80).is_err(), "D 区不归");
    // 族五：总账平。
    let mut b3 = VecBook::new();
    for _ in 0..7 {
        let _ = b3.bump(0x21, IrqSrc::Isa);
    }
    for _ in 0..3 {
        let _ = b3.bump(0xB0, IrqSrc::Generic);
    }
    cs.add("总账平", b3.tally_ok().is_ok(), "10=10");
    cs.add("分桶 7/3", b3.hits(0x21) == Ok(7) && b3.hits(0xB0) == Ok(3), "两桶");
    // 族六：A 区心跳巡检。
    let mut b4 = VecBook::new();
    for v in VEC_LO..=ZONE_A_END {
        let _ = b4.bump(v, IrqSrc::Isa);
    }
    cs.add("A 区全活", b4.zone_a_alive().is_ok(), "16 线心跳");
    let mut b5 = VecBook::new();
    for v in VEC_LO..ZONE_A_END {
        let _ = b5.bump(v, IrqSrc::Isa);
    }
    cs.add("A 区断线立案", b5.zone_a_alive() == Err(XC_ZERO_ZONE_A), "0x2F 零桶");
    // 族七：哨兵禁碰。
    cs.add("哨兵禁碰", b3.sentinel_never(), "0x80 双拒");
    // 族八：回绕哨兵。
    let mut b6 = VecBook::new();
    b6.buckets[(0x22 - VEC_LO) as usize].hits = u32::MAX;
    cs.add("回绕立案", b6.bump(0x22, IrqSrc::Isa) == Err(XC_WRAP), "u32 顶");
    // 族九：快照与重放。
    cs.add("快照非零桶", b3.snapshot().len() == 2, "两桶入照");
    cs.add("快照首项", b3.snapshot().first().map(|x| *x == (0x21, 7)).unwrap_or(false), "A 区在前");
    cs.add("重放一致", b3.replay().is_ok(), "同事件序同快照");
    // 族十：空账快照。
    cs.add("空账空照", VecBook::new().snapshot().is_empty(), "全零无照");
    cs.add("空账总平", VecBook::new().tally_ok().is_ok(), "0=0");
    cs
}
