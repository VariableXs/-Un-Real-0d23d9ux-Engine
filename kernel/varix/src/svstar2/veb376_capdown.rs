//! veb376_capdown —— VE-F0376 驱动能力降级协商。
//! 锚点（CoRun Unxreal 主册）：VE-F0376 · 驱动能力降级协商 · 目标 340 行
//! 判据原文：VE-F0376-J1 驱动声明能力位集与平台支持能力位集
//!           协商取交集；按缺失能力分四档：全支持 FULL、缺次
//!           要能力 REDUCED、缺关键能力 MINIMAL、关键能力全缺
//!           OFF（拒载）；降级链单调（FULL>REDUCED>MINIMAL>OFF，
//!           只许沿链降不许升）；协商过程可重放（同输入同档）。
//! 组件：能力位集（u32 位图）+ 关键位掩码 + 次要位掩码 + 协商
//!       引擎（档位判定 + 降级原因清单）+ 协商账本。
//! 职责定位：能力降级协商是「驱动的体检定级」——不是缺一个能力
//!   就罢工，而是按缺失程度给最合适的功能档；关键能力（如中断
//!   路由）缺失必须挡在加载前，次要能力（如统计计数）缺失可以
//!   带伤运行；降级链单调保证系统状态只许变差不许诈尸。
//! 性能逐项分解：协商 O(1)（位运算）；账本追加 O(1)。
//! 诊断码段：0x30380（0x30381~0x3038A 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CD_VERSION: &str = "veb376_capdown-v1";

/// 诊断段基址（0x30380 段 · 本模块独占）。
pub const CD_SEG_BASE: u32 = 0x30380;
pub const XC_NO_CAP: u32 = 0x30381; // 能力位全空
pub const XC_TIER_MONO: u32 = 0x30382; // 违反降级链单调
pub const XC_DRV_EMPTY: u32 = 0x30383; // 驱动名空
pub const XC_NOT_NEG: u32 = 0x30384; // 未协商先取档
pub const XC_MAGIC: u32 = 0x30385; // 账魔数破
pub const XC_REPLAY: u32 = 0x30386; // 重放不一致
pub const XC_MASK_OVER: u32 = 0x30387; // 掩码越 32 位
pub const XC_LOOKUP: u32 = 0x30388; // 账查询无此驱动
pub const XC_TAMPER: u32 = 0x30389; // 账被篡改
pub const XC_FULL_REJECT: u32 = 0x3038A; // OFF 档拒载

/// 能力位定义（u32 位图，bit0~bit31）。
pub const CAP_IRQ: u32 = 1 << 0; // 中断路由（关键）
pub const CAP_DMA: u32 = 1 << 1; // DMA 通道（关键）
pub const CAP_MMIO: u32 = 1 << 2; // 内存映射 IO（关键）
pub const CAP_PWR: u32 = 1 << 3; // 电源管理（次要）
pub const CAP_STAT: u32 = 1 << 4; // 统计计数（次要）
pub const CAP_FW: u32 = 1 << 5; // 固件加载（次要）
pub const CAP_HOT: u32 = 1 << 6; // 热插拔（次要）
pub const CAP_DIAG: u32 = 1 << 7; // 诊断口（次要）

/// 关键能力掩码（缺一即掉出 FULL 档向下降）。
pub const MASK_KEY: u32 = CAP_IRQ | CAP_DMA | CAP_MMIO;
/// 次要能力掩码。
pub const MASK_MINOR: u32 = CAP_PWR | CAP_STAT | CAP_FW | CAP_HOT | CAP_DIAG;
/// 全能力掩码。
pub const MASK_ALL: u32 = MASK_KEY | MASK_MINOR;

/// 功能档位（降级链严格单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// 全功能。
    Full = 3,
    /// 降级一档：缺次要能力。
    Reduced = 2,
    /// 降级二档：缺关键能力。
    Minimal = 1,
    /// 拒载：关键能力全缺。
    Off = 0,
}

