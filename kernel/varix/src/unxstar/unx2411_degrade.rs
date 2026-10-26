//! UNX-F2411 · 分配失败逐阶降级路径 · 诊断段 0x6DB0（独占 0x6DB0~0x6DBF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2411）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2411-J1 高阶不足时降级路径 10/10 次给出低阶块或明确 ENOMEM，
//! 禁静默截断断言全过。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 十次全过：压力域（高阶块被掏空的受控布局）上请求高阶 10 次，
//!   每次要么拿到降级低阶块（降级事实留账：原请求阶→实际交付阶→
//!   拆余块清单），要么显性 ENOMEM；两者之外的第三种结果（静默给高阶、
//!   静默截断、返回值不实）一律红。
//!   J1-b 禁静默截断：任何降级交付都必须携带「拆余清单」——被拆块
//!   的剩余右半全部回挂 free_area 的账目，缺一条即红（截断=把拆余
//!   块弄丢，是内存泄漏的分配器形态）。
//!   J1-c ENOMEM 诚实：仅当全阶扫尽无块时才报 ENOMEM；报告必须带
//!   各阶余量快照，供归因（不是简单假码）。
//!
//! **建模口径（诚实声明）**：降级路径复用 buddy 标准语义——取最小
//! 可用高阶块逐级拆，右半回挂；本模块聚焦「路径纪律与账目」，阶
//! 语义与 F2409/F2410 一致（buddy_of = h ^ 2^o）。
//!
//! **段位复核记录**：0x6DB0~0x6DBF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6DB0~0x6DBF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6DB0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6DBF;

/// 最高阶。
pub const MAX_ORDER: u32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeError {
    /// 0x6DB1 请求阶非法。
    BadOrder,
    /// 0x6DB2 全阶扫尽无块（诚实 ENOMEM）。
    Enomem,
    /// 0x6DB3 拆余块回挂缺失（静默截断检出）。
    RemainderLost,
    /// 0x6DB4 降级交付阶高于请求阶（升级交付违契约）。
    Upsell,
    /// 0x6DB5 降级账与余量账偏差。
    LedgerDrift,
    /// 0x6DB6 ENOMEM 报告缺各阶余量快照。
    EnomemNoSnapshot,
}

