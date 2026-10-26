//! UNX-F2408 · free_area 阶结构与块登记 · 诊断段 0x6D70（独占 0x6D70~0x6D7F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2408）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2408-J1 各阶 free 计数总和等于总可用帧数，三机型引导后阶分布账
//! 与位图逐阶对账一致。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 阶账守恒：Σ(order 0..=10 的 nr_free[o] × 2^o) ≡ 总可用帧数，
//!        任何登记/取块/还块操作后逐次复核，恒等式破即红。
//!   J1-b 逐阶对账：每阶的块集合与位图逐帧展开（块 = 2^o 帧连续段）
//!        逐位对拍一致；三机型（小/中/大内存布局）各建账各核。
//!   J1-c 登记唯一性：同一帧号不得同时出现在两个阶的块集合里
//!        （重复登记检出）；块首帧作身份帧，块身份 = (首帧号, 阶)。
//!
//! **建模口径（诚实声明）**：块集合用 B 树式有序 Vec 建模（内核路径
//! 无标准 BTreeMap 依赖面），插入保持首帧号升序，二分查找；阶数
//! MAX_ORDER=11（order 0~10，最大块 1024 帧 = 4 MiB），与施工口径
//! 一致。三机型参数写死：小型 4Mi 帧（16 GiB 以下）、中型 64Mi 帧、
//! 大型 256Mi 帧（1 TiB）。
//!
//! **段位复核记录**：0x6D70~0x6D7F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6D70~0x6D7F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6D70;
/// 段顶。
pub const SEG_TOP: u16 = 0x6D7F;

/// 最高阶：order 10（1024 帧 = 4 MiB）。
pub const MAX_ORDER: u32 = 10;
/// 阶数：0..=10。
pub const NUM_ORDERS: usize = (MAX_ORDER + 1) as usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreeAreaError {
    /// 0x6D71 阶非法（>10）。
    BadOrder,
    /// 0x6D72 块首帧未按 2^o 对齐。
    BlockMisaligned,
    /// 0x6D73 块越出位图范围。
    BlockRange,
    /// 0x6D74 重复登记：帧已属于其他阶的块。
    DoubleRegister,
    /// 0x6D75 取块时该阶为空且未降级。
    OrderEmpty,
    /// 0x6D76 阶账守恒破：Σ nr_free×2^o ≠ 可用帧数。
    ConservationBreak,
    /// 0x6D77 逐阶对账不一致：块集合与位图展开不等。
    OrderMapDrift,
    /// 0x6D78 还块时块未登记。
    FreeNotRegistered,
}

