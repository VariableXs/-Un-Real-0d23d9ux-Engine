//! unx0469_pciecap —— UNX-F0469 链路能力账（Link Cap/Status 只读）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0469 · 链路能力账 · 目标 220 行
//! 判据原文：UNX-F0469-J1 PCIe 能力（ID 0x10）结构账：Link
//!           Cap @+0x0C（最大链宽位 9:4、最大速率位 3:0）、
//!           Link Status @+0x12（协商链宽位 9:4、协商速率位
//!           3:0）；账二 协商不变量：协商宽度 ≤ 最大宽度、
//!           协商速率 ≤ 最大速率（超额 = 固件谎报立案）；账
//!           三 速率语义：1=2.5GT/s、2=5.0、3=8.0、4=16.0、
//!           5=32.0（Gen5）；账四 链宽语义：1/2/4/8/12/16/32
//!           合法集合（x3/x5 等非法值立案）；账五 只读纪律：
//!           全解析零写。
//! 组件：链路位域拆解器 + 协商不变量核 + 合法集合闸。
//! 职责定位：链路能力是 PCIe 的「车管所登记」——最大值是车
//!   的设计上限，协商值是实际跑起来的档位；协商比最大还大
//!   就是物理层在说谎，宽度落在合法离散集合之外就是寄存器
//!   域被踩了。
//! 性能逐项分解：拆解 O(1)；集合核 O(1)。
//! 诊断码段：0x34580（0x34581~0x34589 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34580 起）。
pub const SEG_BASE: u32 = 0x34580;
pub const XC_NEG_OVER: u32 = SEG_BASE + 1; // 协商超最大
pub const XC_LANES: u32 = SEG_BASE + 2; // 链宽非法值

/// PCIe 能力结构内偏移。
pub const OFF_LNK_CAP: usize = 0x0C;
pub const OFF_LNK_STA: usize = 0x12;

/// 链路投影。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkInfo {
    pub max_lanes: u8,
    pub max_speed: u8,
    pub neg_lanes: u8,
    pub neg_speed: u8,
}

impl LinkInfo {
    /// 从 Link Cap（u32）与 Link Status（u16）拆解。
    pub fn decode(cap: u32, sta: u16) -> LinkInfo {
        LinkInfo {
            max_lanes: ((cap >> 4) & 0x3F) as u8,
            max_speed: (cap & 0xF) as u8,
            neg_lanes: ((sta >> 4) & 0x3F) as u8,
            neg_speed: (sta & 0xF) as u8,
        }
    }

    /// 协商不变量核。
    pub fn coherent(&self) -> Result<(), u32> {
        if self.neg_lanes > self.max_lanes || self.neg_speed > self.max_speed {
            return Err(XC_NEG_OVER);
        }
        Ok(())
    }

    /// 链宽合法集合核：{0,1,2,4,8,12,16,32}（0 = 下电链路合法）。
    pub fn lanes_ok(&self) -> Result<(), u32> {
        for v in [self.max_lanes, self.neg_lanes] {
            match v {
                0 | 1 | 2 | 4 | 8 | 12 | 16 | 32 => {}
                _ => return Err(XC_LANES),
            }
        }
        Ok(())
    }

    /// 速率语义投影（GT/s ×10）。
    pub fn speed_gts10(&self) -> u16 {
        match self.neg_speed {
            1 => 25,
            2 => 50,
            3 => 80,
            4 => 160,
            5 => 320,
            _ => 0,
        }
    }
}

/// 从能力结构体（含 PCIe cap 起始）拆链路面（只读）。
pub fn extract(cap_struct: &[u8]) -> Option<LinkInfo> {
    if cap_struct.len() < OFF_LNK_STA + 2 {
        return None;
    }
    let cap = u32::from_le_bytes([
        cap_struct[OFF_LNK_CAP],
        cap_struct[OFF_LNK_CAP + 1],
        cap_struct[OFF_LNK_CAP + 2],
        cap_struct[OFF_LNK_CAP + 3],
    ]);
    let sta = u16::from_le_bytes([cap_struct[OFF_LNK_STA], cap_struct[OFF_LNK_STA + 1]]);
    Some(LinkInfo::decode(cap, sta))
}

/// 判据入口：UNX-F0469 全族核。
pub fn run_unx0469_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0469");

    // 夹具：x16 Gen4（max），协商 x8 Gen3。
    let mk = |cap: u32, sta: u16| -> LinkInfo { LinkInfo::decode(cap, sta) };
    let cap = (16u32 << 4) | 4u32;
    let sta = (8u16 << 4) | 3u16;
    let li = mk(cap, sta);

    // 族一：拆解。
    cs.add("最大宽 x16", li.max_lanes == 16, "位 9:4");
    cs.add("最大速 Gen4", li.max_speed == 4, "位 3:0");
    cs.add("协商宽 x8", li.neg_lanes == 8, "位 9:4");
    cs.add("协商速 Gen3", li.neg_speed == 3, "位 3:0");

    // 族二：不变量。
    cs.add("协商 ≤ 最大过", li.coherent().is_ok(), "x8≤x16 Gen3≤Gen4");
    let over = mk((2u32 << 4) | 2u32, (4u16 << 4) | 4u16);
    cs.add("宽超立案", over.coherent() == Err(XC_NEG_OVER), "x4>x2");
    let over2 = mk((8u32 << 4) | 1u32, (4u16 << 4) | 3u16);
    cs.add("速超立案", over2.coherent() == Err(XC_NEG_OVER), "Gen3>Gen1");

    // 族三：链宽合法集合。
    cs.add("x16 合法", li.lanes_ok().is_ok(), "集合内");
    let bad = mk((3u32 << 4) | 4u32, 0u16); // max x3 非法
    cs.add("x3 立案", bad.lanes_ok() == Err(XC_LANES), "集合外");
    let zero = mk(0, 0);
    cs.add("下电链合法", zero.lanes_ok().is_ok(), "全零");
    let l12 = mk((12u32 << 4) | 4u32, 0u16);
    cs.add("x12 合法", l12.lanes_ok().is_ok(), "离散集内");
    let l20 = mk((20u32 << 4) | 4u32, 0u16);
    cs.add("x20 立案", l20.lanes_ok() == Err(XC_LANES), "非标宽");

    // 族四：速率语义。
    cs.add("Gen1 2.5", mk(0, 1).speed_gts10() == 25, "×10");
    cs.add("Gen3 8.0", li.speed_gts10() == 80, "Gen3");
    cs.add("Gen5 32.0", mk(0, 5).speed_gts10() == 320, "顶格");
    cs.add("Gen0 未知", mk(0, 0).speed_gts10() == 0, "无协商");

    // 族五：结构体提取。
    let mut buf = [0u8; 0x20];
    buf[OFF_LNK_CAP..OFF_LNK_CAP + 4].copy_from_slice(&cap.to_le_bytes());
    buf[OFF_LNK_STA..OFF_LNK_STA + 2].copy_from_slice(&sta.to_le_bytes());
    let ext = extract(&buf);
    cs.add("结构体提取", ext == Some(li), "字节面读出");
    cs.add("短结构 None", extract(&buf[..0x10]).is_none(), "不足 Sta");

    // 族六：只读纪律（软证：提取不改输入）。
    let mut buf2 = buf;
    let _ = extract(&mut buf2);
    cs.add("提取零写", buf == buf2, "字节全等");

    cs
}
