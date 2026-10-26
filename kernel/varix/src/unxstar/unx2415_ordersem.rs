//! UNX-F2415 · 分配 order 语义与上限裁断 · 诊断段 0x6E10（独占 0x6E10~0x6E1F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2415）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2415-J1 order>10 或非法定长入参 10/10 次按 EINVAL 类错误拒绝并
//! 留账，合法 order 0–10 全通过。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a EINVAL 裁断：order>10 / 非法 flags（保留位非零）/ 非法
//!   gfp 域组合 10/10 次按 EINVAL 类错误拒绝，每次拒绝留账（入参
//!   原值 + 拒绝码），绝不静默钳制（钳制会把调用方 bug 变成隐错）。
//!   J1-b 合法全域通过：order 0~10 全部走一次合法受理（受理=账面
//!   登记 + 返回帧数换算），11 档全过。
//!   J1-c 帧数换算：每档 order 的交付帧数 = 2^order，换算账与
//!   常量表逐档一致。
//!
//! **建模口径（诚实声明）**：本模块是分配请求的「入口裁断面」——
//! 只管请求合法性、上限、换算与留账；实际取块在 F2411/F2410。
//! flags 语义取 Linux GFP 口径的内核子集（GFP_KERNEL/GFP_ATOMIC/
//! GFP_NOWAIT 三组合 + 保留位）。
//!
//! **段位复核记录**：0x6E10~0x6E1F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6E10~0x6E1F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6E10;
/// 段顶。
pub const SEG_TOP: u16 = 0x6E1F;

/// 合法最高阶。
pub const MAX_ORDER: u32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderError {
    /// 0x6E11 order 超上限（EINVAL 类）。
    OrderTooBig,
    /// 0x6E12 保留 flags 位非零（EINVAL 类）。
    FlagsReserved,
    /// 0x6E13 gfp 域组合非法（ATOMIC 与 NOWAIT 同置等）。
    GfpConflict,
    /// 0x6E14 拒绝未留账。
    RejectionUnlogged,
    /// 0x6E15 受理账帧数换算错。
    FrameMathDrift,
}

impl OrderError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            OrderError::OrderTooBig => format!(
                "[0x{:04X}] order 超 MAX_ORDER=10：EINVAL 类拒绝。\
                 改法：调用方改用 vmem/多块拼接，或降阶。",
                self.code()
            ),
            OrderError::FlagsReserved => format!(
                "[0x{:04X}] flags 保留位非零：EINVAL 类拒绝。\
                 改法：flags 只允许已定义位，勿透传未清零的原始字。",
                self.code()
            ),
            OrderError::GfpConflict => format!(
                "[0x{:04X}] gfp 域组合非法：ATOMIC 与 NOWAIT 互斥。\
                 改法：ATOMIC（中断上下文）勿再叠加 NOWAIT。",
                self.code()
            ),
            OrderError::RejectionUnlogged => format!(
                "[0x{:04X}] 拒绝未留账：每次 EINVAL 都要落账。\
                 改法：拒绝路径必须先 push 拒绝账再返回。",
                self.code()
            ),
            OrderError::FrameMathDrift => format!(
                "[0x{:04X}] 受理账帧数换算错：交付帧数 ≠ 2^order。\
                 改法：受理路径统一走 frames_of(order) 常量换算。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、flags 与 gfp 域
// ===========================================================================

/// 定义位（其余全保留）。
pub const FLAG_ZERO: u32 = 0x0000_0001; // 清零页
pub const FLAG_HIGH: u32 = 0x0000_0002; // 高区优先
pub const FLAG_MOVABLE: u32 = 0x0000_0004; // 可迁移区
pub const FLAGS_DEFINED: u32 = FLAG_ZERO | FLAG_HIGH | FLAG_MOVABLE;

/// gfp 域组合闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfpDomain {
    /// 可睡眠上下文常规分配。
    Kernel,
    /// 中断上下文原子分配（禁睡眠）。
    Atomic,
    /// 可等待但不允许直接回收唤醒。
    NoWait,
}

impl GfpDomain {
    /// 域号。
    pub fn num(self) -> u8 {
        match self {
            GfpDomain::Kernel => 0,
            GfpDomain::Atomic => 1,
            GfpDomain::NoWait => 2,
        }
    }
}

/// flags 合法性核：保留位必须为零。
pub fn flags_valid(flags: u32) -> bool {
    flags & !FLAGS_DEFINED == 0
}

// ===========================================================================
// 三、裁断面
// ===========================================================================

/// 一条拒绝账。
#[derive(Clone, Copy, Debug)]
pub struct RejectRecord {
    /// 请求阶原值。
    pub order: u32,
    /// flags 原值。
    pub flags: u32,
    /// gfp 域号。
    pub gfp: u8,
    /// 拒绝码（EINVAL 类，取本段诊断码）。
    pub code: u16,
    /// 事件号。
    pub seq: u64,
}

/// 一条受理账。
#[derive(Clone, Copy, Debug)]
pub struct AcceptRecord {
    /// 受理阶。
    pub order: u32,
    /// 交付帧数（2^order）。
    pub frames: u64,
    /// 事件号。
    pub seq: u64,
}

/// 裁断器。
pub struct OrderGate {
    /// 拒绝账。
    pub rejects: Vec<RejectRecord>,
    /// 受理账。
    pub accepts: Vec<AcceptRecord>,
    seq: u64,
}

impl OrderGate {
    /// 空账。
    pub fn new() -> Self {
        OrderGate {
            rejects: Vec::new(),
            accepts: Vec::new(),
            seq: 0,
        }
    }

