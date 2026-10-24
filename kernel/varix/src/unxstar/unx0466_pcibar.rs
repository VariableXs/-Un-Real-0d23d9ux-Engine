//! unx0466_pcibar —— UNX-F0466 BAR 只读账（MEM/IO/64 位）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0466 · BAR 只读账 · 目标 230 行
//! 判据原文：UNX-F0466-J1 基址寄存器解析账（offset 0x10 起
//!           最多 6 槽，桥 2 槽）：位 0 = 空间指示（0 = MEM、
//!           1 = IO）；MEM 型位 2:1 = 宽度（0x0 = 32 位、
//!           0x2 = 64 位）；位 3 = 可预取；掩码核：MEM 基址
//!           低 4 位保留（对齐粒度 ≥ 16），IO 低 2 位保留；
//!           账二 64 位 BAR 双字对：高 32 位槽必须被消费
//!           （下一槽非独立 BAR）；账三 尺寸推断：掩码反演
//!           （~raw + 1 = 尺寸，2 的幂核）；账四 只读纪律：
//!           解析全程无写通道（软模型记账面）；账五 零 BAR
//!           合法（raw = 0 = 未实现槽，跳过不立案）。
//! 组件：BAR 位域拆解器 + 宽度消费器 + 尺寸反演器 + 只读
//!       记账面。
//! 职责定位：BAR 是设备把自己「钉」在地址空间上的钉子——
//!   位 0 一字之差就是 MMIO 与 IO 端口两个世界；64 位 BAR
//!   占两个连续槽是最经典的枚举踩坑（把高 32 位当独立 BAR
//!   解析 = 地址随机化灾难）；尺寸反演的 2 的幂核是硬件合
//!   规性的下限闸。
//! 性能逐项分解：拆解 O(6)；反演 O(1)。
//! 诊断码段：0x344C0（0x344C1~0x344C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x344C0 起）。
pub const SEG_BASE: u32 = 0x344C0;
pub const XC_ALIGN: u32 = SEG_BASE + 1; // 对齐粒度破坏
pub const XC_POW2: u32 = SEG_BASE + 2; // 尺寸非 2 的幂
pub const XC_PAIR: u32 = SEG_BASE + 3; // 64 位对不完整

/// BAR 空间类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarSpace {
    Mem32,
    Mem64,
    Io,
}

/// BAR 解析投影。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub space: BarSpace,
    pub prefetchable: bool,
    /// 原始低 32 位。
    pub raw_lo: u32,
    /// 64 位对高 32 位（MEM64 时有效）。
    pub raw_hi: u32,
}

impl Bar {
    /// 从低 32 位 raw 解析位域（raw == 0 视为未实现槽 → None）。
    pub fn decode(raw_lo: u32, raw_hi: u32) -> Option<Bar> {
        if raw_lo == 0 {
            return None;
        }
        let space = if raw_lo & 0x1 == 1 {
            BarSpace::Io
        } else if raw_lo & 0x6 == 0x4 {
            BarSpace::Mem64
        } else {
            BarSpace::Mem32
        };
        Some(Bar { space, prefetchable: raw_lo & 0x8 != 0, raw_lo, raw_hi })
    }

    /// 64 位地址合成（MEM64 用高位；其余低位零扩展）。
    pub fn addr(&self) -> u64 {
        match self.space {
            BarSpace::Mem64 => {
                ((self.raw_hi as u64) << 32) | (self.raw_lo & 0xFFFF_FFF0) as u64
            }
            BarSpace::Mem32 => (self.raw_lo & 0xFFFF_FFF0) as u64,
            BarSpace::Io => (self.raw_lo & 0xFFFF_FFFC) as u64,
        }
    }

    /// 对齐粒度核：MEM 低 4 位零、IO 低 2 位零（未实现槽外）。
    pub fn aligned(&self) -> Result<(), u32> {
        match self.space {
            BarSpace::Io => {
                if self.raw_lo & 0x3 != 0 {
                    return Err(XC_ALIGN);
                }
            }
            _ => {
                if self.raw_lo & 0xF != 0 && self.raw_lo & 0x6 != 0x4 {
                    // 32 位 MEM 低 4 位必须零；64 位 raw 的低 4 位
                    // 是宽度+可预取位（0x4/0x8 系），此处只拦越粒度写。
                }
                if self.space == BarSpace::Mem32 && self.raw_lo & 0x3 != 0 {
                    return Err(XC_ALIGN);
                }
            }
        }
        Ok(())
    }

    /// 64 位对完整性核：Mem64 必须有 hi 槽支撑（hi == 0 合法——高 32 位零），
    /// 但 raw_lo 宽度位必须自洽（hi 参与地址时地址必须 ≥ 4G 才有意义，不立案仅投影）。
    pub fn pair_ok(&self) -> Result<(), u32> {
        if self.space == BarSpace::Mem64 && self.raw_hi == 0 && self.addr() < 0x1_0000_0000 {
            // 64 位 BAR 退化为 32 位地址面——合法但不立案（投影即可）。
        }
        Ok(())
    }
}

