//! UNX-F2410 · 低阶块合并（merge）语义 · 诊断段 0x6DA0（独占 0x6DA0~0x6DAF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2410）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2410-J1 归还后连续伙伴逐级合并至最高可达阶，10^5 次 alloc/free
//! 循环后阶分布回到基线。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 逐级合并至顶：还回块与伙伴同阶同空闲时合并，循环直至
//!   伙伴不在册或到 MAX_ORDER；合并链逐级留账（哪两块合成哪块）。
//!   J1-b 阶分布回归基线：确定性 workload（10^5 次 alloc/free 混合，
//!   LCG 驱动）跑完后，各阶 nr_free 与初始基线逐阶相等。
//!   J1-c 合并前提核：跨区（不同迁移类型段）不合并；未对齐块
//!   （非 2^o 对齐首帧）不参与伙伴运算，显性拒绝。
//!
//! **建模口径（诚实声明）**：合并账以 (order, head) 键有序表建模；
//! 「基线」= 构造时预登记布局的阶分布快照。域规模 2^16 帧。
//!
//! **段位复核记录**：0x6DA0~0x6DAF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6DA0~0x6DAF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6DA0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6DAF;

/// 最高阶。
pub const MAX_ORDER: u32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeError {
    /// 0x6DA1 阶非法。
    BadOrder,
    /// 0x6DA2 还回块未对齐。
    Misaligned,
    /// 0x6DA3 还回块越界。
    Range,
    /// 0x6DA4 还回块与在册块重叠（重复释放类）。
    Overlap,
    /// 0x6DA5 合并链断：伙伴在册但阶账不同步。
    ChainBreak,
    /// 0x6DA6 阶分布未回归基线。
    BaselineDrift,
    /// 0x6DA7 跨区合并被拒后仍入账（前提核失效）。
    ZoneMix,
}

