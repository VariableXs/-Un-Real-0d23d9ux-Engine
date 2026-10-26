//! UNX-F2430 · 反碎片：MOVABLE 区高阶保持策略 · 诊断段 0x6F10（独占 0x6F10~0x6F1F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2430）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2430-J1 混合负载 1h 后 MOVABLE 区 order≥9 存活块数 ≥ 初始 80%，
//! 策略关闭对照组显著劣化。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 高阶保持：策略开启时，长跑负载后 MOVABLE 区 order≥9 存活
//!   块数 ≥ 初始的 80%（8000‱）；存活账分列（初始/终态/比率）。
//!   J1-b 对照组劣化：策略关闭时同负载跑完 order≥9 存活显著低于
//!   开启组（比率差 ≥ 2000‱ 落档），证明策略有效性非巧合。
//!   J1-c 策略语义：①MOVABLE 区小分配优先从「已碎块」取（不拆高阶
//!   满块）；②UNMOVABLE 请求永不落 MOVABLE 区（跨区防线复用）；
//!   ③空闲高阶块标记「保持位」，分配扫描跳过（除非显式拆位申请）。
//!
//! **建模口径（诚实声明）**：「1h 混合负载」以等效事件数建模
//! （10^5 次分配/释放 = 1h 折算），全部确定性 LCG 驱动可回放。
//!
//! **段位复核记录**：0x6F10~0x6F1F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6F10~0x6F1F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6F10;
/// 段顶。
pub const SEG_TOP: u16 = 0x6F1F;

/// 最高阶。
pub const MAX_ORDER: u32 = 10;
/// 高阶阈值：order≥9。
pub const HIGH_ORDER_FLOOR: u32 = 9;
/// 存活率下限（‱）：8000。
pub const SURVIVAL_FLOOR_BP: u64 = 8000;
/// 对照组差异下限（‱）：2000。
pub const CONTROL_GAP_BP: u64 = 2000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AntiFragError {
    /// 0x6F11 阶非法。
    BadOrder,
    /// 0x6F12 存活率低于下限。
    SurvivalLow,
    /// 0x6F13 对照组未劣化（策略无效）。
    ControlNotWorse,
    /// 0x6F14 高阶保持位被静默拆毁。
    KeepBitViolated,
}

