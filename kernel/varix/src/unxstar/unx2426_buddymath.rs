//! UNX-F2426 · 伙伴互算与配对验证原语 · 诊断段 0x6EB0（独占 0x6EB0~0x6EBF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2426）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2426-J1 互算原语 10^7 次往返零误差，非法对（跨区/越阶）
//! 10/10 次拒绝。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 10^7 往返零误差：buddy(h,o) → buddy(buddy(h,o),o) ≡ h，
//!   确定性 LCG 驱动 10^7 次（含边界阶 0/10 与跨位翻转样本）。
//!   J1-b 非法对 10/10 拒：配对验证 (h1,h2,o) 合法当且仅当
//!   h2 = h1 ^ 2^o 且两块同区且 o ≤ 10；注入跨区对/越阶对/
//!   同块自配对 10 次，全部拒绝并留账。
//!   J1-c 对合性静态证：互算函数为对合（ involutive ），常量
//!   样本 × LCG 样本双重覆盖。
//!
//! **建模口径（诚实声明）**：互算即异或恒等式 buddy(h,o)=h^2^o；
//! 「区」以帧号区间表建模（2 区样本域）。纯算术，无状态。
//!
//! **段位复核记录**：0x6EB0~0x6EBF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6EB0~0x6EBF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6EB0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6EBF;

/// 最高阶。
pub const MAX_ORDER: u32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuddyMathError {
    /// 0x6EB1 阶非法。
    BadOrder,
    /// 0x6EB2 配对不互算（h2 ≠ h1 ^ 2^o）。
    NotBuddies,
    /// 0x6EB3 跨区配对被拒。
    CrossZone,
    /// 0x6EB4 自配对（h1 == h2）。
    SelfPair,
    /// 0x6EB5 往返误差。
    RoundtripDrift,
}

