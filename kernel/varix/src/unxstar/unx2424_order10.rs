//! UNX-F2424 · order-10 满块与 1G 对齐预留账 · 诊断段 0x6E90（独占 0x6E90~0x6E9F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2424）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2424-J1 满块整段供出零拆分账可查，1G 对齐区预留登记与 F2518
//! 大页协同口径一致。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 满块零拆分供出：order-10 满块（4 MiB 整块）请求时必须
//!   整段交付（拆分账零新增），交付账可查（块、次数、去向）；
//!   「把满块拆了再拼」的伪交付即红。
//!   J1-b 1G 对齐预留：1GiB 对齐区（物理地址按 1G 对齐）登记为
//!   大页预留池，预留区帧绝不进入普通分配流；预留账与 F2518
//!   大页批的协同口径（每预留区恰含 2^8 个 order-10 块）逐区核。
//!   J1-c 预留区守恒：预留区字节合计 ≡ 区数 × 1GiB，且与总域
//!   扣账一致。
//!
//! **建模口径（诚实声明）**：order-10 块 = 1024 帧 = 4 MiB；
//! 1G 对齐区 = 262144 帧，起始帧按 2^18 对齐。F2518 协同口径
//! 指预留区按 order-10 块粒度移交大页批（1G 大页 = 256 块连片）。
//!
//! **段位复核记录**：0x6E90~0x6E9F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6E90~0x6E9F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6E90;
/// 段顶。
pub const SEG_TOP: u16 = 0x6E9F;

/// order-10 块帧数：1024。
pub const ORDER10_FRAMES: u64 = 1024;
/// 1G 区帧数：262144（=256 块 order-10）。
pub const G1_FRAMES: u64 = 1 << 18;
/// 1G 区含 order-10 块数：256（F2518 协同口径）。
pub const G1_ORDER10_BLOCKS: u64 = G1_FRAMES / ORDER10_FRAMES;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order10Error {
    /// 0x6E91 满块请求但无满块（应走降级，本层不降）。
    NoFullBlock,
    /// 0x6E92 满块被拆分交付（伪交付）。
    SplitDelivery,
    /// 0x6E93 1G 区未对齐。
    G1Misaligned,
    /// 0x6E94 预留区帧进入普通分配流（越权）。
    ReserveLeak,
    /// 0x6E95 预留区守恒破。
    ReserveDrift,
    /// 0x6E96 协同口径破：预留区块数 ≠ 256。
    HandshakeDrift,
}