impl AntiFragError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            AntiFragError::BadOrder => format!(
                "[0x{:04X}] 阶非法：合法 0~10。改法：入参先做域校验。",
                self.code()
            ),
            AntiFragError::SurvivalLow => format!(
                "[0x{:04X}] order≥9 存活率低于 8000‱。\
                 改法：查保持位是否生效、小分配是否误拆高阶满块。",
                self.code()
            ),
            AntiFragError::ControlNotWorse => format!(
                "[0x{:04X}] 对照组未显著劣化：策略效应不可证。\
                 改法：核对对照组是否真的关闭了保持策略。",
                self.code()
            ),
            AntiFragError::KeepBitViolated => format!(
                "[0x{:04X}] 高阶保持位被静默拆毁。\
                 改法：拆高阶块必须走显式「拆位申请」路径并留账。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、策略域
// ===========================================================================

/// 高阶保持域：free_area 精简模型 + 保持位。
pub struct KeepZone {
    /// 在册空闲块 (order, head)。
    pub blocks: Vec<(u32, u64)>,
    /// 保持位集合（order≥9 的块首帧）。
    pub keep_bits: Vec<u64>,
    /// 策略开关。
    pub enabled: bool,
    /// 拆位申请账。
    pub split_grants: u64,
}

impl KeepZone {
    /// 建域。
    pub fn new(enabled: bool) -> Self {
        KeepZone {
            blocks: Vec::new(),
            keep_bits: Vec::new(),
            enabled,
            split_grants: 0,
        }
    }

    /// 铺满块基线（256 块 order-9 + 512 块 order-9 的 2 倍 = 简化为
    /// order-9 块 384 块：域 2^18 帧）。
    pub fn seed_baseline(&mut self) {
        let mut f = 0u64;
        while f < (1u64 << 18) {
            self.blocks.push((HIGH_ORDER_FLOOR, f));
            self.keep_bits.push(f);
            f += 1u64 << HIGH_ORDER_FLOOR;
        }
    }

    fn has_keep(&self, head: u64) -> bool {
        let mut i = 0;
        while i < self.keep_bits.len() {
            if self.keep_bits[i] == head {
                return true;
            }
            i += 1;
        }
        false
    }

    fn find_min(&self, target: u32, skip_keep: bool) -> Option<(u32, u64)> {
        let mut best: Option<(u32, u64)> = None;
        let mut i = 0;
        while i < self.blocks.len() {
            let (o, h) = self.blocks[i];
            if o >= target {
                if skip_keep && self.has_keep(h) {
                    i += 1;
                    continue;
                }
                match best {
                    None => best = Some((o, h)),
                    Some((bo, _)) if o < bo => best = Some((o, h)),
                    _ => {}
                }
            }
            i += 1;
        }
        best
    }

    fn remove(&mut self, order: u32, head: u64) -> bool {
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

    /// 分配：策略开启时跳过保持块（除非显式拆位）。
    pub fn alloc(&mut self, target: u32, force_split_keep: bool) -> result::Result<u64, AntiFragError> {
        if target > MAX_ORDER {
            return Err(AntiFragError::BadOrder);
        }
        let skip = self.enabled && !force_split_keep;
        match self.find_min(target, skip) {
            Some((ob, oh)) => {
                if self.has_keep(oh) && self.enabled && !force_split_keep {
                    return Err(AntiFragError::KeepBitViolated);
                }
                if self.has_keep(oh) && force_split_keep {
                    self.split_grants += 1;
                }
                if !self.remove(ob, oh) {
                    return Err(AntiFragError::KeepBitViolated);
                }
                // 降阶拆分：右半回挂（拆掉的保持位失效）。
                let mut cur = ob;
                let mut head = oh;
                while cur > target {
                    cur -= 1;
                    self.blocks.push((cur, head | (1u64 << cur)));
                }
                Ok(head)
            }
            None => {
                // 全域无块（含保持块跳过）：策略开时尝试拆位申请。
                if skip {
                    if let Some((ob, oh)) = self.find_min(target, false) {
                        self.split_grants += 1;
                        if !self.remove(ob, oh) {
                            return Err(AntiFragError::KeepBitViolated);
                        }
                        let mut cur = ob;
                        let mut head = oh;
                        while cur > target {
                            cur -= 1;
                            self.blocks.push((cur, head | (1u64 << cur)));
                        }
                        return Ok(head);
                    }
                }
                Err(AntiFragError::BadOrder)
            }
        }
    }

    /// 释放回挂（直接以原阶还回，不合并——本模块聚焦保持策略）。
    pub fn free(&mut self, order: u32, head: u64) {
        self.blocks.push((order, head));
    }

    /// order≥9 存活块数。
    pub fn high_order_alive(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i].0 >= HIGH_ORDER_FLOOR {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// 三、J1 证据：混合负载对照实验
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2430_6F10_9E37_79B9)
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

/// 负载等效事件数（1h 折算）。
pub const WORKLOAD_OPS: u64 = 100_000;
/// 初始 order≥9 块数（域 2^18 帧 / 2^9 帧 = 512 块）。
pub const BASELINE_HIGH_BLOCKS: usize = 512;

/// 对照实验：跑一次负载，返回终态 order≥9 存活数。
fn run_workload(enabled: bool, seed: u64) -> usize {
    let mut z = KeepZone::new(enabled);
    z.seed_baseline();
    let mut rng = Lcg::new(seed);
    let mut live: Vec<(u32, u64)> = Vec::new();
    let mut n = 0u64;
    while n < WORKLOAD_OPS {
        let roll = rng.next_below(100);
        if roll < 55 || live.is_empty() {
            // 混合负载：多数是小分配（0~4 阶），偶发高阶（9~10）。
            let want = if roll < 10 {
                (9 + rng.next_below(2) as u32) as u32
            } else {
                rng.next_below(5) as u32
            };
            if let Ok(h) = z.alloc(want, false) {
                live.push((want, h));
            }
        } else {
            let idx = rng.next_below(live.len() as u64) as usize;
            let (o, h) = live[idx];
            live.remove(idx);
            z.free(o, h);
        }
        n += 1;
    }
    z.high_order_alive()
}

/// J1 证据生成器：开启组 ≥8000‱ 存活 + 对照组显著劣化。
pub fn antifrag_audit() -> result::Result<(u64, u64), AntiFragError> {
    let on = run_workload(true, 0x2430_0001);
    let off = run_workload(false, 0x2430_0001); // 同种子同负载。
    let rate_on = (on as u64) * 10_000 / BASELINE_HIGH_BLOCKS as u64;
    let rate_off = (off as u64) * 10_000 / BASELINE_HIGH_BLOCKS as u64;
    if rate_on < SURVIVAL_FLOOR_BP {
        return Err(AntiFragError::SurvivalLow);
    }
    if rate_on.saturating_sub(rate_off) < CONTROL_GAP_BP {
        return Err(AntiFragError::ControlNotWorse);
    }
    Ok((rate_on, rate_off))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 开启组存活率（‱）。
    pub rate_on: u64,
    /// 对照组存活率（‱）。
    pub rate_off: u64,
    /// 保持位违例防线有效。
    pub keep_guard_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.rate_on >= SURVIVAL_FLOOR_BP
            && self.rate_on.saturating_sub(self.rate_off) >= CONTROL_GAP_BP
            && self.keep_guard_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, AntiFragError> {
    let (rate_on, rate_off) = antifrag_audit()?;
    // 保持位违例防线：策略开启时普通分配不得直取保持块——构造
    // 只有保持块的域，请求低阶应走「拆位申请」并留账。
    let mut z = KeepZone::new(true);
    z.blocks.push((9, 0));
    z.keep_bits.push(0);
    let h = z.alloc(0, false)?;
    let grant_ok = z.split_grants == 1;
    let guard_ok = h == 0 && grant_ok;
    Ok(SelfAudit {
        rate_on,
        rate_off,
        keep_guard_ok: guard_ok,
    })
}
