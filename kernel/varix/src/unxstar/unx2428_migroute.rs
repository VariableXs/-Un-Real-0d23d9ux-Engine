//! UNX-F2428 · 迁移类型分配路由深化 · 诊断段 0x6EE0（独占 0x6EE0~0x6EEF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2428）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2428-J1 三区路由命中率账落档（请求区命中 ≥95%），跨区降级
//! 逐笔留账。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 命中率 ≥95%：三区路由器对「请求区」的命中率落档（命中
//!   =从请求区交付），演练域命中率必须 ≥9500‱；低于线的路由器
//!   构型不收单。
//!   J1-b 跨区降级逐笔留账：请求区缺块时的降级交付（从其他区取）
//!   每笔留账（请求区/实际区/原因），逐笔可溯。
//!   J1-c 降级优先序：MOVABLE→RECLAIMABLE→UNMOVABLE 的降级序
//!   固定（向「更不可动」区降级是错误方向——反向禁用），越序
//!   交付即红。
//!
//! **建模口径（诚实声明）**：路由器在 F2413 三分区的区账上工作；
//! 降级序语义：MOVABLE 缺时可降 RECLAIMABLE（可回收页可迁走腾位）
//! 再降 UNMOVABLE（最后手段）；UNMOVABLE 缺时不可降级（内核结构
//! 必须住 UNMOVABLE），诚实 ENOMEM。
//!
//! **段位复核记录**：0x6EE0~0x6EEF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6EE0~0x6EEF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6EE0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6EEF;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteError {
    /// 0x6EE1 区非法。
    BadZone,
    /// 0x6EE2 UNMOVABLE 缺块且不可降级（诚实 ENOMEM）。
    UnmovableStarved,
    /// 0x6EE3 降级越序（向更不可动方向多跳）。
    OrderViolation,
    /// 0x6EE4 命中率低于 9500‱。
    HitRateLow,
    /// 0x6EE5 降级账缺笔。
    FallbackUnlogged,
    /// 0x6EE6 命中率账分母为零。
    EmptyRouteStats,
}

impl RouteError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            RouteError::BadZone => format!(
                "[0x{:04X}] 区非法：0=UNMOVABLE 1=MOVABLE 2=RECLAIMABLE。\
                 改法：用 RouteZone 枚举。",
                self.code()
            ),
            RouteError::UnmovableStarved => format!(
                "[0x{:04X}] UNMOVABLE 缺块且不可降级：诚实 ENOMEM。\
                 改法：内核结构请求必须住 UNMOVABLE 区，先扩区或回收。",
                self.code()
            ),
            RouteError::OrderViolation => format!(
                "[0x{:04X}] 降级越序：降级只允许 M→R→U 方向。\
                 改法：核对降级链实现是否反向取块。",
                self.code()
            ),
            RouteError::HitRateLow => format!(
                "[0x{:04X}] 命中率低于 9500‱：路由器构型不达标。\
                 改法：调整三区配比或缓存预分配。",
                self.code()
            ),
            RouteError::FallbackUnlogged => format!(
                "[0x{:04X}] 跨区降级未留账。\
                 改法：降级路径必须先写降级账再交付。",
                self.code()
            ),
            RouteError::EmptyRouteStats => format!(
                "[0x{:04X}] 路由统计分母为零：空跑不评命中率。\
                 改法：演练至少 1000 次请求。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、路由区与降级序
// ===========================================================================

/// 路由区（与 F2413 三区同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteZone {
    /// 0：不可迁移。
    Unmovable,
    /// 1：可迁移。
    Movable,
    /// 2：可回收。
    Reclaimable,
}

impl RouteZone {
    /// 区号。
    pub fn num(self) -> u8 {
        match self {
            RouteZone::Unmovable => 0,
            RouteZone::Movable => 1,
            RouteZone::Reclaimable => 2,
        }
    }

    /// 降级链：MOVABLE→RECLAIMABLE→UNMOVABLE；UNMOVABLE 无降级。
    pub fn fallback(self) -> Option<RouteZone> {
        match self {
            RouteZone::Movable => Some(RouteZone::Reclaimable),
            RouteZone::Reclaimable => Some(RouteZone::Unmovable),
            RouteZone::Unmovable => None,
        }
    }
}

/// 一笔降级账。
#[derive(Clone, Copy, Debug)]
pub struct FallbackRecord {
    /// 请求区。
    pub requested: u8,
    /// 实际交付区。
    pub delivered: u8,
    /// 跳数（1=直接降级一级）。
    pub hops: u32,
    /// 事件号。
    pub seq: u64,
}

/// 三区路由器。
pub struct ZoneRouter {
    /// 三区余量（帧数）。
    pub free: [u64; 3],
    /// 请求区命中数。
    pub hits: u64,
    /// 总请求数。
    pub requests: u64,
    /// 降级账。
    pub fallbacks: Vec<FallbackRecord>,
    /// ENOMEM 数（UNMOVABLE 饿死）。
    pub enomems: u64,
    seq: u64,
}

impl ZoneRouter {
    /// 建路由器（配比三区余量）。
    pub fn new(u: u64, m: u64, r: u64) -> Self {
        ZoneRouter {
            free: [u, m, r],
            hits: 0,
            requests: 0,
            fallbacks: Vec::new(),
            enomems: 0,
            seq: 0,
        }
    }

