//! UNX-F2438 · 内存区间离线登记语义 · 诊断段 0x6FB0（独占 0x6FB0~0x6FBF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2438）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2438-J1 区间离线标记后分配零命中 10/10 次断言，在线恢复后
//! 阶账回归基线。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 离线零命中：区间标记 offline 后，10/10 次分配尝试对该
//!   区间零命中（命中=交付了离线帧，红）；分配扫描显式跳过。
//!   J1-b 在线恢复回归：区间恢复 online 后，阶账分布与离线前
//!   基线逐阶相等（恢复路径把区间块按原阶还回）。
//!   J1-c 离线留痕：离线动作账（区间、帧数、事件号）+ 在线动作账，
//!   两账对称可对（离线帧数 ≡ 在线归还帧数）。
//!
//! **段位复核记录**：0x6FB0~0x6FBF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6FB0~0x6FBF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6FB0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6FBF;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OfflineError {
    /// 0x6FB1 离线帧被分配（命中）。
    OfflineHit,
    /// 0x6FB2 在线恢复后阶账未回归基线。
    BaselineDrift,
    /// 0x6FB3 离线/在线账不对称。
    AsymmetricLedger,
    /// 0x6FB4 区间重叠。
    RangeOverlap,
    /// 0x6FB5 区间未对齐（整块离线要求）。
    MisalignedRange,
}

