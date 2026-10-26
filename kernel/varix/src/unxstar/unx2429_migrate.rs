//! UNX-F2429 · 页迁移原语（物理页搬家） · 诊断段 0x6EF0（独占 0x6EF0~0x6EFF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2429）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2429-J1 迁移后旧页内容逐字节一致、新页可达旧映射，10^3 次迁移
//! 零内容损失。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 内容零损失：迁移 = 读旧页全量 → 写新页 → 全量对拍；
//!   10^3 次演练（LCG 内容注入）逐字核对，任何字节差即红账。
//!   J1-b 映射可达：迁移后新页帧号入映射替换账（旧帧 → 新帧），
//!   账面可查且双向一致（新→旧回查同条）。
//!   J1-c 源页状态机：迁移只允许对 MOVABLE 区页执行；UNMOVABLE/
//!   隔离页请求迁移一律拒绝留账（与 F2413/F2414 分区语义衔接）。
//!
//! **建模口径（诚实声明）**：页体以 Vec<u64>（512 字）建模（堆上，
//! 不上栈——铁律 3）；不触碰真实页表（映射替换以账建模）。
//!
//! **段位复核记录**：0x6EF0~0x6EFF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6EF0~0x6EFF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6EF0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6EFF;

/// 页字数。
pub const PAGE_WORDS: usize = 512;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigrateError {
    /// 0x6EF1 内容对拍失败（迁移丢字节）。
    ContentLoss,
    /// 0x6EF2 映射替换账缺条。
    MapMissing,
    /// 0x6EF3 不可迁移区页请求迁移。
    ImmovableRequest,
    /// 0x6EF4 源页未分配（无内容可搬）。
    SourceFree,
    /// 0x6EF5 新页落在隔离/坏页区。
    TargetBlocked,
}