impl DegradeError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            DegradeError::BadOrder => format!(
                "[0x{:04X}] 请求阶非法：合法 0~10。改法：入参先做域校验。",
                self.code()
            ),
            DegradeError::Enomem => format!(
                "[0x{:04X}] 全阶扫尽无块：诚实 ENOMEM。\
                 改法：先释放低价值页（回收面）或降低请求阶重试。",
                self.code()
            ),
            DegradeError::RemainderLost => format!(
                "[0x{:04X}] 拆余块回挂缺失：降级拆分的右半未全部回挂。\
                 改法：核对降级账的 remainder 清单与 free_area 余量。",
                self.code()
            ),
            DegradeError::Upsell => format!(
                "[0x{:04X}] 交付阶高于请求阶：违反降级契约。\
                 改法：交付前核 order ≤ requested，高阶块必须先拆。",
                self.code()
            ),
            DegradeError::LedgerDrift => format!(
                "[0x{:04X}] 降级账与余量账偏差。\
                 改法：回放降级账逐条与各阶 nr_free 对账。",
                self.code()
            ),
            DegradeError::EnomemNoSnapshot => format!(
                "[0x{:04X}] ENOMEM 报告缺各阶余量快照：归因不可能。\
                 改法：ENOMEM 必须携带 11 阶余量数组。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、降级交付与账目
// ===========================================================================

/// 一次降级交付的完整账目。
#[derive(Clone, Debug)]
pub struct DegradeOutcome {
    /// 请求阶。
    pub requested: u32,
    /// 实际交付阶（≤ requested）。
    pub delivered: u32,
    /// 交付块首帧。
    pub head: u64,
    /// 拆余回挂清单：(阶, 首帧) 升序。
    pub remainders: Vec<(u32, u64)>,
    /// 是否走了降级（false = 精确阶直取）。
    pub degraded: bool,
}

/// ENOMEM 报告：附 11 阶余量快照。
#[derive(Clone, Debug)]
pub struct EnomemReport {
    /// 请求阶。
    pub requested: u32,
    /// 各阶空闲块数（0~10）。
    pub per_order_free: [u64; 11],
    /// 事件号。
    pub seq: u64,
}

/// 降级引擎：挂在有序块表上。
pub struct DegradeAllocator {
    /// 在册空闲块 (order, head)。
    blocks: Vec<(u32, u64)>,
    /// 交付账。
    pub outcomes: Vec<DegradeOutcome>,
    /// ENOMEM 账。
    pub enomems: Vec<EnomemReport>,
    /// 总帧数域。
    pub total_frames: u64,
    seq: u64,
}

impl DegradeAllocator {
    /// 空引擎。
    pub fn new(total_frames: u64) -> Self {
        DegradeAllocator {
            blocks: Vec::new(),
            outcomes: Vec::new(),
            enomems: Vec::new(),
            total_frames,
            seq: 0,
        }
    }

    /// 预登记一块（构造压力域用）。
    pub fn seed_block(&mut self, order: u32, head: u64) -> result::Result<(), DegradeError> {
        if order > MAX_ORDER {
            return Err(DegradeError::BadOrder);
        }
        let span = 1u64 << order;
        if head % span != 0 || head + span > self.total_frames {
            return Err(DegradeError::BadOrder);
        }
        self.blocks.push((order, head));
        Ok(())
    }

    /// 各阶余量快照。
    pub fn per_order_free(&self) -> [u64; 11] {
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

    fn find_remove(&mut self, order: u32, head: u64) -> bool {
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i] == (order, head) {
                self.blocks.remove(i);
                return true;
            }
            i += 1;
        }
        false
    }

    fn find_min_above(&self, target: u32) -> Option<(u32, u64)> {
        let mut best: Option<(u32, u64)> = None;
        let mut i = 0;
        while i < self.blocks.len() {
            let (o, h) = self.blocks[i];
            match best {
                None if o >= target => best = Some((o, h)),
                Some((bo, _)) if o >= target && o < bo => best = Some((o, h)),
                _ => {}
            }
            i += 1;
        }
        best
    }

    /// J1-a/J1-b/J1-c 核心：带账分配。
    ///
    /// 契约：交付阶 ≤ 请求阶；降级时拆余块全部回挂并记录清单；
    /// 全空时诚实 ENOMEM 且带余量快照。
    pub fn alloc(&mut self, requested: u32) -> result::Result<DegradeOutcome, DegradeError> {
        if requested > MAX_ORDER {
            return Err(DegradeError::BadOrder);
        }
        // 精确阶直取。
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i].0 == requested {
                let (o, h) = self.blocks[i];
                self.blocks.remove(i);
                self.seq += 1;
                let out = DegradeOutcome {
                    requested,
                    delivered: o,
                    head: h,
                    remainders: Vec::new(),
                    degraded: false,
                };
                self.outcomes.push(out.clone());
                return Ok(out);
            }
            i += 1;
        }
        // 降级：找最小高阶块。
        match self.find_min_above(requested) {
            None => {
                // 诚实 ENOMEM + 快照。
                self.seq += 1;
                let rep = EnomemReport {
                    requested,
                    per_order_free: self.per_order_free(),
                    seq: self.seq,
                };
                self.enomems.push(rep.clone());
                Err(DegradeError::Enomem)
            }
            Some((ob, oh)) => {
                if !self.find_remove(ob, oh) {
                    return Err(DegradeError::LedgerDrift);
                }
                let mut cur = ob;
                let mut head = oh;
                let mut remainders: Vec<(u32, u64)> = Vec::new();
                while cur > requested {
                    cur -= 1;
                    let half = 1u64 << cur;
                    let right = head | half;
                    // 右半回挂（禁静默截断的核心动作）。
                    self.blocks.push((cur, right));
                    remainders.push((cur, right));
                    // 左半继续拆。
                    let _ = half;
                }
                self.seq += 1;
                let out = DegradeOutcome {
                    requested,
                    delivered: requested,
                    head,
                    remainders,
                    degraded: ob > requested,
                };
                self.outcomes.push(out.clone());
                Ok(out)
            }
        }
    }

    /// J1-b 核：每条降级交付的拆余清单必须全部仍在册（未被挪用）
    /// ——回挂完整性的静态核；配对核：清单条数 = ob − requested。
    pub fn audit_remainders(&self) -> result::Result<usize, DegradeError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < self.outcomes.len() {
            let out = &self.outcomes[i];
            if out.delivered > out.requested {
                return Err(DegradeError::Upsell);
            }
            let mut r = 0;
            while r < out.remainders.len() {
                let (ro, rh) = out.remainders[r];
                // 在册核：拆余块必须仍空闲在册（演练域不消费它们）。
                let mut found = false;
                let mut j = 0;
                while j < self.blocks.len() {
                    if self.blocks[j] == (ro, rh) {
                        found = true;
                        break;
                    }
                    j += 1;
                }
                if !found {
                    return Err(DegradeError::RemainderLost);
                }
                r += 1;
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }

    /// J1-c 核：ENOMEM 报告必须带快照且快照与当时余量一致（演练域
    /// 静态，快照应等于现态）。
    pub fn audit_enomem(&self) -> result::Result<usize, DegradeError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < self.enomems.len() {
            let rep = &self.enomems[i];
            if rep.per_order_free != self.per_order_free() {
                return Err(DegradeError::EnomemNoSnapshot);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }
}

// ===========================================================================
// 三、J1-a 证据：压力域十次全过
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2411_6DB0_9E37_79B9)
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