impl OfflineError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            OfflineError::OfflineHit => format!(
                "[0x{:04X}] 离线帧被分配：分配扫描未跳过离线区。\
                 改法：分配路径加离线守卫。",
                self.code()
            ),
            OfflineError::BaselineDrift => format!(
                "[0x{:04X}] 在线恢复后阶账未回归基线。\
                 改法：恢复路径按离线时记录的原阶还块。",
                self.code()
            ),
            OfflineError::AsymmetricLedger => format!(
                "[0x{:04X}] 离线/在线帧数不对称。\
                 改法：恢复必须覆盖离线区间全量。",
                self.code()
            ),
            OfflineError::RangeOverlap => format!(
                "[0x{:04X}] 离线区间与既有区间重叠。\
                 改法：离线前查重叠。",
                self.code()
            ),
            OfflineError::MisalignedRange => format!(
                "[0x{:04X}] 离线区间未按整块对齐。\
                 改法：离线粒度 = 块（order 对齐）。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、离线登记器
// ===========================================================================

/// 一条离线记录。
#[derive(Clone, Copy, Debug)]
pub struct OfflineRecord {
    /// 区间 (lo, hi)。
    pub lo: u64,
    pub hi: u64,
    /// 离线时各阶块快照（按整块粒度）。
    pub order_snapshot: [u64; 11],
    /// 事件号。
    pub seq: u64,
}

/// 离线登记器（挂简化 free_area）。
pub struct OfflineRegistry {
    /// 总帧数。
    pub total_frames: u64,
    /// 各阶空闲块首列表。
    pub areas: Vec<Vec<u64>>,
    /// 离线帧位图。
    pub offline_map: Vec<bool>,
    /// 离线账。
    pub offline_ledger: Vec<OfflineRecord>,
    /// 在线归还帧数（对称核用）。
    pub online_frames: u64,
    seq: u64,
}

impl OfflineRegistry {
    /// 构造：域按 order-4 块铺满。
    pub fn new(total_frames: u64) -> Self {
        let mut areas = Vec::new();
        let mut o = 0;
        while o <= 10 {
            areas.push(Vec::new());
            o += 1;
        }
        OfflineRegistry {
            total_frames,
            areas,
            offline_map: vec![false; total_frames as usize],
            offline_ledger: Vec::new(),
            online_frames: 0,
            seq: 0,
        }
    }

    /// 基线：order-4 块铺满。
    pub fn seed_baseline(&mut self) {
        let mut f = 0u64;
        while f < self.total_frames {
            self.areas[4].push(f);
            f += 16;
        }
    }

    fn is_offline(&self, frame: u64) -> bool {
        self.offline_map[frame as usize]
    }

    /// 分配（order-4）：跳过离线帧（J1-a）。
    pub fn alloc(&mut self) -> result::Result<u64, OfflineError> {
        let mut i = 0usize;
        loop {
            if i >= self.areas[4].len() {
                return Err(OfflineError::OfflineHit);
            }
            let h = self.areas[4][i];
            // 块内 16 帧逐帧查离线。
            let mut f = h;
            let mut dirty = false;
            while f < h + 16 {
                if self.offline_map[f as usize] {
                    dirty = true;
                    break;
                }
                f += 1;
            }
            if dirty {
                i += 1;
                continue; // 跳过离线块。
            }
            self.areas[4].remove(i);
            return Ok(h);
        }
    }

    /// 离线登记：整块粒度（16 帧对齐）+ 快照。
    pub fn take_offline(&mut self, lo: u64, blocks: u64) -> result::Result<(), OfflineError> {
        if lo % 16 != 0 || lo + blocks * 16 > self.total_frames {
            return Err(OfflineError::MisalignedRange);
        }
        self.seq += 1;
        // 快照：区间覆盖的 order-4 块出册。
        let mut snap = [0u64; 11];
        let mut b = 0;
        while b < blocks {
            let head = lo + b * 16;
            let mut i = 0;
            while i < self.areas[4].len() {
                if self.areas[4][i] == head {
                    self.areas[4].remove(i);
                    snap[4] += 1;
                    break;
                }
                i += 1;
            }
            b += 1;
        }
        let mut f = lo;
        while f < lo + blocks * 16 {
            self.offline_map[f as usize] = true;
            f += 1;
        }
        self.offline_ledger.push(OfflineRecord {
            lo,
            hi: lo + blocks * 16,
            order_snapshot: snap,
            seq: self.seq,
        });
        Ok(())
    }

    /// 在线恢复：按快照原阶还块（J1-b/J1-c）。
    pub fn bring_online(&mut self, lo: u64) -> result::Result<(), OfflineError> {
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.offline_ledger.len() {
            if self.offline_ledger[i].lo == lo {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match idx {
            Some(v) => v,
            None => return Err(OfflineError::AsymmetricLedger),
        };
        let rec = self.offline_ledger[idx];
        // 快照还块。
        let blocks = rec.order_snapshot[4];
        let mut b = 0;
        while b < blocks {
            let head = rec.lo + b * 16;
            self.areas[4].push(head);
            b += 1;
        }
        self.online_frames += blocks * 16;
        // 位图清离线。
        let mut f = rec.lo;
        while f < rec.hi {
            self.offline_map[f as usize] = false;
            f += 1;
        }
        self.offline_ledger.remove(idx);
        Ok(())
    }

    /// J1-b 阶账回归核：当前阶账与基线（全 order-4 满铺）相等。
    pub fn audit_baseline(&self) -> result::Result<(), OfflineError> {
        let n = self.areas[4].len() as u64;
        if n != self.total_frames / 16 {
            return Err(OfflineError::BaselineDrift);
        }
        Ok(())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// J1 证据生成器：10/10 离线零命中 + 恢复回归基线。
pub fn offline_audit() -> result::Result<usize, OfflineError> {
    let mut rg = OfflineRegistry::new(1 << 12);
    rg.seed_baseline();
    // 离线 2 块。
    rg.take_offline(64, 2)?;
    // 10 次分配零命中离线帧。
    let mut zero_hit = 0usize;
    let mut n = 0;
    while n < 10 {
        let h = rg.alloc()?;
        // 命中核：块区间与离线区不相交。
        let mut f = h;
        while f < h + 16 {
            if rg.is_offline_public(f) {
                return Err(OfflineError::OfflineHit);
            }
            f += 1;
        }
        zero_hit += 1;
        n += 1;
    }
    // 恢复在线 + 基线回归。
    rg.bring_online(64)?;
    rg.audit_baseline()?;
    // 对称核：离线账空（已全恢复）且在线帧数 = 32。
    let sym = rg.offline_ledger.is_empty() && rg.online_frames == 32;
    if !sym {
        return Err(OfflineError::AsymmetricLedger);
    }
    Ok(zero_hit)
}

impl OfflineRegistry {
    /// 位图只读访问（证据核用）。
    pub fn is_offline_public(&self, frame: u64) -> bool {
        self.offline_map[frame as usize]
    }
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 离线零命中断言数（应 10/10）。
    pub zero_hits: usize,
    /// 基线回归通过。
    pub baseline_ok: bool,
    /// 离线/在线对称。
    pub symmetric_ok: bool,
    /// 非对齐区间被拒。
    pub misalign_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.zero_hits == 10 && self.baseline_ok && self.symmetric_ok && self.misalign_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, OfflineError> {
    let zero_hits = offline_audit()?;
    let mut rg = OfflineRegistry::new(1024);
    rg.seed_baseline();
    let mis = rg.take_offline(15, 1).is_err();
    Ok(SelfAudit {
        zero_hits,
        baseline_ok: true,
        symmetric_ok: true,
        misalign_rejected: mis,
    })
}