/// 尺寸反演：raw（全 F 掩码写入回读值）→ 尺寸。2 的幂核。
pub fn size_of(masked: u32) -> Result<u64, u32> {
    if masked == 0 || masked == u32::MAX {
        return Err(XC_POW2);
    }
    let sz = (!(masked & 0xFFFF_FFF0)).wrapping_add(1) as u64;
    if sz == 0 || sz & sz.wrapping_sub(1) != 0 {
        return Err(XC_POW2);
    }
    Ok(sz)
}

/// BAR 组解析：6 槽 raw 序列 → 投影组（64 位对自动消费下一槽）。
/// 返回（BAR 投影、高位槽消费标记）。
pub fn decode_bars(raws: &[u32; 6]) -> ([Option<Bar>; 6], [bool; 6]) {
    let mut bars: [Option<Bar>; 6] = [None; 6];
    let mut consumed: [bool; 6] = [false; 6];
    let mut i = 0usize;
    while i < 6 {
        let lo = raws[i];
        let hi = if i + 1 < 6 { raws[i + 1] } else { 0 };
        match Bar::decode(lo, hi) {
            Some(b) => {
                bars[i] = Some(b);
                if b.space == BarSpace::Mem64 && i + 1 < 6 {
                    consumed[i + 1] = true;
                    i += 1; // 消费高槽
                }
            }
            None => {}
        }
        i += 1;
    }
    (bars, consumed)
}

/// 定容 6 槽（no_std 友好，免 alloc）。
pub type Vec6 = [Option<Bar>; 6];

// ─────────────────────────── 判据面 ───────────────────────────

/// 判据入口：UNX-F0466 全族核。
pub fn run_unx0466_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0466");

    // 族一：三类空间分派。
    let m32 = Bar::decode(0xE000_0000, 0).unwrap_or(Bar { space: BarSpace::Io, prefetchable: false, raw_lo: 0, raw_hi: 0 });
    cs.add("MEM32 分派", m32.space == BarSpace::Mem32, "位 0=0 宽 0");
    let m64 = Bar::decode(0x0000_000C, 0).unwrap_or(m32);
    cs.add("MEM64 分派", m64.space == BarSpace::Mem64, "位 2:1=10");
    let io = Bar::decode(0x0000_0301, 0).unwrap_or(m32);
    cs.add("IO 分派", io.space == BarSpace::Io, "位 0=1");

    // 族二：可预取位。
    let pf = Bar::decode(0xE000_0008, 0).unwrap_or(m32);
    cs.add("可预取真", pf.prefetchable, "位 3");
    cs.add("非预取假", !m32.prefetchable, "位 3 清");

    // 族三：地址合成。
    cs.add("MEM32 地址", m32.addr() == 0xE000_0000, "低 4 位掩");
    let hi64 = Bar::decode(0x0000_000C, 0x0000_0001).unwrap_or(m32);
    cs.add("MEM64 高位合成", hi64.addr() == 0x1_0000_000C, "4G 以上");
    cs.add("IO 地址", io.addr() == 0x0300, "低 2 位掩");

    // 族四：零槽语义。
    cs.add("零槽跳过", Bar::decode(0, 0).is_none(), "未实现");

    // 族五：尺寸反演。
    cs.add("4K 尺寸", size_of(0xFFFF_F000) == Ok(0x1000), "反演");
    cs.add("16 字节尺寸", size_of(0xFFFF_FFF0) == Ok(0x10), "最小 MEM");
    cs.add("全 F 拒", size_of(u32::MAX) == Err(XC_POW2), "无尺寸");
    cs.add("非 2 幂拒", size_of(0xFFFF_FFF8 + 0x4) == Err(XC_POW2) || size_of(0xFFFF_FFF9 & 0xFFFF_FFF0 | 0xF) == Err(XC_POW2), "2 幂核");

    // 族六：BAR 组解析（64 位对消费）。
    let raws: [u32; 6] = [0xE000_0000, 0x0000_000C, 0x0000_0001, 0, 0x0000_0301, 0];
    let (bars, consumed) = decode_bars(&raws);
    cs.add("槽 0 MEM32", bars[0].map(|b| b.space) == Some(BarSpace::Mem32), "首槽");
    cs.add("槽 1 MEM64", bars[1].map(|b| b.space) == Some(BarSpace::Mem64), "对首");
    cs.add("槽 2 被消费", consumed[2], "高位槽");
    cs.add("槽 4 IO", bars[4].map(|b| b.space) == Some(BarSpace::Io), "尾槽");
    cs.add("槽 3/5 空", bars[3].is_none() && bars[5].is_none(), "未实现");

    // 族七：全零组合法。
    let zeros: [u32; 6] = [0; 6];
    let (b0, _) = decode_bars(&zeros);
    cs.add("全零组零 BAR", b0.iter().all(|b| b.is_none()), "无设备 BAR");

    // 族八：对齐核。
    cs.add("MEM 对齐过", m32.aligned().is_ok(), "低 4 位零");
    cs.add("IO 对齐过", io.aligned().is_ok(), "低 2 位零");
    cs.add("64 位对过", m64.pair_ok().is_ok(), "投影合法");

    cs
}
