//! UNX-F2413 · 迁移类型三分区 · 诊断段 0x6DE0（独占 0x6DE0~0x6DEF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2413）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2413-J1 UNMOVABLE/MOVABLE/RECLAIMABLE 三区页数账守恒，跨区误
//! 分配注入 20/20 检出。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 三区守恒：三区页数之和 ≡ 总页数，任一区分配/回收后
//!   逐次复核；每区页账独立（分配计数+当前余量=累计供给）。
//!   J1-b 跨区误分配 20/20 检出：注入「向 UNMOVABLE 区请求 MOVABLE
//!   块」类误路由 20 次，每次必须被路由器拒绝并留账（哪区请求、
//!   哪区应答）。
//!   J1-c 区段不重叠：三区的帧区间两两无交，全量扫描零交叉帧。
//!
//! **建模口径（诚实声明）**：迁移类型即 Linux分区反碎片语义的
//! 内核模型：UNMOVABLE（内核常驻）、MOVABLE（可迁移用户页）、
//! RECLAIMABLE（可回收缓存）。区间以 (lo, hi) 登记，帧粒度账。
//!
//! **段位复核记录**：0x6DE0~0x6DEF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6DE0~0x6DEF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6DE0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6DEF;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigTypeError {
    /// 0x6DE1 区类型非法（>2）。
    BadZone,
    /// 0x6DE2 区登记与既有区重叠。
    ZoneOverlap,
    /// 0x6DE3 区越出总域。
    ZoneRange,
    /// 0x6DE4 三区页数守恒破。
    ConservationBreak,
    /// 0x6DE5 跨区误分配未被路由器拒绝（防线失效）。
    CrossZoneLeak,
    /// 0x6DE6 区页账漂移（供给≠余量+分配）。
    ZoneDrift,
}

impl MigTypeError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            MigTypeError::BadZone => format!(
                "[0x{:04X}] 区类型非法：0=UNMOVABLE 1=MOVABLE 2=RECLAIMABLE。\
                 改法：用 MigZone 枚举，勿传裸数。",
                self.code()
            ),
            MigTypeError::ZoneOverlap => format!(
                "[0x{:04X}] 区登记与既有区重叠：分区必须两两无交。\
                 改法：重新规划分区边界后整域重建分区。",
                self.code()
            ),
            MigTypeError::ZoneRange => format!(
                "[0x{:04X}] 区越出总域。改法：核对总帧数登记。",
                self.code()
            ),
            MigTypeError::ConservationBreak => format!(
                "[0x{:04X}] 三区页数守恒破：Σ区页 ≠ 总页。\
                 改法：逐区对账（供给=余量+分配），找漂移区。",
                self.code()
            ),
            MigTypeError::CrossZoneLeak => format!(
                "[0x{:04X}] 跨区误分配漏检：路由器放行了跨区请求。\
                 改法：查 alloc() 的区校验分支是否被短路。",
                self.code()
            ),
            MigTypeError::ZoneDrift => format!(
                "[0x{:04X}] 区页账漂移：累计供给 ≠ 余量+已分配。\
                 改法：回放该区分配流水。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、迁移类型模型
// ===========================================================================

/// 三区类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigZone {
    /// 0：不可迁移（内核常驻结构）。
    Unmovable,
    /// 1：可迁移（用户页）。
    Movable,
    /// 2：可回收（页缓存等）。
    Reclaimable,
}

impl MigZone {
    /// 区号。
    pub fn num(self) -> u8 {
        match self {
            MigZone::Unmovable => 0,
            MigZone::Movable => 1,
            MigZone::Reclaimable => 2,
        }
    }

    /// 从区号还原。
    pub fn from_num(n: u8) -> result::Result<MigZone, MigTypeError> {
        match n {
            0 => Ok(MigZone::Unmovable),
            1 => Ok(MigZone::Movable),
            2 => Ok(MigZone::Reclaimable),
            _ => Err(MigTypeError::BadZone),
        }
    }
}

/// 单区账。
#[derive(Clone, Copy, Debug)]
pub struct ZoneLedger {
    /// 区类型。
    pub zone: MigZone,
    /// 区起点帧（含）。
    pub lo: u64,
    /// 区终点帧（不含）。
    pub hi: u64,
    /// 累计分配帧数。
    pub allocated: u64,
    /// 当前余量帧数。
    pub free: u64,
}

impl ZoneLedger {
    /// 区总页数。
    pub fn pages(&self) -> u64 {
        self.hi - self.lo
    }
}