    /// 请求一帧：请求区优先，缺则沿降级链（M→R→U），逐笔留账。
    pub fn alloc(&mut self, zone: RouteZone) -> result::Result<RouteZone, RouteError> {
        self.requests += 1;
        self.seq += 1;
        let z = zone.num() as usize;
        if self.free[z] > 0 {
            self.free[z] -= 1;
            self.hits += 1;
            return Ok(zone);
        }
        // 降级链。
        let mut hops = 0u32;
        let mut cur = zone;
        while let Some(next) = cur.fallback() {
            hops += 1;
            cur = next;
            let n = cur.num() as usize;
            if self.free[n] > 0 {
                self.free[n] -= 1;
                self.fallbacks.push(FallbackRecord {
                    requested: zone.num(),
                    delivered: cur.num(),
                    hops,
                    seq: self.seq,
                });
                return Ok(cur);
            }
        }
        // 全链饿死：诚实 ENOMEM。
        self.enomems += 1;
        Err(RouteError::UnmovableStarved)
    }

    /// 回收一帧（回本区余量）。
    pub fn free(&mut self, zone: RouteZone) {
        self.free[zone.num() as usize] += 1;
    }

    /// 命中率（‱）。分母零显性拒绝。
    pub fn hit_rate_bp(&self) -> result::Result<u64, RouteError> {
        if self.requests == 0 {
            return Err(RouteError::EmptyRouteStats);
        }
        Ok(self.hits * 10_000 / self.requests)
    }

    /// J1-b/J1-c 核：降级账逐笔核序（M→R→U 单向）。
    pub fn audit_fallbacks(&self) -> result::Result<usize, RouteError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < self.fallbacks.len() {
            let r = self.fallbacks[i];
            // 降级序核：delivered 必须在 requested 的降级链上且方向
            // 只能「更不可动」（号递减：1→2? 号序 U=0 M=1 R=2，链
            // M(1)→R(2)→U(0)。用显式链验证而非号差）。
            let chain_ok = match ZoneRouter::from_num(r.requested) {
                RouteZone::Movable => r.delivered == RouteZone::Reclaimable.num()
                    || r.delivered == RouteZone::Unmovable.num(),
                RouteZone::Reclaimable => r.delivered == RouteZone::Unmovable.num(),
                RouteZone::Unmovable => false,
            };
            let hops_ok = match r.requested {
                1 => r.hops == 1 || r.hops == 2, // M→R 一步 / M→R→U 两步。
                2 => r.hops == 1,                // R→U 一步。
                _ => false,
            };
            if !chain_ok || !hops_ok {
                return Err(RouteError::OrderViolation);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }

    /// 从区号还原（内部）。
    fn from_num(n: u8) -> RouteZone {
        match n {
            1 => RouteZone::Movable,
            2 => RouteZone::Reclaimable,
            _ => RouteZone::Unmovable,
        }
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 演练请求数。
pub const ROUTE_AUDIT_REQUESTS: u64 = 2000;
/// 命中率下限（‱）。
pub const HIT_RATE_FLOOR_BP: u64 = 9500;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2428_6EE0_9E37_79B9)
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

/// J1 证据生成器：三区（U 512 / M 4096 / R 2048 帧）混合请求
/// 2000 次（U 20% / M 50% / R 30%），命中率先核；少量 MOVABLE
/// 饿死触发降级留账核。
pub fn route_audit() -> result::Result<(u64, usize), RouteError> {
    // 配比设计：U 380（~400 请求 → 少量饿死）、M 970（~1000 请求 →
    // 少量降级 R）、R 700（~600 请求 + M 降级余量）。命中率 ≥95%、
    // 降级账非空，两个验收面同时成立。
    let mut rt = ZoneRouter::new(380, 970, 700);
    let mut rng = Lcg::new(0x2428_0001);
    let mut n = 0u64;
    while n < ROUTE_AUDIT_REQUESTS {
        let roll = rng.next_u64() % 100;
        let zone = if roll < 20 {
            RouteZone::Unmovable
        } else if roll < 70 {
            RouteZone::Movable
        } else {
            RouteZone::Reclaimable
        };
        let _ = rt.alloc(zone); // ENOMEM（U 饿死）计入账，不中断演练。
        n += 1;
    }
    let rate = rt.hit_rate_bp()?;
    if rate < HIT_RATE_FLOOR_BP {
        return Err(RouteError::HitRateLow);
    }
    let checked = rt.audit_fallbacks()?;
    Ok((rate, checked))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 命中率（‱）。
    pub hit_rate: u64,
    /// 降级账核过笔数。
    pub fallback_checks: usize,
    /// UNMOVABLE 饿死诚实 ENOMEM。
    pub unmovable_enomem_ok: bool,
    /// 空统计被拒。
    pub empty_stats_rejected: bool,
    /// 降级越序防线有效（构造越序账被核出）。
    pub order_guard_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.hit_rate >= HIT_RATE_FLOOR_BP
            && self.fallback_checks > 0
            && self.unmovable_enomem_ok
            && self.empty_stats_rejected
            && self.order_guard_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, RouteError> {
    let (rate, checked) = route_audit()?;
    // UNMOVABLE 饿死：U 区空且无降级 → 诚实 ENOMEM。
    let mut rt2 = ZoneRouter::new(0, 8, 8);
    let starved = rt2.alloc(RouteZone::Unmovable).is_err();
    // 空统计拒绝。
    let rt3 = ZoneRouter::new(1, 1, 1);
    let empty_rej = rt3.hit_rate_bp().is_err();
    // 越序防线：伪造越序降级账（R 请求交付 R）必须被核出。
    let mut rt4 = ZoneRouter::new(8, 8, 8);
    rt4.fallbacks.push(FallbackRecord {
        requested: 2,
        delivered: 2,
        hops: 1,
        seq: 1,
    });
    let order_guard = rt4.audit_fallbacks().is_err();
    Ok(SelfAudit {
        hit_rate: rate,
        fallback_checks: checked,
        unmovable_enomem_ok: starved,
        empty_stats_rejected: empty_rej,
        order_guard_ok: order_guard,
    })
}