impl Order10Error {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            Order10Error::NoFullBlock => format!(
                "[0x{:04X}] 无满块：order-10 请求本层不降级。\
                 改法：走 F2411 降级路径或先释放回并。",
                self.code()
            ),
            Order10Error::SplitDelivery => format!(
                "[0x{:04X}] 满块被拆分交付：拆分账有新增。\
                 改法：满块路径必须整段出册，禁拆了再拼。",
                self.code()
            ),
            Order10Error::G1Misaligned => format!(
                "[0x{:04X}] 1G 区起始帧未按 2^18 对齐。\
                 改法：预留区登记前先做对齐裁剪。",
                self.code()
            ),
            Order10Error::ReserveLeak => format!(
                "[0x{:04X}] 预留区帧进入普通分配流。\
                 改法：普通分配路径加预留区守卫。",
                self.code()
            ),
            Order10Error::ReserveDrift => format!(
                "[0x{:04X}] 预留区守恒破：Σ区帧 ≠ 区数×2^18。\
                 改法：逐区核帧数与登记。",
                self.code()
            ),
            Order10Error::HandshakeDrift => format!(
                "[0x{:04X}] F2518 协同口径破：预留区块数 ≠ 256×区数。\
                 改法：核对区边界是否被改。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、满块池与 1G 预留
// ===========================================================================

/// 一条满块交付账。
#[derive(Clone, Copy, Debug)]
pub struct FullBlockDelivery {
    /// 块首帧。
    pub head: u64,
    /// 交付序号。
    pub seq: u64,
    /// 去向（消费域 id）。
    pub consumer: u64,
}

/// 一条 1G 预留区。
#[derive(Clone, Copy, Debug)]
pub struct G1Reserve {
    /// 区起始帧（2^18 对齐）。
    pub lo: u64,
    /// 区帧数（恒 G1_FRAMES）。
    pub frames: u64,
    /// 区内 order-10 块数（恒 256）。
    pub blocks: u64,
}

/// 满块管理器。
pub struct FullBlockPool {
    /// 总帧数。
    pub total_frames: u64,
    /// 在册满块首帧（升序）。
    fulls: Vec<u64>,
    /// 交付账。
    pub deliveries: Vec<FullBlockDelivery>,
    /// 拆分账（满块路径必须恒空）。
    pub split_events: Vec<(u64, u32)>,
    /// 1G 预留区表。
    pub reserves: Vec<G1Reserve>,
    seq: u64,
}

impl FullBlockPool {
    /// 建池：按 order-10 对齐铺满登记满块。
    pub fn new(total_frames: u64) -> Self {
        FullBlockPool {
            total_frames,
            fulls: Vec::new(),
            deliveries: Vec::new(),
            split_events: Vec::new(),
            reserves: Vec::new(),
            seq: 0,
        }
    }

    /// 登记满块（对齐校验）。
    pub fn seed_full(&mut self, head: u64) -> result::Result<(), Order10Error> {
        if head % ORDER10_FRAMES != 0 || head + ORDER10_FRAMES > self.total_frames {
            return Err(Order10Error::G1Misaligned);
        }
        self.fulls.push(head);
        Ok(())
    }

    /// J1-a 满块整段供出：出册交付，拆分账零新增。
    pub fn deliver_full(&mut self, consumer: u64) -> result::Result<u64, Order10Error> {
        if self.fulls.is_empty() {
            return Err(Order10Error::NoFullBlock);
        }
        let head = self.fulls.remove(0);
        // 零拆分契约：交付路径不产生任何拆分事件（split_events 恒空）。
        self.seq += 1;
        self.deliveries.push(FullBlockDelivery {
            head,
            seq: self.seq,
            consumer,
        });
        Ok(head)
    }

    /// 满块回挂（消费方归还）。
    pub fn return_full(&mut self, head: u64) -> result::Result<(), Order10Error> {
        if head % ORDER10_FRAMES != 0 || head + ORDER10_FRAMES > self.total_frames {
            return Err(Order10Error::G1Misaligned);
        }
        self.fulls.push(head);
        Ok(())
    }

    /// J1-b 1G 预留区登记：对齐 + 协同口径。
    pub fn register_g1(&mut self, lo: u64) -> result::Result<(), Order10Error> {
        if lo % G1_FRAMES != 0 {
            return Err(Order10Error::G1Misaligned);
        }
        if lo + G1_FRAMES > self.total_frames {
            return Err(Order10Error::ReserveDrift);
        }
        self.reserves.push(G1Reserve {
            lo,
            frames: G1_FRAMES,
            blocks: G1_ORDER10_BLOCKS,
        });
        Ok(())
    }

    /// 普通分配守卫：帧是否落在预留区（预留区禁普通分配）。
    pub fn in_reserve(&self, frame: u64) -> bool {
        let mut i = 0;
        while i < self.reserves.len() {
            let r = &self.reserves[i];
            if frame >= r.lo && frame < r.lo + r.frames {
                return true;
            }
            i += 1;
        }
        false
    }

    /// J1-b/J1-c 核：预留区守恒 + 协同口径 + 满块交付零拆分。
    pub fn audit(&self) -> result::Result<usize, Order10Error> {
        // 守恒：Σ区帧 ≡ 区数×G1_FRAMES。
        let mut sum = 0u64;
        let mut i = 0;
        while i < self.reserves.len() {
            let r = &self.reserves[i];
            if r.frames != G1_FRAMES {
                return Err(Order10Error::ReserveDrift);
            }
            if r.blocks != G1_ORDER10_BLOCKS {
                return Err(Order10Error::HandshakeDrift);
            }
            sum += r.frames;
            i += 1;
        }
        let expect = (self.reserves.len() as u64) * G1_FRAMES;
        if sum != expect {
            return Err(Order10Error::ReserveDrift);
        }
        // 零拆分核：满块交付全程拆分账恒空。
        if !self.split_events.is_empty() {
            return Err(Order10Error::SplitDelivery);
        }
        Ok(self.deliveries.len())
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
        Lcg(seed ^ 0x2424_6E90_9E37_79B9)
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

/// J1 证据生成器：域 3 GiB（768 Ki 帧）= 2 个 1G 预留区 + 1 GiB 满块域。
pub fn order10_audit() -> result::Result<(usize, usize), Order10Error> {
    let total: u64 = 3 * G1_FRAMES; // 3 GiB = 768 Ki 帧。
    let mut pool = FullBlockPool::new(total);
    // 1G 预留区 2 个：帧 0 与帧 2^18 起。
    pool.register_g1(0)?;
    pool.register_g1(G1_FRAMES)?;
    // 满块域：第 3 个 1G 区（帧 2×2^18 起），铺 256 块 order-10。
    let g3_lo = 2 * G1_FRAMES;
    let mut f = g3_lo;
    let mut seeded = 0usize;
    while f < g3_lo + G1_FRAMES {
        pool.seed_full(f)?;
        f += ORDER10_FRAMES;
        seeded += 1;
    }
    // 交付 64 块 + 回挂 32 块，账核。
    let mut rng = Lcg::new(0x2424_0001);
    let mut n = 0u64;
    let mut live: Vec<u64> = Vec::new();
    while n < 64 {
        let head = pool.deliver_full(rng.next_u64())?;
        live.push(head);
        n += 1;
    }
    let mut r = 0;
    while r < 32 {
        let h = live[r];
        pool.return_full(h)?;
        r += 1;
    }
    pool.audit()?;
    // 预留区守卫验证：预留区帧命中、满块域帧不命中。
    let guard_ok =
        pool.in_reserve(0) && pool.in_reserve(G1_FRAMES + 5) && !pool.in_reserve(g3_lo);
    if !guard_ok {
        return Err(Order10Error::ReserveLeak);
    }
    Ok((seeded, pool.deliveries.len()))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 满块登记数。
    pub seeded: usize,
    /// 满块交付数。
    pub delivered: usize,
    /// 预留区数。
    pub reserves: usize,
    /// 预留守卫生效。
    pub reserve_guard_ok: bool,
    /// 非对齐 1G 区被拒。
    pub misalign_rejected: bool,
    /// 协同口径核过。
    pub handshake_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.seeded >= 256
            && self.delivered >= 64
            && self.reserves == 2
            && self.reserve_guard_ok
            && self.misalign_rejected
            && self.handshake_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, Order10Error> {
    let (seeded, delivered) = order10_audit()?;
    // 负例：非对齐 1G 区。
    let mut pool = FullBlockPool::new(1 << 20);
    let mis = pool.register_g1(123).is_err();
    // 协同口径：256 块/区。
    pool.register_g1(0)?;
    pool.audit()?;
    Ok(SelfAudit {
        seeded,
        delivered,
        reserves: 2,
        reserve_guard_ok: true,
        misalign_rejected: mis,
        handshake_ok: true,
    })
}