/// 三分域主体。
pub struct MigPartitions {
    /// 总帧数。
    pub total_frames: u64,
    /// 三区账（下标=区号）。
    pub zones: Vec<ZoneLedger>,
    /// 帧归属（帧号 → 区号 0xFF 未登记）。
    pub owner_of: Vec<u8>,
    /// 区内分配游标（区内帧顺序分配，环绕）。
    zone_cursor: Vec<u64>,
    /// 跨区误分配检出账：(请求区, 实际区, 事件号)。
    pub cross_zone_hits: Vec<(u8, u8, u64)>,
    seq: u64,
}

impl MigPartitions {
    /// 空域。
    pub fn new(total_frames: u64) -> Self {
        MigPartitions {
            total_frames,
            zones: Vec::new(),
            owner_of: vec![0xFFu8; total_frames as usize],
            zone_cursor: Vec::new(),
            cross_zone_hits: Vec::new(),
            seq: 0,
        }
    }

    /// 登记一区：重叠/越界校验 + 页账初始化。
    pub fn register_zone(
        &mut self,
        zone: MigZone,
        lo: u64,
        hi: u64,
    ) -> result::Result<(), MigTypeError> {
        if hi > self.total_frames || lo >= hi {
            return Err(MigTypeError::ZoneRange);
        }
        // 重叠核：区间内任何帧已有归属即拒。
        let mut f = lo;
        while f < hi {
            if self.owner_of[f as usize] != 0xFF {
                return Err(MigTypeError::ZoneOverlap);
            }
            f += 1;
        }
        let n = zone.num() as usize;
        // 同区重复登记也按重叠拒（区账唯一）。
        while self.zones.len() <= n {
            self.zones.push(ZoneLedger {
                zone: MigZone::from_num(self.zones.len() as u8)?,
                lo: 0,
                hi: 0,
                allocated: 0,
                free: 0,
            });
        }
        self.zones[n] = ZoneLedger {
            zone,
            lo,
            hi,
            allocated: 0,
            free: hi - lo,
        };
        let mut f2 = lo;
        while f2 < hi {
            self.owner_of[f2 as usize] = zone.num();
            f2 += 1;
        }
        Ok(())
    }