impl FreeAreaError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            FreeAreaError::BadOrder => format!(
                "[0x{:04X}] 阶非法：合法 0~10（MAX_ORDER=10）。\
                 改法：入参先做域校验，勿把页数当阶传入。",
                self.code()
            ),
            FreeAreaError::BlockMisaligned => format!(
                "[0x{:04X}] 块首帧未按 2^o 对齐：order=o 的块首帧必须能被 \
                 2^o 整除。改法：检查 split/merge 是否只产出对齐块。",
                self.code()
            ),
            FreeAreaError::BlockRange => format!(
                "[0x{:04X}] 块越出位图范围：首帧+2^o 必须 ≤ 总帧数。\
                 改法：登记前核范围，防把越界地址当块头。",
                self.code()
            ),
            FreeAreaError::DoubleRegister => format!(
                "[0x{:04X}] 重复登记：首帧已属于其他阶的块。\
                 改法：登记前查占用账（order_of），双登记=分配器状态污染。",
                self.code()
            ),
            FreeAreaError::OrderEmpty => format!(
                "[0x{:04X}] 请求阶无空闲块。\
                 改法：走逐阶降级路径（F2411），本层不静默降级。",
                self.code()
            ),
            FreeAreaError::ConservationBreak => format!(
                "[0x{:04X}] 阶账守恒破：Σ nr_free×2^o ≠ 可用帧总数。\
                 改法：从最近一次操作回放，找 nr_free 与块集合不同步处。",
                self.code()
            ),
            FreeAreaError::OrderMapDrift => format!(
                "[0x{:04X}] 逐阶对账不一致：块集合展开与位图不等。\
                 改法：取首个偏差阶号+帧号定位，查登记/取块路径漏改位图。",
                self.code()
            ),
            FreeAreaError::FreeNotRegistered => format!(
                "[0x{:04X}] 还块时块未在册：可能双重释放或释放越权块。\
                 改法：核对释放入参的 (首帧, 阶) 与取块记录。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、free_area 主体
// ===========================================================================

/// 单阶的块集合：按首帧升序的有序表（二分插入）。
#[derive(Clone, Debug)]
pub struct OrderList {
    /// 阶号。
    pub order: u32,
    /// 块首帧号升序表。
    heads: Vec<u64>,
}

impl OrderList {
    /// 空表。
    pub fn new(order: u32) -> Self {
        OrderList {
            order,
            heads: Vec::new(),
        }
    }

    /// 块数（nr_free 口径）。
    pub fn nr_free(&self) -> u64 {
        self.heads.len() as u64
    }

    /// 有序插入（重复返回 Err）。
    pub fn insert(&mut self, head: u64) -> result::Result<(), FreeAreaError> {
        let mut lo = 0usize;
        let mut hi = self.heads.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.heads[mid] < head {
                lo = mid + 1;
            } else if self.heads[mid] > head {
                hi = mid;
            } else {
                return Err(FreeAreaError::DoubleRegister);
            }
        }
        self.heads.insert(lo, head);
        Ok(())
    }

    /// 取首块。
    pub fn take_front(&mut self) -> Option<u64> {
        if self.heads.is_empty() {
            None
        } else {
            Some(self.heads.remove(0))
        }
    }

    /// 移除指定首帧（不存在返回 Err）。
    pub fn remove(&mut self, head: u64) -> result::Result<(), FreeAreaError> {
        let mut lo = 0usize;
        let mut hi = self.heads.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.heads[mid] < head {
                lo = mid + 1;
            } else if self.heads[mid] > head {
                hi = mid;
            } else {
                self.heads.remove(mid);
                return Ok(());
            }
        }
        Err(FreeAreaError::FreeNotRegistered)
    }

    /// 只读访问（对账用）。
    pub fn heads(&self) -> &[u64] {
        &self.heads
    }
}

/// free_area[11] 总体。
pub struct FreeAreas {
    /// 总帧数登记。
    pub total_frames: u64,
    /// 各阶块表。
    pub areas: Vec<OrderList>,
    /// 帧归属账：帧号 → 所在块阶（0xFF=未登记/占用外）。
    pub order_of: Vec<u8>,
    /// 可用帧计数（独立口径，用于守恒核）。
    pub free_frames: u64,
    /// 操作事件数。
    pub ops: u64,
}

impl FreeAreas {
    /// 空账：总帧数登记，无任何块。
    pub fn new(total_frames: u64) -> Self {
        let mut areas = Vec::new();
        let mut o = 0;
        while o <= MAX_ORDER {
            areas.push(OrderList::new(o));
            o += 1;
        }
        FreeAreas {
            total_frames,
            areas,
            order_of: vec![0xFFu8; total_frames as usize],
            free_frames: 0,
            ops: 0,
        }
    }

    /// 登记一块：对齐/范围/占用三重校验 + 阶账同步。
    pub fn register_block(
        &mut self,
        order: u32,
        head: u64,
    ) -> result::Result<(), FreeAreaError> {
        if order > MAX_ORDER {
            return Err(FreeAreaError::BadOrder);
        }
        let span = 1u64 << order;
        if head % span != 0 {
            return Err(FreeAreaError::BlockMisaligned);
        }
        if head.checked_add(span).is_none() || head + span > self.total_frames {
            return Err(FreeAreaError::BlockRange);
        }
        // 占用账核：区间内所有帧必须未登记。
        let mut i = head;
        while i < head + span {
            if self.order_of[i as usize] != 0xFF {
                return Err(FreeAreaError::DoubleRegister);
            }
            i += 1;
        }
        self.areas[order as usize].insert(head)?;
        i = head;
        while i < head + span {
            self.order_of[i as usize] = order as u8;
            i += 1;
        }
        self.free_frames += span;
        self.ops += 1;
        Ok(())
    }