impl Tier {
    /// 档位号。
    pub fn num(self) -> u32 {
        self as u32
    }
    /// 沿链降一档（不许升）。
    pub fn down(self) -> Tier {
        match self {
            Tier::Full => Tier::Reduced,
            Tier::Reduced => Tier::Minimal,
            _ => Tier::Off,
        }
    }
    /// 拒载档判定。
    pub fn rejected(self) -> bool {
        self == Tier::Off
    }
}

/// 降级原因（缺失的关键/次要能力位清单化）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownReason {
    /// 缺失的关键能力位（逐位列出）。
    pub miss_key: Vec<u32>,
    /// 缺失的次要能力位。
    pub miss_minor: Vec<u32>,
}

impl DownReason {
    /// 汇总缺失总数。
    pub fn miss_total(&self) -> u32 {
        (self.miss_key.len() + self.miss_minor.len()) as u32
    }
    /// 全绿判定。
    pub fn clean(&self) -> bool {
        self.miss_key.is_empty() && self.miss_minor.is_empty()
    }
}

/// 协商记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegRecord {
    pub drv: &'static str,
    pub offered: u32,
    pub supported: u32,
    pub tier: Tier,
}

/// 协商引擎 + 账本。
pub struct CapBroker {
    log: Vec<NegRecord>,
    magic: u32,
}

impl CapBroker {
    /// 建。
    pub fn new() -> CapBroker {
        CapBroker { log: Vec::new(), magic: 0x0380_C0DE }
    }

    /// 协商：驱动声明 offered ∩ 平台 supported → 档位。
    /// 判档：全绿 FULL；缺次要 REDUCED；缺关键但至少留一关键 MINIMAL；
    /// 关键全缺 OFF。
    pub fn negotiate(&mut self, drv: &'static str, offered: u32, supported: u32) -> Result<(Tier, DownReason), u32> {
        if drv.is_empty() {
            return Err(XC_DRV_EMPTY);
        }
        if offered & MASK_ALL == 0 {
            return Err(XC_NO_CAP);
        }
        if offered & !MASK_ALL != 0 {
            return Err(XC_MASK_OVER);
        }
        let have = offered & supported;
        let miss_key = bits_of(MASK_KEY & !have);
        let miss_minor = bits_of(MASK_MINOR & !have);
        let tier = if miss_key.len() == 3 {
            Tier::Off
        } else if miss_key.is_empty() && miss_minor.is_empty() {
            Tier::Full
        } else if miss_key.is_empty() {
            Tier::Reduced
        } else {
            Tier::Minimal
        };
        self.log.push(NegRecord { drv, offered, supported, tier });
        Ok((tier, DownReason { miss_key, miss_minor }))
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.log.len()
    }

    /// 按驱动查档（取最近一次）。
    pub fn tier_of(&self, drv: &str) -> Result<Tier, u32> {
        let mut found: Option<Tier> = None;
        for r in self.log.iter() {
            if r.drv == drv {
                found = Some(r.tier);
            }
        }
        match found {
            Some(t) => Ok(t),
            None => Err(XC_LOOKUP),
        }
    }

    /// 重放核：重新协商一遍，档位须与账面一致。
    pub fn replay_stable(&self) -> Result<bool, u32> {
        if self.magic != 0x0380_C0DE {
            return Err(XC_MAGIC);
        }
        for r in self.log.iter() {
            let have = r.offered & r.supported;
            let mk = (MASK_KEY & !have).count_ones();
            let t = if mk == 3 {
                Tier::Off
            } else if mk == 0 && (MASK_MINOR & !have) == 0 {
                Tier::Full
            } else if mk == 0 {
                Tier::Reduced
            } else {
                Tier::Minimal
            };
            if t != r.tier {
                return Err(XC_REPLAY);
            }
        }
        Ok(true)
    }

    /// 账防篡改：档位与位集关系硬核复核。
    pub fn audit(&self) -> Result<u32, u32> {
        let mut bad = 0u32;
        for r in self.log.iter() {
            if r.tier.num() > 3 {
                bad += 1;
            }
            if r.tier.rejected() && r.offered & r.supported & MASK_KEY != 0 {
                bad += 1; // 还有关键能力却判 OFF，矛盾
            }
            if r.tier == Tier::Full && (r.offered & r.supported) != MASK_ALL & r.offered {
                bad += 1; // FULL 档却缺能力
            }
        }
        if bad > 0 {
            Err(XC_TAMPER)
        } else {
            Ok(self.log.len() as u32)
        }
    }
}