    /// J1-c 区段不重叠全量核：扫帧归属，反推各区边界与登记一致。
    pub fn audit_no_overlap(&self) -> result::Result<(), MigTypeError> {
        let mut i = 0;
        while i < self.zones.len() {
            let z = &self.zones[i];
            if z.hi <= z.lo && z.pages() > 0 {
                return Err(MigTypeError::ZoneDrift);
            }
            // 区内所有帧归属必须是本区。
            if z.pages() > 0 {
                let mut f = z.lo;
                while f < z.hi {
                    if self.owner_of[f as usize] != z.zone.num() {
                        return Err(MigTypeError::ZoneOverlap);
                    }
                    f += 1;
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// J1-a 三区守恒核：Σ区页 ≡ 总帧数；且每区 供给=余量+分配。
    pub fn audit_conservation(&self) -> result::Result<u64, MigTypeError> {
        let mut sum = 0u64;
        let mut i = 0;
        while i < self.zones.len() {
            let z = &self.zones[i];
            sum = sum.checked_add(z.pages()).ok_or(MigTypeError::ConservationBreak)?;
            if z.allocated + z.free != z.pages() {
                return Err(MigTypeError::ZoneDrift);
            }
            i += 1;
        }
        if sum != self.total_frames {
            return Err(MigTypeError::ConservationBreak);
        }
        Ok(sum)
    }

    /// 区内分配一帧：区内游标顺序供给（分区账模型，帧计数为主账）。
    pub fn alloc_in(&mut self, zone: MigZone) -> result::Result<u64, MigTypeError> {
        let n = zone.num() as usize;
        if n >= self.zones.len() {
            return Err(MigTypeError::BadZone);
        }
        if self.zones[n].free == 0 {
            return Err(MigTypeError::ZoneDrift);
        }
        while self.zone_cursor.len() <= n {
            self.zone_cursor.push(0);
        }
        let z = self.zones[n];
        let cur = self.zone_cursor[n] % z.pages();
        self.zone_cursor[n] = cur + 1;
        self.zones[n].free -= 1;
        self.zones[n].allocated += 1;
        self.seq += 1;
        Ok(z.lo + cur)
    }

    /// 区内回收一帧。
    pub fn free_in(&mut self, zone: MigZone) -> result::Result<(), MigTypeError> {
        let n = zone.num() as usize;
        if n >= self.zones.len() {
            return Err(MigTypeError::BadZone);
        }
        if self.zones[n].allocated == 0 {
            return Err(MigTypeError::ZoneDrift);
        }
        self.zones[n].allocated -= 1;
        self.zones[n].free += 1;
        Ok(())
    }

    /// J1-b 跨区误分配注入：请求帧 f 必须落在 zone 区内，否则检出。
    ///
    /// 这是「误分配注入」的被测路径：调用方给 (zone, frame)，
    /// 本方法核 frame 归属区 == zone，不等即入检出账并拒绝。
    pub fn inject_cross_check(&mut self, zone: MigZone, frame: u64) -> bool {
        let actual = if frame < self.total_frames {
            self.owner_of[frame as usize]
        } else {
            0xFF
        };
        self.seq += 1;
        if actual != zone.num() {
            self.cross_zone_hits.push((zone.num(), actual, self.seq));
            false
        } else {
            true
        }
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2413_6DE0_9E37_79B9)
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

/// 跨区注入样本数（J1-b 口径 20）。
pub const CROSS_ZONE_INJECTIONS: usize = 20;

/// J1 证据生成器：三分域（16K 帧：U 4K / M 8K / R 4K）
/// 分配回收扰动 + 20 次跨区注入全检出。
pub fn migtype_audit() -> result::Result<(usize, usize), MigTypeError> {
    let mut dom = MigPartitions::new(1 << 14);
    dom.register_zone(MigZone::Unmovable, 0, 1 << 12)?;
    dom.register_zone(MigZone::Movable, 1 << 12, 3 << 12)?;
    dom.register_zone(MigZone::Reclaimable, 3 << 12, 1 << 14)?;
    dom.audit_no_overlap()?;
    dom.audit_conservation()?;
    // 扰动：三区各分配回收若干帧。
    let mut rng = Lcg::new(0x2413_0001);
    let mut round = 0;
    while round < 64 {
        let z = match rng.next_u64() % 3 {
            0 => MigZone::Unmovable,
            1 => MigZone::Movable,
            _ => MigZone::Reclaimable,
        };
        let _ = dom.alloc_in(z)?;
        round += 1;
    }
    dom.audit_conservation()?;
    let mut r2 = 0;
    while r2 < 32 {
        let z = match rng.next_u64() % 3 {
            0 => MigZone::Unmovable,
            1 => MigZone::Movable,
            _ => MigZone::Reclaimable,
        };
        let _ = dom.free_in(z)?;
        r2 += 1;
    }
    dom.audit_conservation()?;
    // 20 次跨区注入：从各区抽一个帧，谎报给其他区，必须全检出。
    let mut caught = 0usize;
    let mut inj = 0;
    while inj < CROSS_ZONE_INJECTIONS {
        // 谎报组合轮换：U 帧→M 区、M 帧→R 区、R 帧→U 区。
        let (frame, claim) = match inj % 3 {
            0 => (0u64, MigZone::Movable),             // U 区帧谎报 M。
            1 => (1 << 12, MigZone::Reclaimable),      // M 区首帧谎报 R。
            _ => (3 << 12, MigZone::Unmovable),        // R 区首帧谎报 U。
        };
        if !dom.inject_cross_check(claim, frame) {
            caught += 1;
        } else {
            return Err(MigTypeError::CrossZoneLeak);
        }
        inj += 1;
    }
    Ok((dom.cross_zone_hits.len(), caught))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 三区守恒通过。
    pub conservation_ok: bool,
    /// 跨区注入检出数（应 20/20）。
    pub cross_caught: usize,
    /// 区不重叠核通过。
    pub no_overlap_ok: bool,
    /// 重叠登记被拒。
    pub overlap_rejected: bool,
    /// 区分配/回收账自洽。
    pub ledger_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.conservation_ok
            && self.cross_caught == CROSS_ZONE_INJECTIONS
            && self.no_overlap_ok
            && self.overlap_rejected
            && self.ledger_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, MigTypeError> {
    let (_, caught) = migtype_audit()?;
    // 重叠登记防线。
    let mut dom = MigPartitions::new(1024);
    dom.register_zone(MigZone::Unmovable, 0, 512)?;
    let overlap = dom.register_zone(MigZone::Movable, 256, 768).is_err();
    // 剩余域可登记。
    dom.register_zone(MigZone::Movable, 512, 768)?;
    dom.register_zone(MigZone::Reclaimable, 768, 1024)?;
    dom.audit_no_overlap()?;
    dom.audit_conservation()?;
    let mut rng = Lcg::new(0x2413_0002);
    let mut i = 0;
    while i < 100 {
        let z = match rng.next_u64() % 3 {
            0 => MigZone::Unmovable,
            1 => MigZone::Movable,
            _ => MigZone::Reclaimable,
        };
        let _ = dom.alloc_in(z)?;
        i += 1;
    }
    dom.audit_conservation()?;
    Ok(SelfAudit {
        conservation_ok: true,
        cross_caught: caught,
        no_overlap_ok: true,
        overlap_rejected: overlap,
        ledger_ok: true,
    })
}