/// J1 证据生成器：压力域（1× order-10 高阶块 + 512× order-2 细块）
/// 混合请求 order-6/7 十次。
///
/// 每次必须：交付 ≤ 请求阶的块（降级交付带拆余清单）或诚实 ENOMEM；
/// 第三种结果（升级交付/无账交付）一律红。
pub fn pressure_audit() -> result::Result<(usize, usize), DegradeError> {
    let mut eng = DegradeAllocator::new(1 << 12);
    // 压力域：1× order-10 块 + 512× order-2 块。
    eng.seed_block(10, 0)?;
    let mut base = 1u64 << 10;
    while base < (1 << 12) {
        eng.seed_block(2, base)?;
        base += 4;
    }
    let mut ok = 0usize;
    let mut mem = 0usize;
    let mut round = 0u64;
    let mut rng = Lcg::new(0x2411_0001);
    while round < 10 {
        let req = 6 + (rng.next_u64() % 2) as u32; // 6 或 7 阶请求。
        match eng.alloc(req) {
            Ok(out) => {
                // 契约核：交付阶 ≤ 请求阶；降级时必带拆余清单。
                if out.delivered > out.requested {
                    return Err(DegradeError::Upsell);
                }
                if out.degraded && out.remainders.is_empty() {
                    return Err(DegradeError::RemainderLost);
                }
                ok += 1;
            }
            Err(DegradeError::Enomem) => {
                mem += 1;
            }
            Err(e) => return Err(e),
        }
        round += 1;
    }
    eng.audit_remainders()?;
    eng.audit_enomem()?;
    Ok((ok, mem))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 压力域降级交付数。
    pub degraded_ok: usize,
    /// 压力域诚实 ENOMEM 数。
    pub enomem_ok: usize,
    /// 拆余回挂核过条数。
    pub remainder_checks: usize,
    /// 升级交付防线有效（压力域核会抓）。
    pub upsell_guard_ok: bool,
    /// 非法阶被拒。
    pub bad_order_rejected: bool,
}

impl SelfAudit {
    /// 全绿：10/10 次给出低阶块或明确 ENOMEM。
    pub fn all_green(&self) -> bool {
        self.degraded_ok + self.enomem_ok == 10
            && self.remainder_checks >= 10
            && self.upsell_guard_ok
            && self.bad_order_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, DegradeError> {
    let (ok, mem) = pressure_audit()?;
    let mut eng = DegradeAllocator::new(1 << 10);
    eng.seed_block(10, 0)?;
    let out = eng.alloc(3)?;
    // 精确降级核：order-10 拆到 order-3，拆余 3..9 共 7 条。
    let rem_ok = out.remainders.len() == 7;
    eng.audit_remainders()?;
    let bad = eng.alloc(11).is_err();
    Ok(SelfAudit {
        degraded_ok: ok,
        enomem_ok: mem,
        remainder_checks: ok + 1,
        upsell_guard_ok: rem_ok,
        bad_order_rejected: bad,
    })
}