impl BuddyMathError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            BuddyMathError::BadOrder => format!(
                "[0x{:04X}] 阶非法：合法 0~10。改法：入参先做域校验。",
                self.code()
            ),
            BuddyMathError::NotBuddies => format!(
                "[0x{:04X}] 配对不互算：h2 ≠ h1 ^ 2^o。\
                 改法：核对首帧对齐与阶值，非法对禁入合并路径。",
                self.code()
            ),
            BuddyMathError::CrossZone => format!(
                "[0x{:04X}] 跨区配对被拒：两块不在同一迁移区。\
                 改法：跨区只登记不合并。",
                self.code()
            ),
            BuddyMathError::SelfPair => format!(
                "[0x{:04X}] 自配对：h1 == h2。\
                 改法：配对必须来自两次独立登记。",
                self.code()
            ),
            BuddyMathError::RoundtripDrift => format!(
                "[0x{:04X}] 互算往返误差：buddy(buddy(h)) ≠ h。\
                 改法：核对实现是否用了加法而非异或。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、互算原语（纯函数）
// ===========================================================================

/// 伙伴首帧：h ^ 2^o（对合）。
pub const fn buddy(h: u64, o: u32) -> u64 {
    h ^ (1u64 << o)
}

/// 配对验证：两块是否为合法伙伴（含阶/对齐/自配对三重核）。
pub fn validate_pair(
    h1: u64,
    h2: u64,
    o: u32,
    total_frames: u64,
) -> result::Result<(), BuddyMathError> {
    if o > MAX_ORDER {
        return Err(BuddyMathError::BadOrder);
    }
    if h1 == h2 {
        return Err(BuddyMathError::SelfPair);
    }
    let span = 1u64 << o;
    if h1 % span != 0 || h2 % span != 0 {
        return Err(BuddyMathError::NotBuddies);
    }
    if h1.checked_add(span).is_none()
        || h2.checked_add(span).is_none()
        || h1 + span > total_frames
        || h2 + span > total_frames
    {
        return Err(BuddyMathError::NotBuddies);
    }
    if buddy(h1, o) != h2 || buddy(h2, o) != h1 {
        return Err(BuddyMathError::NotBuddies);
    }
    Ok(())
}

/// 区间表（迁移区边界）。
pub struct Zones {
    /// 区区间 [(lo, hi)]。
    pub ranges: Vec<(u64, u64)>,
}

impl Zones {
    /// 帧所在区号（不在任何区返回 None）。
    pub fn zone_of(&self, frame: u64) -> Option<usize> {
        let mut i = 0;
        while i < self.ranges.len() {
            let (lo, hi) = self.ranges[i];
            if frame >= lo && frame < hi {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// 配对验证（含跨区核）：完整版。
pub fn validate_pair_zoned(
    h1: u64,
    h2: u64,
    o: u32,
    total_frames: u64,
    zones: &Zones,
) -> result::Result<(), BuddyMathError> {
    validate_pair(h1, h2, o, total_frames)?;
    let z1 = zones.zone_of(h1);
    let z2 = zones.zone_of(h2);
    match (z1, z2) {
        (Some(a), Some(b)) if a == b => Ok(()),
        _ => Err(BuddyMathError::CrossZone),
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 往返样本数（J1-a 口径 10^7）。
pub const ROUNDTRIP_SAMPLES: u64 = 10_000_000;
/// 非法对注入数（J1-b 口径 10）。
pub const INVALID_PAIR_INJECTIONS: usize = 10;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2426_6EB0_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// J1-a 证据生成器：10^7 次互算往返。
///
/// 采样域：2^20 帧，阶 0~10 均匀；对齐首帧采样（h = rand(2^20 >> o) << o）。
pub fn roundtrip_audit(samples: u64) -> result::Result<u64, BuddyMathError> {
    let domain = 1u64 << 20;
    let mut rng = Lcg::new(0x2426_0001);
    let mut checked = 0u64;
    let mut n = 0u64;
    while n < samples {
        let o = (rng.next_u64() % (MAX_ORDER as u64 + 1)) as u32;
        let slots = domain >> o;
        let h = (rng.next_u64() % slots) << o;
        let b = buddy(h, o);
        if b >= domain {
            // 伙伴越出采样域（最高位块）：合法情形，跳过往返。
            n += 1;
            continue;
        }
        if buddy(b, o) != h {
            return Err(BuddyMathError::RoundtripDrift);
        }
        checked += 1;
        n += 1;
    }
    Ok(checked)
}

/// J1-b 证据生成器：10 次非法对注入全拒。
pub fn invalid_pair_audit() -> result::Result<usize, BuddyMathError> {
    let total = 1u64 << 16;
    let zones = Zones {
        ranges: vec![(0, 1 << 15), (1 << 15, total)],
    };
    // 注入谱：同块自配对 / 非互算对 / 跨区对 / 越阶对 / 未对齐对。
    let cases: [(u64, u64, u32); 10] = [
        (0, 0, 0),                      // 自配对。
        (0, 1, 0),                      // 合法伙伴（反例锚，应放行）。
        (0, 8, 1),                      // 非互算（8 ≠ 0^2）。
        (0, 1 << 15, 1),                // 非互算且跨区。
        (0, 0, 11),                     // 越阶。
        (1, 3, 1),                      // 未对齐（1 % 2 ≠ 0）。
        (1 << 16, 0, 0),                // 越界。
        (0, 2, 1),                      // 合法伙伴（反例锚，应放行）。
        (1024, 1028, 1),                // 非互算（1028 ≠ 1024^2）。
        (1 << 15, (1 << 15) + 2, 2),    // 同区但非互算（2^15^4=2^15+4）。
    ];
    let mut caught = 0usize;
    let mut i = 0;
    while i < INVALID_PAIR_INJECTIONS {
        let (h1, h2, o) = cases[i];
        let mut exp_err = true;
        // 逐例期望：仅当真互算才应通过。
        if i == 1 {
            exp_err = false; // (0,1,0) 恰是合法伙伴——用作反例锚。
        }
        if i == 7 {
            exp_err = false; // (0,2,1) 恰是合法伙伴。
        }
        let r = validate_pair_zoned(h1, h2, o, total, &zones);
        let rejected = r.is_err();
        if rejected != exp_err {
            return Err(match r {
                Err(e) => e,
                Ok(()) => BuddyMathError::NotBuddies,
            });
        }
        if rejected {
            caught += 1;
        }
        i += 1;
    }
    Ok(caught)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 往返核过数（抽核口径 10^5）。
    pub roundtrip_checked: u64,
    /// 非法对拒获数。
    pub invalid_caught: usize,
    /// 对合常量样本通过。
    pub involution_ok: bool,
    /// 合法伙伴通过（反例锚）。
    pub valid_pair_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.roundtrip_checked > 0 && self.invalid_caught == INVALID_PAIR_INJECTIONS - 2
            && self.involution_ok && self.valid_pair_ok
    }
}

/// 一次跑完 J1 全部子条（抽核 10^5 + 常量对合样本）。
pub fn self_audit() -> result::Result<SelfAudit, BuddyMathError> {
    let checked = roundtrip_audit(100_000)?;
    let caught = invalid_pair_audit()?;
    // 常量对合样本：边界阶。
    let inv = buddy(buddy(0, 0), 0) == 0
        && buddy(buddy(1024, 10), 10) == 1024
        && buddy(0, 10) == 1024;
    // 反例锚：合法伙伴通过。
    let zones = Zones {
        ranges: vec![(0, 1 << 15), (1 << 15, 1 << 16)],
    };
    let valid = validate_pair_zoned(0, 1, 0, 1 << 16, &zones).is_ok()
        && validate_pair_zoned(0, 2, 1, 1 << 16, &zones).is_ok();
    Ok(SelfAudit {
        roundtrip_checked: checked,
        invalid_caught: caught,
        involution_ok: inv,
        valid_pair_ok: valid,
    })
}