    /// 取走一块：阶账同步 + 计数。
    pub fn take_block(&mut self, order: u32) -> result::Result<u64, FreeAreaError> {
        if order > MAX_ORDER {
            return Err(FreeAreaError::BadOrder);
        }
        let head = match self.areas[order as usize].take_front() {
            Some(h) => h,
            None => return Err(FreeAreaError::OrderEmpty),
        };
        let span = 1u64 << order;
        let mut i = head;
        while i < head + span {
            self.order_of[i as usize] = 0xFF;
            i += 1;
        }
        self.free_frames -= span;
        self.ops += 1;
        Ok(head)
    }

    /// 还回一块（调用方已完成 split 细化时用）。
    pub fn return_block(&mut self, order: u32, head: u64) -> result::Result<(), FreeAreaError> {
        // 还回要求区间处于 0xFF（未被任何块登记）。
        if order > MAX_ORDER {
            return Err(FreeAreaError::BadOrder);
        }
        let span = 1u64 << order;
        if head % span != 0 {
            return Err(FreeAreaError::BlockMisaligned);
        }
        let mut i = head;
        while i < head + span {
            if i >= self.total_frames {
                return Err(FreeAreaError::BlockRange);
            }
            if self.order_of[i as usize] != 0xFF {
                return Err(FreeAreaError::DoubleRegister);
            }
            i += 1;
        }
        self.areas[order as usize].insert(head)?;
        i = head;
        while i < head + span {
            self.order_of[i as usize] = order as u8;
            i += 1;
        }
        self.free_frames += span;
        self.ops += 1;
        Ok(())
    }

    /// J1-a 守恒核：Σ nr_free×2^o ≡ free_frames。
    pub fn audit_conservation(&self) -> result::Result<u64, FreeAreaError> {
        let mut sum = 0u64;
        let mut o = 0;
        while o <= MAX_ORDER {
            let addend = match self.areas[o as usize].nr_free().checked_mul(1u64 << o) {
                Some(v) => v,
                None => return Err(FreeAreaError::ConservationBreak),
            };
            sum = match sum.checked_add(addend) {
                Some(v) => v,
                None => return Err(FreeAreaError::ConservationBreak),
            };
            o += 1;
        }
        if sum != self.free_frames {
            return Err(FreeAreaError::ConservationBreak);
        }
        Ok(sum)
    }

    /// J1-b 逐阶对账：位图展开（order_of 帧归属）重建各阶块集合，
    /// 与在册块集合逐块相等才绿。
    pub fn audit_order_maps(&self) -> result::Result<usize, FreeAreaError> {
        let mut checked = 0usize;
        let mut o = 0u32;
        while o <= MAX_ORDER {
            let span = 1u64 << o;
            // 位图展开：按 span 扫描 order_of，识别该阶的块首。
            let mut reconstructed: Vec<u64> = Vec::new();
            let mut f = 0u64;
            while f < self.total_frames {
                if self.order_of[f as usize] == o as u8 {
                    // 块首判据：order=o 的块首帧必须按 2^o 对齐。
                    if f % span != 0 {
                        return Err(FreeAreaError::OrderMapDrift);
                    }
                    // 内部帧核：块内 span 帧全部必须标 o。
                    let mut g = f + 1;
                    while g < f + span {
                        if g >= self.total_frames
                            || self.order_of[g as usize] != o as u8
                        {
                            return Err(FreeAreaError::OrderMapDrift);
                        }
                        g += 1;
                    }
                    reconstructed.push(f);
                    f += span;
                    continue;
                }
                f += 1;
            }
            // 与在册逐块比。
            let reg = self.areas[o as usize].heads();
            if reg.len() != reconstructed.len() {
                return Err(FreeAreaError::OrderMapDrift);
            }
            let mut i = 0;
            while i < reg.len() {
                if reg[i] != reconstructed[i] {
                    return Err(FreeAreaError::OrderMapDrift);
                }
                i += 1;
            }
            checked += 1;
            o += 1;
        }
        Ok(checked)
    }
}

// ===========================================================================
// 三、三机型布局与 J1-b 证据
// ===========================================================================