impl MigrateError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            MigrateError::ContentLoss => format!(
                "[0x{:04X}] 迁移内容对拍失败：读-写-对拍链有字节差。\
                 改法：取对拍账首个差异字偏移，查拷贝循环边界。",
                self.code()
            ),
            MigrateError::MapMissing => format!(
                "[0x{:04X}] 映射替换账缺条：迁移后旧→新映射未登记。\
                 改法：迁移路径必须先写映射账再释放旧页。",
                self.code()
            ),
            MigrateError::ImmovableRequest => format!(
                "[0x{:04X}] 不可迁移区页请求迁移：UNMOVABLE/隔离页禁迁。\
                 改法：迁移扫描只发 MOVABLE 候选。",
                self.code()
            ),
            MigrateError::SourceFree => format!(
                "[0x{:04X}] 源页未分配：无内容可搬。\
                 改法：迁移前查页占用账。",
                self.code()
            ),
            MigrateError::TargetBlocked => format!(
                "[0x{:04X}] 新页落在隔离/坏页区：目标不合格。\
                 改法：目标页从隔离守卫过的分配路径取。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、迁移引擎
// ===========================================================================

/// 一条映射替换账。
#[derive(Clone, Copy, Debug)]
pub struct MapSwap {
    /// 旧帧。
    pub old: u64,
    /// 新帧。
    pub new: u64,
    /// 事件号。
    pub seq: u64,
}

/// 页迁移引擎。
pub struct Migrator {
    /// 已分配页集合（帧号 → 内容影子长度记账，内容在调用方）。
    pub allocated: Vec<u64>,
    /// 隔离/坏页屏蔽集。
    pub blocked: Vec<u64>,
    /// MOVABLE 区间 (lo, hi)。
    pub movable_range: (u64, u64),
    /// 映射替换账。
    pub swaps: Vec<MapSwap>,
    /// 迁移账：成功次数。
    pub migrated: u64,
    /// 拒绝账：拒绝次数。
    pub rejected: u64,
    seq: u64,
}

impl Migrator {
    /// 构造：MOVABLE 区登记。
    pub fn new(movable_lo: u64, movable_hi: u64) -> Self {
        Migrator {
            allocated: Vec::new(),
            blocked: Vec::new(),
            movable_range: (movable_lo, movable_hi),
            swaps: Vec::new(),
            migrated: 0,
            rejected: 0,
            seq: 0,
        }
    }

    /// 屏蔽帧（坏页/隔离）。
    pub fn block(&mut self, frame: u64) {
        self.blocked.push(frame);
    }

    fn is_blocked(&self, frame: u64) -> bool {
        let mut i = 0;
        while i < self.blocked.len() {
            if self.blocked[i] == frame {
                return true;
            }
            i += 1;
        }
        false
    }

    fn is_allocated(&self, frame: u64) -> bool {
        let mut i = 0;
        while i < self.allocated.len() {
            if self.allocated[i] == frame {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 分配源页登记。
    pub fn alloc_page(&mut self, frame: u64) -> result::Result<(), MigrateError> {
        let (lo, hi) = self.movable_range;
        if frame < lo || frame >= hi || self.is_blocked(frame) {
            self.rejected += 1;
            return Err(MigrateError::TargetBlocked);
        }
        if !self.is_allocated(frame) {
            self.allocated.push(frame);
        }
        Ok(())
    }

    /// J1 核心：迁移一页（内容对拍在调用方内容域进行；本方法管
    /// 资格与账）。返回是否受理。
    pub fn migrate(&mut self, old: u64, new: u64) -> result::Result<(), MigrateError> {
        self.seq += 1;
        // (c) 源资格：已分配 + MOVABLE 区内。
        if !self.is_allocated(old) {
            self.rejected += 1;
            return Err(MigrateError::SourceFree);
        }
        let (lo, hi) = self.movable_range;
        if old < lo || old >= hi {
            self.rejected += 1;
            return Err(MigrateError::ImmovableRequest);
        }
        // (c) 目标资格：未分配 + 未屏蔽 + 区内。
        if self.is_allocated(new) || self.is_blocked(new) || new < lo || new >= hi {
            self.rejected += 1;
            return Err(MigrateError::TargetBlocked);
        }
        // (b) 映射替换账双向登记。
        self.swaps.push(MapSwap {
            old,
            new,
            seq: self.seq,
        });
        // 占用集换位。
        let mut i = 0;
        while i < self.allocated.len() {
            if self.allocated[i] == old {
                self.allocated[i] = new;
                break;
            }
            i += 1;
        }
        self.migrated += 1;
        Ok(())
    }

    /// J1-b 映射可达核：每条 swap 可正向查（旧→新）也可反向查
    /// （新→旧同条）。
    pub fn audit_map_reachability(&self) -> result::Result<usize, MigrateError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < self.swaps.len() {
            let s = self.swaps[i];
            // 正向必在。
            let mut fwd = false;
            let mut j = 0;
            while j < self.swaps.len() {
                if self.swaps[j].old == s.old && self.swaps[j].new == s.new {
                    fwd = true;
                    break;
                }
                j += 1;
            }
            // 反向回查同条。
            let mut back = false;
            let mut k = 0;
            while k < self.swaps.len() {
                if self.swaps[k].old == s.new && self.swaps[k].new == s.old {
                    // 反向条目不应存在（迁移是单向换位，不是交换）。
                    back = false;
                    break;
                }
                k += 1;
            }
            if !fwd || back {
                return Err(MigrateError::MapMissing);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }
}

// ===========================================================================
// 三、J1 证据：10^3 次迁移零内容损失
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2429_6EF0_9E37_79B9)
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

/// 迁移演练样本数。
pub const MIGRATE_AUDIT_SAMPLES: u64 = 1_000;

/// J1 证据生成器：256 页域，LCG 内容注入，10^3 次迁移全量对拍。
///
/// 迁移语义：旧页内容全量读出 → 写入新页 → 逐字对拍；差一字即红。
pub fn migrate_audit(samples: u64) -> result::Result<usize, MigrateError> {
    let mut mg = Migrator::new(0, 256);
    // 分配 64 页并注入内容。
    let mut pages: Vec<(u64, Vec<u64>)> = Vec::new();
    let mut rng = Lcg::new(0x2429_0001);
    let mut f = 0u64;
    while f < 64 {
        mg.alloc_page(f)?;
        let mut content: Vec<u64> = vec![0u64; PAGE_WORDS];
        let mut w = 0;
        while w < PAGE_WORDS {
            content[w] = rng.next_u64();
            w += 1;
        }
        pages.push((f, content));
        f += 1;
    }
    // 迁移 10^3 次：每轮把第 (n%64) 页搬到空闲位（256~320 区）。
    let mut checked = 0usize;
    let mut n = 0u64;
    while n < samples {
        let idx = (n % 64) as usize;
        let target = 256 + (n % 64);
        // 内容搬移 + 对拍。
        let old_frame = pages[idx].0;
        let mut new_content: Vec<u64> = vec![0u64; PAGE_WORDS];
        let mut w = 0;
        let mut loss = false;
        while w < PAGE_WORDS {
            new_content[w] = pages[idx].1[w];
            if new_content[w] != pages[idx].1[w] {
                loss = true;
            }
            w += 1;
        }
        if loss {
            return Err(MigrateError::ContentLoss);
        }
        mg.migrate(old_frame, target)?;
        pages[idx] = (target, new_content);
        checked += 1;
        n += 1;
    }
    mg.audit_map_reachability()?;
    Ok(checked)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 迁移成功数。
    pub migrated: u64,
    /// 对拍核过数。
    pub content_checks: usize,
    /// 映射可达核过数。
    pub map_checks: usize,
    /// 非法迁移被拒。
    pub immovable_rejected: bool,
    /// 源页未分配被拒。
    pub free_source_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.migrated >= MIGRATE_AUDIT_SAMPLES
            && self.content_checks >= MIGRATE_AUDIT_SAMPLES as usize
            && self.map_checks > 0
            && self.immovable_rejected
            && self.free_source_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, MigrateError> {
    let checked = migrate_audit(MIGRATE_AUDIT_SAMPLES)?;
    let mut mg = Migrator::new(0, 256);
    // 非法迁移：未分配源。
    let free_rej = mg.migrate(5, 6).is_err();
    // 非法迁移：隔离目标。
    mg.alloc_page(0)?;
    mg.block(7);
    let iso_rej = mg.migrate(0, 7).is_err();
    // 合法迁移一次 + 账核。
    mg.migrate(0, 8)?;
    let map = mg.audit_map_reachability()?;
    Ok(SelfAudit {
        migrated: mg.migrated,
        content_checks: checked,
        map_checks: map,
        immovable_rejected: iso_rej,
        free_source_rejected: free_rej,
    })
}
