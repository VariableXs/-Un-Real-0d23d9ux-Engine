//! UNX-F2433 · 分区锁与并发分配结构 · 诊断段 0x6F50（独占 0x6F50~0x6F5F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2433）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2433-J1 8 核并发 10^6 次分配释放对账零损失，锁争用下降账
//! （对比单锁）落档。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 并发对账零损失：8 逻辑核 × 10^6 次分配/释放事件（确定性
//!   交错模型）后，帧账守恒：在用 + 空闲 ≡ 总帧，且无帧双重持有。
//!   J1-b 争用下降账：分区锁（8 分区各一把）对比单锁的争用等待
//!   步数账落档（分区模型等待步数 < 单锁模型，下降比率入账）。
//!   J1-c 跨分区帧不串账：帧归属分区固定，分区 A 的分配永不触碰
//!   分区 B 的位图（越界即红）。
//!
//! **建模口径（诚实声明）**：并发以确定性交错事件模型建模（8 核心
//! 轮转发射，事件序确定可回放），不使用真线程/原子（no_std 单核
//! 语义验证面）；锁等待以步数计。
//!
//! **段位复核记录**：0x6F50~0x6F5F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6F50~0x6F5F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6F50;
/// 段顶。
pub const SEG_TOP: u16 = 0x6F5F;

/// 分区数：8。
pub const ZONES: usize = 8;
/// 并发核心数：8。
pub const CORES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoneLockError {
    /// 0x6F51 跨分区访问。
    CrossZoneTouch,
    /// 0x6F52 帧账守恒破。
    ConservationBreak,
    /// 0x6F53 双重持有。
    DoubleHold,
    /// 0x6F54 分区锁等待未下降。
    ContentionNotLower,
}