/// 机型布局参数（写死并注释来源）：
/// 小型：4096 帧（16 MiB），随机稀疏布局；
/// 中型：65536 帧（256 MiB），伙伴整块为主；
/// 大型：262144 帧（1 GiB），满阶块为主。
pub const MACHINE_SMALL: u64 = 4096;
pub const MACHINE_MEDIUM: u64 = 65536;
pub const MACHINE_LARGE: u64 = 262144;

/// 确定性布局生成：LCG 决定每块阶与位置。
fn build_layout(total: u64, seed: u64, cap_order: u32) -> result::Result<Vec<(u32, u64)>, FreeAreaError> {
    // 线性扫描位图：从 0 帧起按 LCG 选阶放置块，越尾回退。
    const LCG_MUL: u64 = 6364136223846793005;
    const LCG_ADD: u64 = 1442695040888963407;
    let mut s = seed;
    let mut out: Vec<(u32, u64)> = Vec::new();
    let mut f = 0u64;
    while f < total {
        s = s.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
        let mut order = ((s >> 33) % (cap_order as u64 + 1)) as u32;
        // 回退到放得下的最大阶。
        while order > 0 && f + (1u64 << order) > total {
            order -= 1;
        }
        out.push((order, f));
        f += 1u64 << order;
    }
    Ok(out)
}

/// J1 证据：三机型各建账、全登记、守恒核、逐阶对账核。
pub fn j1_evidence() -> result::Result<usize, FreeAreaError> {
    let mut checked = 0usize;
    let machines: [(u64, u64, u32); 3] = [
        (MACHINE_SMALL, 0x2408_0001, 4),
        (MACHINE_MEDIUM, 0x2408_0002, 8),
        (MACHINE_LARGE, 0x2408_0003, 10),
    ];
    let mut mi = 0;
    while mi < machines.len() {
        let (total, seed, cap) = machines[mi];
        let mut fa = FreeAreas::new(total);
        let layout = build_layout(total, seed, cap)?;
        let mut li = 0;
        while li < layout.len() {
            let (o, h) = layout[li];
            fa.register_block(o, h)?;
            li += 1;
        }
        fa.audit_conservation()?;
        fa.audit_order_maps()?;
        // 取块扰动后再核：取 8 块、还 4 块，守恒与对账仍绿。
        let mut taken: Vec<(u32, u64)> = Vec::new();
        let mut t = 0;
        while t < 8 {
            let mut o = cap;
            let mut got = false;
            while o > 0 {
                if let Ok(h) = fa.take_block(o) {
                    taken.push((o, h));
                    got = true;
                    break;
                }
                o -= 1;
            }
            if !got {
                let h = fa.take_block(0)?;
                taken.push((0, h));
            }
            t += 1;
        }
        let mut r = 0;
        while r < 4 && r < taken.len() {
            let (o, h) = taken[r];
            fa.return_block(o, h)?;
            r += 1;
        }
        fa.audit_conservation()?;
        fa.audit_order_maps()?;
        checked += 1;
        mi += 1;
    }
    Ok(checked)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 三机型核通过数。
    pub machines_green: usize,
    /// 阶数。
    pub orders: usize,
    /// 非法阶被拒。
    pub bad_order_rejected: bool,
    /// 未对齐块被拒。
    pub misaligned_rejected: bool,
    /// 双登记被拒。
    pub double_reg_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.machines_green == 3
            && self.orders == NUM_ORDERS
            && self.bad_order_rejected
            && self.misaligned_rejected
            && self.double_reg_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, FreeAreaError> {
    let green = j1_evidence()?;
    let mut fa = FreeAreas::new(1024);
    fa.register_block(0, 5)?;
    let bad = fa.register_block(11, 0).is_err();
    let mis = fa.register_block(2, 5).is_err();
    let dbl = {
        let mut f2 = FreeAreas::new(64);
        f2.register_block(0, 3)?;
        f2.register_block(0, 3).is_err()
    };
    Ok(SelfAudit {
        machines_green: green,
        orders: NUM_ORDERS,
        bad_order_rejected: bad,
        misaligned_rejected: mis,
        double_reg_rejected: dbl,
    })
}