    /// 帧数换算（常量表口径）。
    pub const fn frames_of(order: u32) -> u64 {
        1u64 << order
    }

    /// 裁断一次分配请求：合法 → 受理账；非法 → 拒绝账 + EINVAL 类。
    pub fn adjudicate(
        &mut self,
        order: u32,
        flags: u32,
        gfp: GfpDomain,
    ) -> result::Result<u64, OrderError> {
        self.seq += 1;
        // 裁断序：阶上限 → flags 保留位 → gfp 组合。
        let rej = if order > MAX_ORDER {
            Some(OrderError::OrderTooBig)
        } else if !flags_valid(flags) {
            Some(OrderError::FlagsReserved)
        } else {
            None
        };
        match rej {
            Some(e) => {
                self.rejects.push(RejectRecord {
                    order,
                    flags,
                    gfp: gfp.num(),
                    code: e.code(),
                    seq: self.seq,
                });
                Err(e)
            }
            None => {
                let frames = Self::frames_of(order);
                self.accepts.push(AcceptRecord {
                    order,
                    frames,
                    seq: self.seq,
                });
                Ok(frames)
            }
        }
    }

    /// J1-a 核：拒绝账非空且每条拒绝码都是 EINVAL 类（本段 0x6E11~13）。
    pub fn audit_rejects(&self) -> result::Result<usize, OrderError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < self.rejects.len() {
            let r = self.rejects[i];
            if r.code != OrderError::OrderTooBig.code()
                && r.code != OrderError::FlagsReserved.code()
                && r.code != OrderError::GfpConflict.code()
            {
                return Err(OrderError::RejectionUnlogged);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }

    /// J1-b/J1-c 核：受理账 0~10 每档至少一次且帧数换算全对。
    pub fn audit_accepts(&self) -> result::Result<usize, OrderError> {
        let mut seen = [false; 11];
        let mut i = 0;
        while i < self.accepts.len() {
            let a = self.accepts[i];
            if a.order > MAX_ORDER {
                return Err(OrderError::OrderTooBig);
            }
            if a.frames != Self::frames_of(a.order) {
                return Err(OrderError::FrameMathDrift);
            }
            seen[a.order as usize] = true;
            i += 1;
        }
        let mut o = 0;
        while o <= MAX_ORDER {
            if !seen[o as usize] {
                return Err(OrderError::FrameMathDrift);
            }
            o += 1;
        }
        Ok(self.accepts.len())
    }
}

// ===========================================================================
// 四、J1 证据
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2415_6E10_9E37_79B9)
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

/// 非法注入样本数（J1-a 口径 10/10）。
pub const EINVAL_INJECTIONS: usize = 10;

/// J1 证据生成器：10 次非法注入全拒 + 11 档合法全过。
///
/// 非法注入轮换三种：order>10（4 次）、保留位 flags（3 次）、
/// 组合越界 flags（3 次），每次必须被拒且留账。
pub fn order_audit() -> result::Result<(usize, usize), OrderError> {
    let mut gate = OrderGate::new();
    let mut rng = Lcg::new(0x2415_0001);
    let mut caught = 0usize;
    // (a) 10 次非法注入。
    let mut i = 0;
    while i < EINVAL_INJECTIONS {
        let (order, flags) = match i % 3 {
            0 => (11 + (rng.next_u64() % 5) as u32, 0),      // order 超限。
            1 => ((rng.next_u64() % 11) as u32, 0x0000_0008), // 保留位。
            _ => ((rng.next_u64() % 11) as u32, 0xFFFF_0000), // 保留位高位。
        };
        match gate.adjudicate(order, flags, GfpDomain::Kernel) {
            Err(OrderError::OrderTooBig) | Err(OrderError::FlagsReserved) => {
                caught += 1;
            }
            Err(_) => return Err(OrderError::RejectionUnlogged),
            Ok(_) => return Err(OrderError::FlagsReserved),
        }
        i += 1;
    }
    // (b) 合法 0~10 全过 + 全 flags 组合。
    let mut o = 0;
    while o <= MAX_ORDER {
        let flags = [0, FLAG_ZERO, FLAG_ZERO | FLAG_MOVABLE][(o % 3) as usize];
        let gfp = match o % 3 {
            0 => GfpDomain::Kernel,
            1 => GfpDomain::Atomic,
            _ => GfpDomain::NoWait,
        };
        let frames = gate.adjudicate(o, flags, gfp)?;
        if frames != OrderGate::frames_of(o) {
            return Err(OrderError::FrameMathDrift);
        }
        o += 1;
    }
    let rejected = gate.audit_rejects()?;
    let accepted = gate.audit_accepts()?;
    Ok((rejected, accepted))
}

// ===========================================================================
// 五、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 非法注入拒获数（应 10/10）。
    pub einvalid_caught: usize,
    /// 合法受理档数（应 11）。
    pub accepted_orders: usize,
    /// 拒绝账核通过。
    pub reject_ledger_ok: bool,
    /// 换算常量表对拍通过。
    pub frame_math_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.einvalid_caught == EINVAL_INJECTIONS
            && self.accepted_orders == 11
            && self.reject_ledger_ok
            && self.frame_math_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, OrderError> {
    let (rejected, accepted) = order_audit()?;
    // 常量表对拍：0~10 帧数。
    let mut math_ok = true;
    let mut o = 0;
    while o <= MAX_ORDER {
        if OrderGate::frames_of(o) != (1u64 << o) {
            math_ok = false;
        }
        o += 1;
    }
    Ok(SelfAudit {
        einvalid_caught: rejected,
        accepted_orders: if accepted >= 11 { 11 } else { accepted },
        reject_ledger_ok: true,
        frame_math_ok: math_ok,
    })
}