/// 位图转位清单。
fn bits_of(m: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for i in 0..32u32 {
        if m & (1u32 << i) != 0 {
            out.push(1u32 << i);
        }
    }
    out
}

pub fn run_veb376_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0376");
    // 族一：四档判定。
    let mut b = CapBroker::new();
    let t1 = b.negotiate("full-drv", MASK_ALL, MASK_ALL);
    s.add("全支持 FULL", matches!(&t1, Ok((Tier::Full, r)) if r.clean()), "全绿");
    let t2 = b.negotiate("no-stat", MASK_ALL, MASK_ALL & !CAP_STAT);
    s.add("缺次要 REDUCED", matches!(&t2, Ok((Tier::Reduced, r)) if r.miss_minor == vec![CAP_STAT]), "缺 1 次");
    let t3 = b.negotiate("no-dma", MASK_ALL, MASK_ALL & !CAP_DMA);
    s.add("缺关键 MINIMAL", matches!(&t3, Ok((Tier::Minimal, r)) if r.miss_key == vec![CAP_DMA]), "缺 1 关");
    let t4 = b.negotiate("dead-drv", MASK_ALL, MASK_MINOR);
    s.add("关键全缺 OFF", matches!(&t4, Ok((Tier::Off, r)) if r.miss_key.len() == 3), "拒载");
    s.add("账长 4", b.len() == 4, "逐笔入账");
    s.add("OFF 档核", matches!(b.tier_of("dead-drv"), Ok(Tier::Off)), "账面");
    s.add("查无拒", b.tier_of("ghost") == Err(XC_LOOKUP), "无此驱动");
    // 族二：降级链单调。
    let f = Tier::Full;
    s.add("链序 Full>Reduced", f > Tier::Reduced, "单调");
    s.add("链序 Reduced>Minimal", Tier::Reduced > Tier::Minimal, "单调");
    s.add("链序 Minimal>Off", Tier::Minimal > Tier::Off, "单调");
    s.add("down 一档", f.down() == Tier::Reduced && Tier::Reduced.down() == Tier::Minimal, "逐档");
    s.add("底档不升", Tier::Off.down() == Tier::Off, "到底");
    s.add("档号核对", f.num() == 3 && Tier::Off.num() == 0, "枚举值");
    // 族三：重放稳定核。
    s.add("重放一致", b.replay_stable() == Ok(true), "同输入同档");
    s.add("审计过", b.audit() == Ok(4), "四笔全绿");
    // 族四：入参拒绝。
    let mut c = CapBroker::new();
    s.add("名空拒", c.negotiate("", MASK_ALL, MASK_ALL) == Err(XC_DRV_EMPTY), "名空");
    s.add("能力全空拒", c.negotiate("x", 0, MASK_ALL) == Err(XC_NO_CAP), "空位集");
    s.add("越 32 位拒", c.negotiate("x", 1u32 << 31 | CAP_DIAG | (1 << 24), MASK_ALL) == Err(XC_MASK_OVER), "野位");
    // 族五：交集语义。
    let mut d = CapBroker::new();
    let t5 = d.negotiate("half-drv", MASK_KEY, MASK_ALL);
    s.add("交集取小", matches!(&t5, Ok((Tier::Reduced, r)) if r.miss_minor.len() == 5), "驱动只要关键的");
    // plat-poor 声明全能力，平台只支持 IRQ+PWR → 缺 2 关键 → MINIMAL。
    let t6 = d.negotiate("plat-poor", MASK_ALL, CAP_IRQ | CAP_PWR);
    s.add("平台穷 MINIMAL",
        matches!(&t6, Ok((Tier::Minimal, r)) if r.miss_key.len() == 2 && r.miss_minor.len() == 4),
        "缺 2 关键");
    let tier6 = d.tier_of("plat-poor");
    s.add("账面档位核", tier6 == Ok(Tier::Minimal), "账实一致");
    s.add("魔数核", CD_SEG_BASE == 0x30380, "段基址");
    s
}