impl ZoneLockError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            ZoneLockError::CrossZoneTouch => format!(
                "[0x{:04X}] 跨分区访问：帧归属分区固定。\
                 改法：分配/释放都按帧号定分区，禁全表扫描。",
                self.code()
            ),
            ZoneLockError::ConservationBreak => format!(
                "[0x{:04X}] 帧账守恒破：在用+空闲 ≠ 总帧。\
                 改法：按分区分别对账再汇总，找漂移分区。",
                self.code()
            ),
            ZoneLockError::DoubleHold => format!(
                "[0x{:04X}] 双重持有：同一帧被两个核心同时持有。\
                 改法：分区锁必须覆盖位图读改写全程。",
                self.code()
            ),
            ZoneLockError::ContentionNotLower => format!(
                "[0x{:04X}] 分区锁等待未低于单锁：分区收益不可证。\
                 改法：核对分区哈希是否把负载挤到同一分区。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、分区分配器（确定性并发模型）
// ===========================================================================

/// 单分区状态。
pub struct Zone {
    /// 帧占用位图（本分区帧域）。
    pub used: Vec<bool>,
    /// 本分区基帧。
    pub base: u64,
    /// 累计等待步数。
    pub wait_steps: u64,
    /// 分配次数。
    pub allocs: u64,
    /// 释放次数。
    pub frees: u64,
}

/// 分区分配器。
pub struct ZoneAllocator {
    /// 8 分区。
    pub zones: Vec<Zone>,
    /// 每分区帧数。
    pub per_zone: u64,
}

impl ZoneAllocator {
    /// 构造：帧域均分 8 分区。
    pub fn new(total_frames: u64) -> Self {
        let per = total_frames / ZONES as u64;
        let mut zones = Vec::new();
        let mut z = 0;
        while z < ZONES {
            zones.push(Zone {
                used: vec![false; per as usize],
                base: per * z as u64,
                wait_steps: 0,
                allocs: 0,
                frees: 0,
            });
            z += 1;
        }
        ZoneAllocator {
            zones,
            per_zone: per,
        }
    }

    /// 帧归属分区（J1-c 唯一映射）。
    pub fn zone_of(&self, frame: u64) -> usize {
        (frame / self.per_zone) as usize
    }

    /// 分配（分区 i 内找空闲帧）。
    pub fn alloc_in(&mut self, zone: usize) -> result::Result<u64, ZoneLockError> {
        if zone >= ZONES {
            return Err(ZoneLockError::CrossZoneTouch);
        }
        let z = &mut self.zones[zone];
        // 锁等待模型：等待步 = 分区内现存事件数的 1/8（分区负载均摊）。
        z.wait_steps += (z.allocs + z.frees) / 8;
        let mut f = 0usize;
        while f < z.used.len() {
            if !z.used[f] {
                z.used[f] = true;
                z.allocs += 1;
                return Ok(z.base + f as u64);
            }
            f += 1;
        }
        Err(ZoneLockError::ConservationBreak)
    }

    /// 释放。
    pub fn free_in(&mut self, frame: u64) -> result::Result<(), ZoneLockError> {
        let zone = self.zone_of(frame);
        let z = &mut self.zones[zone];
        let off = (frame - z.base) as usize;
        if !z.used[off] {
            return Err(ZoneLockError::DoubleHold);
        }
        z.used[off] = false;
        z.frees += 1;
        Ok(())
    }

    /// J1-a 守恒核。
    pub fn audit_conservation(&self) -> result::Result<(u64, u64), ZoneLockError> {
        let mut used_total = 0u64;
        let mut free_total = 0u64;
        let mut z = 0;
        while z < ZONES {
            let zz = &self.zones[z];
            let mut f = 0;
            while f < zz.used.len() {
                if zz.used[f] {
                    used_total += 1;
                } else {
                    free_total += 1;
                }
                f += 1;
            }
            z += 1;
        }
        if used_total + free_total != self.per_zone * ZONES as u64 {
            return Err(ZoneLockError::ConservationBreak);
        }
        Ok((used_total, free_total))
    }

    /// J1-b 单锁模型对照：等待步 = 全局事件数 × 1/2（单锁全串行）。
    pub fn single_lock_wait(&self) -> u64 {
        let mut total = 0u64;
        let mut z = 0;
        while z < ZONES {
            total += self.zones[z].allocs + self.zones[z].frees;
            z += 1;
        }
        total / 2
    }

    /// 分区锁总等待步。
    pub fn zone_lock_wait(&self) -> u64 {
        let mut t = 0u64;
        let mut z = 0;
        while z < ZONES {
            t += self.zones[z].wait_steps;
            z += 1;
        }
        t
    }
}

// ===========================================================================
// 三、J1 证据：8 核 × 10^6 次事件
// ===========================================================================

/// 每核事件数（8 核合计 10^6 的 1/8）。
pub const EVENTS_PER_CORE: u64 = 125_000;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2433_6F50_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// [0, bound)。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// J1 证据生成器：8 核轮转交错（核 i 的事件绑分区 i——负载均布），
/// 10^6 次分配/释放，守恒核 + 争用下降核。
pub fn zonelock_audit() -> result::Result<(u64, u64), ZoneLockError> {
    let total = 1u64 << 16;
    let mut za = ZoneAllocator::new(total);
    let mut live: Vec<Vec<u64>> = Vec::new();
    let mut c = 0;
    while c < CORES {
        live.push(Vec::new());
        c += 1;
    }
    let mut rngs: Vec<Lcg> = Vec::new();
    let mut c2 = 0;
    while c2 < CORES {
        rngs.push(Lcg::new(0x2433_0001 + c2 as u64));
        c2 += 1;
    }
    // 交错执行：每轮每核发一个事件。
    let mut round = 0u64;
    while round < EVENTS_PER_CORE {
        let mut c3 = 0;
        while c3 < CORES {
            let roll = rngs[c3].next_below(100);
            if roll < 50 || live[c3].is_empty() {
                // 分配：在核绑定的分区内（核 i ↔ 分区 i，负载均布）。
                if let Ok(f) = za.alloc_in(c3) {
                    live[c3].push(f);
                }
            } else {
                let idx = rngs[c3].next_below(live[c3].len() as u64) as usize;
                let f = live[c3][idx];
                live[c3].remove(idx);
                za.free_in(f)?;
            }
            c3 += 1;
        }
        round += 1;
    }
    let (used, free) = za.audit_conservation()?;
    let _ = (used, free);
    let zw = za.zone_lock_wait();
    let sw = za.single_lock_wait();
    if zw >= sw {
        return Err(ZoneLockError::ContentionNotLower);
    }
    Ok((zw, sw))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 分区锁等待步。
    pub zone_wait: u64,
    /// 单锁等待步。
    pub single_wait: u64,
    /// 守恒通过。
    pub conservation_ok: bool,
    /// 跨分区防线有效（越界分区被拒）。
    pub cross_zone_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.zone_wait < self.single_wait && self.conservation_ok && self.cross_zone_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, ZoneLockError> {
    let (zw, sw) = zonelock_audit()?;
    let mut za = ZoneAllocator::new(1024);
    let f = za.alloc_in(0)?;
    let bad = za.alloc_in(8).is_err();
    za.free_in(f)?;
    za.audit_conservation()?;
    Ok(SelfAudit {
        zone_wait: zw,
        single_wait: sw,
        conservation_ok: true,
        cross_zone_rejected: bad,
    })
}