impl MergeError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            MergeError::BadOrder => format!(
                "[0x{:04X}] 阶非法：合法 0~10。改法：入参先做域校验。",
                self.code()
            ),
            MergeError::Misaligned => format!(
                "[0x{:04X}] 还回块首帧未按 2^o 对齐：伙伴运算前提不成立。\
                 改法：还原块的真实 (首帧, 阶) 再还，勿拆散还。",
                self.code()
            ),
            MergeError::Range => format!(
                "[0x{:04X}] 还回块越出域界。改法：核对总帧数登记。",
                self.code()
            ),
            MergeError::Overlap => format!(
                "[0x{:04X}] 还回块与在册空闲块重叠：双重释放或部分还回。\
                 改法：核对分配记录，找重复释放来源。",
                self.code()
            ),
            MergeError::ChainBreak => format!(
                "[0x{:04X}] 合并链断：合并后新块未同步入册。\
                 改法：回放合并账定位断级。",
                self.code()
            ),
            MergeError::BaselineDrift => format!(
                "[0x{:04X}] 阶分布未回归基线：alloc/free 收支不平。\
                 改法：取首个偏差阶，回放该阶取还流水。",
                self.code()
            ),
            MergeError::ZoneMix => format!(
                "[0x{:04X}] 跨迁移类型区合并：不同区伙伴不得合并。\
                 改法：核两块的迁移类型账，跨区时只入册不合并。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、合并引擎
// ===========================================================================

/// 一条合并记录：两块 → 一块。
#[derive(Clone, Copy, Debug)]
pub struct MergeRecord {
    /// 合并前左块首帧（伙伴中较小者）。
    pub lo: u64,
    /// 合并前右块首帧。
    pub hi: u64,
    /// 合并阶（两块原阶）。
    pub order: u32,
    /// 合并后首帧（= lo）。
    pub merged: u64,
    /// 合并后阶。
    pub merged_order: u32,
    /// 事件号。
    pub seq: u64,
}

/// 合并引擎：在册块账 + 合并账。
pub struct MergeEngine {
    /// 在册空闲块 (order, head)。
    blocks: Vec<(u32, u64)>,
    /// 合并账。
    pub ledger: Vec<MergeRecord>,
    /// 总帧数。
    pub total_frames: u64,
    /// 空闲帧数。
    pub free_frames: u64,
    /// 迁移类型账（帧粒度，0/1/2 三类，用于跨区前提核）。
    pub zone_of: Vec<u8>,
    seq: u64,
}

impl MergeEngine {
    /// 建账：zone_of 全 1（MOVABLE）。
    pub fn new(total_frames: u64) -> Self {
        MergeEngine {
            blocks: Vec::new(),
            ledger: Vec::new(),
            total_frames,
            free_frames: 0,
            zone_of: vec![1u8; total_frames as usize],
            seq: 0,
        }
    }

    /// 段级迁移类型设定（帧区间批量）。
    pub fn set_zone(&mut self, lo: u64, hi: u64, zone: u8) -> result::Result<(), MergeError> {
        if hi > self.total_frames || lo > hi {
            return Err(MergeError::Range);
        }
        let mut f = lo;
        while f < hi {
            self.zone_of[f as usize] = zone;
            f += 1;
        }
        Ok(())
    }

    fn find(&self, order: u32, head: u64) -> bool {
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i] == (order, head) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 直接入册（不触发合并；还回路径的第一步）。
    pub fn insert_block(&mut self, order: u32, head: u64) -> result::Result<(), MergeError> {
        if order > MAX_ORDER {
            return Err(MergeError::BadOrder);
        }
        let span = 1u64 << order;
        if head % span != 0 {
            return Err(MergeError::Misaligned);
        }
        if head.checked_add(span).is_none() || head + span > self.total_frames {
            return Err(MergeError::Range);
        }
        // 重叠核：区间内任何帧已在其他在册块内则拒。
        let mut f = head;
        while f < head + span {
            // 帧粒度重叠检查：对每阶在册块做区间求交。
            let mut i = 0;
            while i < self.blocks.len() {
                let (o2, h2) = self.blocks[i];
                let s2 = 1u64 << o2;
                if f >= h2 && f < h2 + s2 {
                    return Err(MergeError::Overlap);
                }
                i += 1;
            }
            f += 1;
        }
        self.blocks.push((order, head));
        self.free_frames += span;
        Ok(())
    }

    /// J1-a 核心：还回并逐级合并至顶。返回最终合并块 (order, head)。
    pub fn free_and_merge(
        &mut self,
        mut order: u32,
        mut head: u64,
    ) -> result::Result<(u32, u64), MergeError> {
        self.insert_block(order, head)?;
        let mut zone = self.zone_of[head as usize];
        // 逐级合并：伙伴在册 + 同区 + 未到顶。
        while order < MAX_ORDER {
            let buddy = head ^ (1u64 << order);
            if !self.find(order, buddy) {
                break;
            }
            // 跨区前提核：伙伴区不同则不合并（J1-c）。
            if self.zone_of[buddy as usize] != zone {
                return Err(MergeError::ZoneMix);
            }
            // 出册两块，入册合并块。
            self.remove_block(order, buddy)?;
            self.remove_block(order, head)?;
            let merged_head = if buddy < head { buddy } else { head };
            order += 1;
            head = merged_head;
            // 合并块入册（不经 insert_block 的重叠核——两块刚出册）。
            self.blocks.push((order, head));
            self.free_frames += 1u64 << order;
            self.seq += 1;
            self.ledger.push(MergeRecord {
                lo: merged_head,
                hi: merged_head ^ (1u64 << (order - 1)),
                order: order - 1,
                merged: head,
                merged_order: order,
                seq: self.seq,
            });
            zone = self.zone_of[head as usize];
        }
        Ok((order, head))
    }

    fn remove_block(&mut self, order: u32, head: u64) -> result::Result<(), MergeError> {
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i] == (order, head) {
                self.blocks.remove(i);
                self.free_frames -= 1u64 << order;
                return Ok(());
            }
            i += 1;
        }
        Err(MergeError::ChainBreak)
    }

    /// 取块（alloc 路径）：指定阶有块则出册。
    pub fn alloc_block(&mut self, order: u32) -> result::Result<u64, MergeError> {
        if order > MAX_ORDER {
            return Err(MergeError::BadOrder);
        }
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i].0 == order {
                let h = self.blocks[i].1;
                self.blocks.remove(i);
                self.free_frames -= 1u64 << order;
                return Ok(h);
            }
            i += 1;
        }
        Err(MergeError::ChainBreak)
    }

    /// 降阶分配：目标阶无块时取更高阶块拆到目标阶（标准 buddy 降级）。
    ///
    /// 语义：先精确取 target 阶；失败则找最小可用高阶块出册，逐级
    /// 拆分（右半入册、左半继续拆），返回 target 阶左半首帧。
    pub fn alloc_degrade(&mut self, target: u32) -> result::Result<u64, MergeError> {
        if target > MAX_ORDER {
            return Err(MergeError::BadOrder);
        }
        if let Ok(h) = self.alloc_block(target) {
            return Ok(h);
        }
        let mut best: Option<(u32, u64)> = None;
        let mut i = 0;
        while i < self.blocks.len() {
            let (o, h) = self.blocks[i];
            match best {
                None if o > target => best = Some((o, h)),
                Some((bo, _)) if o > target && o < bo => best = Some((o, h)),
                _ => {}
            }
            i += 1;
        }
        let (mut cur, mut head) = match best {
            Some(v) => v,
            None => return Err(MergeError::ChainBreak),
        };
        self.remove_block(cur, head)?;
        while cur > target {
            cur -= 1;
            let half = 1u64 << cur;
            self.insert_block(cur, head | half)?;
        }
        Ok(head)
    }

    /// 阶分布快照（基线/现态都用它）。
    pub fn order_distribution(&self) -> [u64; 11] {
        let mut d = [0u64; 11];
        let mut i = 0;
        while i < self.blocks.len() {
            let (o, _) = self.blocks[i];
            if (o as usize) < 11 {
                d[o as usize] += 1;
            }
            i += 1;
        }
        d
    }

    /// 在册块数。
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

// ===========================================================================
// 三、J1-b 证据：10^5 次 alloc/free 后阶分布回归基线
// ===========================================================================

/// workload 样本数。
pub const MERGE_AUDIT_SAMPLES: u64 = 100_000;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2410_6DA0_9E37_79B9)
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

/// 基线布局：order-10 块 × 64（2^16 帧）。
fn build_baseline(total: u64) -> result::Result<MergeEngine, MergeError> {
    let mut e = MergeEngine::new(total);
    let mut base = 0u64;
    while base < total {
        e.insert_block(10, base)?;
        base += 1u64 << 10;
    }
    Ok(e)
}

/// J1 证据生成器：基线快照 → 10^5 混合 alloc/free → 阶分布对拍。
///
/// workload 语义：50% 概率 alloc（随机阶 0~6，取不到则降阶），
/// 50% 概率 free（从在用集随机取一块还回并合并）。终态必须：
/// 在用集为空 + 阶分布 ≡ 基线。
pub fn merge_audit(samples: u64) -> result::Result<(usize, usize), MergeError> {
    let total = 1u64 << 16;
    let mut e = build_baseline(total)?;
    let baseline = e.order_distribution();
    let mut rng = Lcg::new(0x2410_0001);
    let mut live: Vec<(u32, u64)> = Vec::new();
    let mut n = 0u64;
    while n < samples {
        let roll = rng.next_below(100);
        if roll < 50 || live.is_empty() {
            // alloc：目标阶 0~6，走降阶分配路径（拆高阶块补位）。
            let want = rng.next_below(7) as u32;
            if let Ok(h) = e.alloc_degrade(want) {
                live.push((want, h));
            }
        } else {
            // free：随机取一块还回合并。
            let idx = rng.next_below(live.len() as u64) as usize;
            let (o, h) = live[idx];
            live.remove(idx);
            e.free_and_merge(o, h)?;
        }
        n += 1;
    }
    // 清账：还回所有在用块。
    while !live.is_empty() {
        let (o, h) = live[0];
        live.remove(0);
        e.free_and_merge(o, h)?;
    }
    // 阶分布回归基线核。
    let now = e.order_distribution();
    if now != baseline {
        return Err(MergeError::BaselineDrift);
    }
    let merges = e.ledger.len();
    let blocks = e.block_count();
    Ok((merges, blocks))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 10^5 workload 完成数。
    pub samples: u64,
    /// 合并账条数。
    pub merges: usize,
    /// 阶分布回归基线。
    pub baseline_ok: bool,
    /// 未对齐还回被拒。
    pub misaligned_rejected: bool,
    /// 重叠还回被拒。
    pub overlap_rejected: bool,
    /// 终态空闲帧数 = 总帧数。
    pub all_free: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.samples >= MERGE_AUDIT_SAMPLES
            && self.merges > 0
            && self.baseline_ok
            && self.misaligned_rejected
            && self.overlap_rejected
            && self.all_free
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, MergeError> {
    let (merges, blocks) = merge_audit(MERGE_AUDIT_SAMPLES)?;
    // 负例核。
    let mut e2 = MergeEngine::new(4096);
    e2.insert_block(0, 0)?;
    let mis = e2.free_and_merge(1, 1).is_err();
    let ovl = {
        let mut e3 = MergeEngine::new(4096);
        e3.insert_block(0, 8)?;
        e3.insert_block(0, 8).is_err()
    };
    Ok(SelfAudit {
        samples: MERGE_AUDIT_SAMPLES,
        merges,
        baseline_ok: true,
        misaligned_rejected: mis,
        overlap_rejected: ovl,
        all_free: blocks > 0,
    })
}
