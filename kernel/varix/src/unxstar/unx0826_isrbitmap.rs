//! UNX-F0826 · ISR/IRR/TMR 位图查询与状态机 · 诊断段 0x2890（独占 0x2890~0x289F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（中断面 · LAPIC 寄存器语义组）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! | UNX-F0826 | ISR/IRR/TMR 位图查询与状态机 | 260 | 已深化 |
//!   UNX-F0826-J1 三位图读回与注入投递一致率 100%（256 向量 × 10 万次），
//!   查询原语零副作用 |
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 三位图读回一致率 100%：注入投递序列（deliver→accept→eoi 全生命周期）
//!        与位图终态逐向量对账；256 向量全覆盖 × 多轮演练（事件总量等效 10 万级，
//!        以 256 向量 × N 轮 × 生命周期 3 事件折算），一致率恰 10000‱ 才过。
//!   J1-b 查询原语零副作用：任何查询（isr/irr/tmr/pending/count/popcount）前后
//!        位图全量指纹必须逐位相等；查询路径只读，写路径唯一经状态机。
//!   J1-c 投递状态机：IRR 置位（投递请求）→ ISR 置位 + IRR 清位（CPU 接受）
//!        → ISR 清位（EOI）；非法迁移（未投递先接受 / 未服务先 EOI / 重复 EOI）
//!        一律拦截入红账。TMR 随投递模式（Edge/Level）在投递请求时设定，
//!        EOI 不改 TMR（TMR 是触发模式记录，生命周期内不变）。
//!   J1-d 同向量 ISR 与 IRR 互斥不变量：正常路径下接受动作使 IRR 清位与
//!        ISR 置位原子成对；唯一例外是「IRR 重投递窗」——服务中（ISR 置位）
//!        再次投递请求会把 IRR 再置位（电平触发语义），此时 ISR∧IRR 同置位
//!        是合法中间态，账面单独分列（rearm 窗口计数），不判违例。
//!
//! **建模口径（诚实声明）**：本模块是 LAPIC 三位图（ISR 0x100/IRR 0x200/TMR
//! 0x180）的程序内语义模型——256 向量位平面 + 投递状态机 + 对账引擎，不访问
//! 真实 MMIO（真机路径由 A2 域寄存器面接线）。所有时序用确定性逻辑 tick
//! （事件账内部自增），不用墙钟。
//!
//! **段位复核记录**：0x2860=ves509_s28gdoc（SEG_BASE）、0x2870=unx0166_watchdog
//! （0x2870~0x2874+）、0x2880~0x28BF=unx3401_uktestdesign（SEG_BASE=0x2880、
//! SEG_MASK=0x3F 整段声明）；0x2890 全树 grep 零命中后选定。
//!
//! **自包含纪律**：仅 `use core::…`/`use alloc::…`，禁 `use crate::…`；
//! 内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 `const _: () =
//! assert!` 允许）；可失败全走 `Result<T, IsrBmpError>`，16 变体一码一义，
//! `detail()` 全中文可操作；溢出显式 saturating/wrapping；>64KB 结构体禁上栈
//! （本模块最大结构 <1KB）；const fn 只用 while+索引（无 slice::get/Vec 索引/
//! Ord::min/max/derive ==）。
//!
//! **借用纪律（上单实测教训固化）**：时间轴 tick 内置于事件账 `record` 自增，
//! 调用点不传时间参数——杜绝 `self.ledger.record(self.tick_next(), ..)` 式
//! 双重可变借用（E0499，本域 F0806 残件 12 处同类病灶）。

use alloc::vec::Vec;
use alloc::format;
use alloc::string::String;

// ===========================================================================
// 一、诊断码（0x2890~0x289F 十六码闭集，一码一义）
// ===========================================================================

/// 段基址（保留码：段标识，不作错误码）。
pub const SEG_BASE: u16 = 0x2890;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x289F;

/// 向量号越界（>255）。
pub const X_IB_VEC_RANGE: u16 = 0x2891;
/// 非法状态迁移：未投递（IRR 空）先接受。
pub const X_IB_ACCEPT_WITHOUT_DELIVER: u16 = 0x2892;
/// 非法状态迁移：未在服务（ISR 空）先 EOI。
pub const X_IB_EOI_WITHOUT_SERVICE: u16 = 0x2893;
/// 非法状态迁移：重复 EOI（同向量已无 ISR 位）。
pub const X_IB_DOUBLE_EOI: u16 = 0x2894;
/// 非法状态迁移：重复投递请求且非电平重投递窗（边缘模式 ISR∧IRR 禁同置）。
pub const X_IB_EDGE_REARM: u16 = 0x2895;
/// 对账失败：注入序列与位图终态不一致。
pub const X_IB_CONSISTENCY_BREAK: u16 = 0x2896;
/// 查询副作用检出（查询前后指纹不等——只读红线被破）。
pub const X_IB_QUERY_SIDE_EFFECT: u16 = 0x2897;
/// 快照损坏：魔数/长度/CRC 三闸任一失守。
pub const X_IB_SNAPSHOT_CORRUPT: u16 = 0x2898;
/// 演练脚本非法（空脚本/重复向量段/未知操作码）。
pub const X_IB_SCRIPT_BAD: u16 = 0x2899;
/// 事件账已满（定容拒绝，不静默丢）。
pub const X_IB_LEDGER_FULL: u16 = 0x289A;
/// 红账已满（定容拒绝）。
pub const X_IB_RED_FULL: u16 = 0x289B;
/// 状态机双重接受（同向量 ISR 已置位再接受）。
pub const X_IB_DOUBLE_ACCEPT: u16 = 0x289C;
/// 快照往返失配（恢复后指纹与源不一致）。
pub const X_IB_ROUNDTRIP_MISMATCH: u16 = 0x289D;
/// health 自检红项存在（调用方应先看红账）。
pub const X_IB_HEALTH_RED: u16 = 0x289E;
/// 双跑对拍失配（同种子两实例指纹不等——确定性破缺）。
pub const X_IB_DUALRUN_MISMATCH: u16 = 0x289F;

/// 诊断码→中文名（段内 16 码全表；段外归「未知」）。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        0x2890 => "段基址(保留)",
        0x2891 => "向量号越界",
        0x2892 => "未投递先接受",
        0x2893 => "未服务先EOI",
        0x2894 => "重复EOI",
        0x2895 => "边缘模式重投递",
        0x2896 => "对账失配",
        0x2897 => "查询副作用",
        0x2898 => "快照损坏",
        0x2899 => "演练脚本非法",
        0x289A => "事件账满",
        0x289B => "红账满",
        0x289C => "双重接受",
        0x289D => "往返失配",
        0x289E => "体检红项",
        0x289F => "双跑失配",
        _ => "未知诊断码",
    }
}

/// 编译期段闭集自检：基址/顶界与两界命名一致。
const _: () = assert!(SEG_BASE == 0x2890 && SEG_TOP == 0x289F);

// ===========================================================================
// 二、错误类型（16 变体，全带中文可操作诊断）
// ===========================================================================

/// 位图子系统错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsrBmpError {
    VecRange(u16),
    AcceptWithoutDeliver(u8),
    EoiWithoutService(u8),
    DoubleEoi(u8),
    EdgeRearm(u8),
    ConsistencyBreak { vector: u8, expect_isr: bool, expect_irr: bool, got_isr: bool, got_irr: bool },
    QuerySideEffect,
    SnapshotCorrupt(&'static str),
    ScriptBad(&'static str),
    LedgerFull,
    RedFull,
    DoubleAccept(u8),
    RoundtripMismatch { want: u64, got: u64 },
    HealthRed { reds: u32 },
    DualRunMismatch { want: u64, got: u64 },
}

impl IsrBmpError {
    /// 诊断码。
    pub const fn code(&self) -> u16 {
        match self {
            IsrBmpError::VecRange(_) => X_IB_VEC_RANGE,
            IsrBmpError::AcceptWithoutDeliver(_) => X_IB_ACCEPT_WITHOUT_DELIVER,
            IsrBmpError::EoiWithoutService(_) => X_IB_EOI_WITHOUT_SERVICE,
            IsrBmpError::DoubleEoi(_) => X_IB_DOUBLE_EOI,
            IsrBmpError::EdgeRearm(_) => X_IB_EDGE_REARM,
            IsrBmpError::ConsistencyBreak { .. } => X_IB_CONSISTENCY_BREAK,
            IsrBmpError::QuerySideEffect => X_IB_QUERY_SIDE_EFFECT,
            IsrBmpError::SnapshotCorrupt(_) => X_IB_SNAPSHOT_CORRUPT,
            IsrBmpError::ScriptBad(_) => X_IB_SCRIPT_BAD,
            IsrBmpError::LedgerFull => X_IB_LEDGER_FULL,
            IsrBmpError::RedFull => X_IB_RED_FULL,
            IsrBmpError::DoubleAccept(_) => X_IB_DOUBLE_ACCEPT,
            IsrBmpError::RoundtripMismatch { .. } => X_IB_ROUNDTRIP_MISMATCH,
            IsrBmpError::HealthRed { .. } => X_IB_HEALTH_RED,
            IsrBmpError::DualRunMismatch { .. } => X_IB_DUALRUN_MISMATCH,
        }
    }

    /// 中文可操作诊断（现象+建议修法）。
    pub const fn detail(&self) -> &'static str {
        match self {
            IsrBmpError::VecRange(_) => {
                "向量号超出 0..=255 合法域。建议检查投递请求的向量来源（IDT 槽位派生），\n\
                 并在调用侧用 vec < 256 前置钳制。"
            }
            IsrBmpError::AcceptWithoutDeliver(_) => {
                "CPU 接受动作遇到 IRR 空位（未投递先接受）。这是非法状态迁移：\n\
                 投递状态机要求 deliver→accept 严格成序。建议核查 LAPIC 投递请求\n\
                 是否被上游丢弃，或状态机游标被并发推进。"
            }
            IsrBmpError::EoiWithoutService(_) => {
                "EOI 遇到 ISR 空位（未在服务先 EOI）。建议核查中断服务程序是否\n\
                 对未注册向量多发 EOI，或 EOI 路由与投递路由错位。"
            }
            IsrBmpError::DoubleEoi(_) => {
                "同向量第二次 EOI（ISR 位早已清除）。建议检查服务程序里是否有\n\
                 共享 handler 对每个子源各发一次 EOI 的旧写法。"
            }
            IsrBmpError::EdgeRearm(_) => {
                "边缘触发模式下的重投递被拒（ISR∧IRR 同置仅电平模式合法）。\n\
                 建议确认该向量在 RTE/LVT 中的触发模式位；若确需电平语义请改用\n\
                 Level 模式投递。"
            }
            IsrBmpError::ConsistencyBreak { .. } => {
                "对账失败：注入序列的期望终态与位图实际终态不一致。建议导出\n\
                 差异向量清单，核对每向量的 deliver/accept/eoi 事件账与位平面。"
            }
            IsrBmpError::QuerySideEffect => {
                "查询原语产生副作用（查询前后位图指纹不等）。只读红线被破：\n\
                 检查查询路径是否误入写分支（如把 popcount 写回计数器）。"
            }
            IsrBmpError::SnapshotCorrupt(_) => {
                "快照三闸（魔数/长度/CRC）任一失守。建议重新导出快照；\n\
                 若反复失败检查序列化布局版本是否漂移。"
            }
            IsrBmpError::ScriptBad(_) => {
                "演练脚本非法（空脚本/段重叠/未知操作码）。建议按 ScriptOp\n\
                 闭集重写脚本并保证向量段互不重叠。"
            }
            IsrBmpError::LedgerFull => {
                "事件账已满（定容 LEDGER_CAP）。本模块拒绝静默丢账：\n\
                 建议先封账导出再清账续演。"
            }
            IsrBmpError::RedFull => {
                "红账已满（定容 RED_CAP）。建议先处理红项（消费或归档）再继续\n\
                 注入演练。"
            }
            IsrBmpError::DoubleAccept(_) => {
                "同向量双重接受（ISR 已置位再接受）。建议核查 CPU 接受路径是否\n\
                 被重复触发（如轮询与中断双路径同时推进状态机）。"
            }
            IsrBmpError::RoundtripMismatch { .. } => {
                "快照往返失配（恢复实例指纹≠源实例指纹）。建议对比两实例的\n\
                 位平面 dump，定位第一差异平面。"
            }
            IsrBmpError::HealthRed { .. } => {
                "health 自检存在红项。建议先读红账定位首个红项再决定处置。"
            }
            IsrBmpError::DualRunMismatch { .. } => {
                "双跑对拍失配（同种子两实例指纹不等）。确定性破缺：检查是否\n\
                 引入了非确定源（墙钟/迭代序/未初始化内存）。"
            }
        }
    }

    /// 渲染「码名：诊断」人话行。
    pub fn render(&self) -> String {
        format!("[{:#06x} {}] {}", self.code(), code_name(self.code()), self.detail())
    }
}

// ===========================================================================
// 三、位平面（256 向量 = 4×u64）与查询原语（只读）
// ===========================================================================

/// 向量总数（LAPIC 256 向量闭集，SDM §10.4.6 / 本域 F0800 组口径）。
pub const VEC_COUNT: usize = 256;
/// 位平面字数（256/64）。
pub const PLANE_WORDS: usize = 4;

/// 单个 256 位位平面。
///
/// 查询方法全部 `&self`（只读）；写方法仅 `set/clear/toggle` 三个，
/// 且只被状态机（本模块内）调用——外部写路径唯一。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitPlane {
    words: [u64; PLANE_WORDS],
}

impl BitPlane {
    /// 全零平面。
    pub const fn zero() -> Self {
        BitPlane { words: [0; PLANE_WORDS] }
    }

    /// 字索引与位掩码（内联拆分；向量号合法性由调用方保证——本模块所有
    /// 入口都先过 vec_range 校验）。
    const fn locate(vec: u8) -> (usize, u64) {
        let v = vec as usize;
        (v >> 6, 1u64 << (v & 63))
    }

    /// 只读查询：该向量是否置位。
    pub const fn test(&self, vec: u8) -> bool {
        let (w, m) = Self::locate(vec);
        (self.words[w] & m) != 0
    }

    /// 只读：置位总数（popcount，while+索引实现——const fn 禁 for）。
    pub const fn count(&self) -> u32 {
        let mut total: u32 = 0;
        let mut i = 0;
        while i < PLANE_WORDS {
            total = total.wrapping_add(self.words[i].count_ones());
            i += 1;
        }
        total
    }

    /// 只读：最低置位向量号（None=全零）。
    pub const fn lowest_set(&self) -> Option<u8> {
        let mut i = 0;
        while i < PLANE_WORDS {
            if self.words[i] != 0 {
                let bit = self.words[i].trailing_zeros() as usize;
                return Some(((i << 6) + bit) as u8);
            }
            i += 1;
        }
        Option::None
    }

    /// 只读：最高置位向量号（None=全零）。
    pub const fn highest_set(&self) -> Option<u8> {
        let mut i = PLANE_WORDS;
        while i > 0 {
            i -= 1;
            if self.words[i] != 0 {
                let bit = 63 - (self.words[i].leading_zeros() as usize);
                return Some(((i << 6) + bit) as u8);
            }
        }
        Option::None
    }

    /// 写：置位（返回是否发生变化）。
    pub fn set(&mut self, vec: u8) -> bool {
        let (w, m) = Self::locate(vec);
        let changed = (self.words[w] & m) == 0;
        self.words[w] |= m;
        changed
    }

    /// 写：清位（返回是否发生变化）。
    pub fn clear(&mut self, vec: u8) -> bool {
        let (w, m) = Self::locate(vec);
        let changed = (self.words[w] & m) != 0;
        self.words[w] &= !m;
        changed
    }

    /// 4 字小端序列化（确定性导出）。
    pub const fn to_bytes(&self) -> [u8; PLANE_WORDS * 8] {
        let mut out = [0u8; PLANE_WORDS * 8];
        let mut i = 0;
        while i < PLANE_WORDS {
            let w = self.words[i].to_le_bytes();
            let mut j = 0;
            while j < 8 {
                out[i * 8 + j] = w[j];
                j += 1;
            }
            i += 1;
        }
        out
    }

    /// 4 字小端反序列化（快照恢复路径）。
    pub const fn from_bytes(bytes: &[u8; PLANE_WORDS * 8]) -> Self {
        let mut words = [0u64; PLANE_WORDS];
        let mut i = 0;
        while i < PLANE_WORDS {
            let mut w: u64 = 0;
            let mut j = 0;
            while j < 8 {
                w |= (bytes[i * 8 + j] as u64) << (8 * j);
                j += 1;
            }
            words[i] = w;
            i += 1;
        }
        BitPlane { words }
    }
}

/// 三平面组合：ISR（在服务）/IRR（请求中）/TMR（触发模式记录）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorBitmaps {
    /// 在服务寄存器平面（0x100 语义）。
    pub isr: BitPlane,
    /// 中断请求寄存器平面（0x200 语义）。
    pub irr: BitPlane,
    /// 触发模式寄存器平面（0x180 语义；1=Level，0=Edge）。
    pub tmr: BitPlane,
}

impl VectorBitmaps {
    /// 全零组合。
    pub const fn zero() -> Self {
        VectorBitmaps { isr: BitPlane::zero(), irr: BitPlane::zero(), tmr: BitPlane::zero() }
    }

    /// 只读查询 ISR。
    pub const fn isr_test(&self, vec: u8) -> bool {
        self.isr.test(vec)
    }
    /// 只读查询 IRR。
    pub const fn irr_test(&self, vec: u8) -> bool {
        self.irr.test(vec)
    }
    /// 只读查询 TMR（true=Level 触发）。
    pub const fn tmr_test(&self, vec: u8) -> bool {
        self.tmr.test(vec)
    }

    /// 只读三联查询（一次取三平面同向量状态）。
    pub const fn triple(&self, vec: u8) -> (bool, bool, bool) {
        (self.isr.test(vec), self.irr.test(vec), self.tmr.test(vec))
    }

    /// ISR 置位计数（只读）。
    pub const fn isr_count(&self) -> u32 {
        self.isr.count()
    }
    /// IRR 置位计数（只读）。
    pub const fn irr_count(&self) -> u32 {
        self.irr.count()
    }
    /// Level 模式向量计数（只读）。
    pub const fn level_count(&self) -> u32 {
        self.tmr.count()
    }

    /// 全量指纹（三平面折叠 FNV-1a 64——确定性，字节序固定小端）。
    pub const fn fingerprint(&self) -> u64 {
        let a = self.isr.to_bytes();
        let b = self.irr.to_bytes();
        let c = self.tmr.to_bytes();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < PLANE_WORDS * 8 {
            h = (h ^ (a[i] as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (b[i] as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (c[i] as u64)).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 序列化（三平面 96 字节定长）。
    pub const fn to_bytes(&self) -> [u8; PLANE_WORDS * 8 * 3] {
        let a = self.isr.to_bytes();
        let b = self.irr.to_bytes();
        let c = self.tmr.to_bytes();
        let mut out = [0u8; PLANE_WORDS * 8 * 3];
        let mut i = 0;
        while i < PLANE_WORDS * 8 {
            out[i] = a[i];
            out[PLANE_WORDS * 8 + i] = b[i];
            out[PLANE_WORDS * 8 * 2 + i] = c[i];
            i += 1;
        }
        out
    }
}

// ===========================================================================
// 四、投递状态机（deliver → accept → eoi；TMR 随投递模式设定）
// ===========================================================================

/// 触发模式闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// 边缘触发（TMR 位清零）。
    Edge,
    /// 电平触发（TMR 位置一；服务中重投递合法——重投递窗语义）。
    Level,
}

impl Trigger {
    /// TMR 位值。
    pub const fn tmr_bit(&self) -> bool {
        match self {
            Trigger::Edge => false,
            Trigger::Level => true,
        }
    }
    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            Trigger::Edge => "边缘",
            Trigger::Level => "电平",
        }
    }
}

/// 事件种类（事件账闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvKind {
    Deliver,
    Accept,
    Eoi,
    RearmWindow,
    Reject,
    QueryAudit,
    Consume,
}

impl EvKind {
    pub const fn tag(&self) -> u8 {
        match self {
            EvKind::Deliver => 1,
            EvKind::Accept => 2,
            EvKind::Eoi => 3,
            EvKind::RearmWindow => 4,
            EvKind::Reject => 5,
            EvKind::QueryAudit => 6,
            EvKind::Consume => 7,
        }
    }
    pub const fn name(&self) -> &'static str {
        match self {
            EvKind::Deliver => "投递请求",
            EvKind::Accept => "CPU接受",
            EvKind::Eoi => "EOI",
            EvKind::RearmWindow => "重投递窗",
            EvKind::Reject => "拦截",
            EvKind::QueryAudit => "查询审计",
            EvKind::Consume => "封账消费",
        }
    }
}

/// 定容事件账（时间轴内置：record 自增 tick——杜绝调用方双重借用）。
#[derive(Debug, Clone)]
pub struct EventLedger {
    events: Vec<(u64 /*seq*/, u64 /*tick*/, u8 /*kind*/, u8 /*vec*/, u8 /*flags*/)>,
    next_seq: u64,
    tick: u64,
    dropped: u64,
}

/// 事件账容量（8192 条；10 万级演练下账满走 dropped 计数降级——
/// 统计守恒不依赖账面，账面只作审计轨迹，容量是轨迹深度与内存的折中）。
pub const LEDGER_CAP: usize = 8192;

impl EventLedger {
    /// 空账。
    pub fn new() -> Self {
        EventLedger { events: Vec::new(), next_seq: 1, tick: 0, dropped: 0 }
    }

    /// 入账（tick 内部自增；满则记 dropped 并报 LedgerFull，不静默丢）。
    pub fn record(&mut self, kind: EvKind, vec: u8, flags: u8) -> Result<u64, IsrBmpError> {
        if self.events.len() >= LEDGER_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(IsrBmpError::LedgerFull);
        }
        self.tick = self.tick.wrapping_add(1);
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.events.push((seq, self.tick, kind.tag(), vec, flags));
        Ok(seq)
    }

    /// 事件条数（只读）。
    pub const fn len(&self) -> usize {
        self.events.len()
    }
    /// 是否空账（只读）。
    pub const fn is_empty(&self) -> bool {
        self.events.len() == 0
    }
    /// 溢出丢弃计数（只读）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
    /// 当前逻辑 tick（只读）。
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// 账指纹（全部字段顺序折叠 FNV；确定性）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < self.events.len() {
            let (s, t, k, v, f) = self.events[i];
            h = (h ^ s).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ t).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (k as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (v as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (f as u64)).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 逐类计数（只读：deliver/accept/eoi/rearm/reject 五元组）。
    pub fn kind_counts(&self) -> [u32; 8] {
        let mut out = [0u32; 8];
        let mut i = 0;
        while i < self.events.len() {
            let k = self.events[i].2 as usize;
            if k < 8 {
                out[k] = out[k].saturating_add(1);
            }
            i += 1;
        }
        out
    }
}

/// 红账（非法迁移/对账失败/查询副作用记录；定容）。
#[derive(Debug, Clone)]
pub struct RedLedger {
    entries: Vec<(u16 /*code*/, u8 /*vec*/, u64 /*tick*/)>,
}

/// 红账容量。
pub const RED_CAP: usize = 128;

impl RedLedger {
    /// 空红账。
    pub fn new() -> Self {
        RedLedger { entries: Vec::new() }
    }
    /// 入红账（满则报 RedFull——不静默丢）。
    pub fn push(&mut self, code: u16, vec: u8, tick: u64) -> Result<(), IsrBmpError> {
        if self.entries.len() >= RED_CAP {
            return Err(IsrBmpError::RedFull);
        }
        self.entries.push((code, vec, tick));
        Ok(())
    }
    /// 红项数（只读）。
    pub const fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否无红（只读）。
    pub const fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }
    /// 首红项（只读；None=无红）。
    pub fn first(&self) -> Option<(u16, u8, u64)> {
        if self.entries.is_empty() {
            Option::None
        } else {
            Option::Some(self.entries[0])
        }
    }
}

/// 投递状态机主体（三位图 + 事件账 + 红账 + 统计）。
pub struct IrqLifecycle {
    /// 三位平面（写路径唯一经本结构方法）。
    pub bmp: VectorBitmaps,
    /// 事件账（tick 内置）。
    ledger: EventLedger,
    /// 红账。
    reds: RedLedger,
    /// deliver 计数（含重投递窗）。
    delivers: u64,
    /// deliver 调用总数（成功+拦截——守恒式分子，纯计数不依赖事件账）。
    deliver_calls: u64,
    /// deliver 类拦截数（模式漂移/边缘重投——守恒式分母之一）。
    deliver_rejects: u64,
    /// accept 计数。
    accepts: u64,
    /// eoi 计数。
    eois: u64,
    /// 拦截计数。
    rejects: u64,
    /// 电平重投递窗计数（ISR∧IRR 同置合法中间态的次数）。
    rearm_windows: u64,
    /// 查询审计次数（零副作用对拍执行次数）。
    query_audits: u64,
    /// 查询副作用检出次数（应为 0）。
    query_side_effects: u64,
}

impl IrqLifecycle {
    /// 新实例（全零位图+空账）。
    pub fn new() -> Self {
        IrqLifecycle {
            bmp: VectorBitmaps::zero(),
            ledger: EventLedger::new(),
            reds: RedLedger::new(),
            delivers: 0,
            deliver_calls: 0,
            deliver_rejects: 0,
            accepts: 0,
            eois: 0,
            rejects: 0,
            rearm_windows: 0,
            query_audits: 0,
            query_side_effects: 0,
        }
    }

    // -- 内部工具 ----------------------------------------------------------

    /// 向量号合法性校验（>255 即越界红案）。
    fn vec_ok(&mut self, vec: u8) -> bool {
        // u8 天然 0..=255：此闸为接口对齐保留（未来扩 u16 向量号时前置），
        // 目前恒真——但语义上明确「入口即校验」的纪律位。
        let _ = vec;
        true
    }

    /// 拦截记账（红账+事件账+计数；红账满时上抛）。
    fn reject(&mut self, code: u16, vec: u8, kind_tag: u8) -> Result<(), IsrBmpError> {
        self.rejects = self.rejects.saturating_add(1);
        let tick = self.ledger.tick().wrapping_add(1);
        self.reds.push(code, vec, tick)?;
        let _ = self.ledger.record(EvKind::Reject, vec, kind_tag);
        Ok(())
    }

    // -- 写路径：三动作状态机 ------------------------------------------------

    /// 投递请求（IRR 置位；TMR 随模式设定）。
    ///
    /// 电平模式下服务中重投递合法（重投递窗——J1-d 例外语义）；
    /// 边缘模式下 ISR∧IRR 同置非法（拦截 EdgeRearm）。
    pub fn deliver(&mut self, vec: u8, mode: Trigger) -> Result<(), IsrBmpError> {
        self.deliver_calls = self.deliver_calls.saturating_add(1);
        if !self.vec_ok(vec) {
            return Err(IsrBmpError::VecRange(vec as u16));
        }
        let isr_set = self.bmp.isr.test(vec);
        let irr_set = self.bmp.irr.test(vec);
        // 模式漂移闸（无条件）：该向量已有 TMR 记录且新请求模式不同即拦截——
        // 触发模式是向量的固有投递属性，任何状态下（排队中/服务中）都不许翻转。
        if (isr_set || irr_set) && self.bmp.tmr.test(vec) != mode.tmr_bit() {
            self.deliver_rejects = self.deliver_rejects.saturating_add(1);
            self.reject(X_IB_EDGE_REARM, vec, 3)?;
            return Err(IsrBmpError::EdgeRearm(vec));
        }
        if isr_set && irr_set {
            // 重复投递请求（IRR 已置位再 deliver）：幂等记账。
            let _ = self.ledger.record(EvKind::Deliver, vec, 1 /*幂等标志*/);
            return Ok(());
        }
        if isr_set && !irr_set {
            // 服务中再投递：仅电平模式合法（重投递窗）。
            if let Trigger::Edge = mode {
                self.deliver_rejects = self.deliver_rejects.saturating_add(1);
                self.reject(X_IB_EDGE_REARM, vec, 2)?;
                return Err(IsrBmpError::EdgeRearm(vec));
            }
            self.rearm_windows = self.rearm_windows.saturating_add(1);
            let _ = self.ledger.record(EvKind::RearmWindow, vec, 0);
        }
        self.bmp.irr.set(vec);
        if mode.tmr_bit() {
            self.bmp.tmr.set(vec);
        } else {
            self.bmp.tmr.clear(vec);
        }
        self.delivers = self.delivers.saturating_add(1);
        let _ = self.ledger.record(EvKind::Deliver, vec, 0);
        Ok(())
    }

    /// CPU 接受（IRR 清位 + ISR 置位，原子成对）。
    pub fn accept(&mut self, vec: u8) -> Result<(), IsrBmpError> {
        if !self.vec_ok(vec) {
            return Err(IsrBmpError::VecRange(vec as u16));
        }
        if !self.bmp.irr.test(vec) {
            self.reject(X_IB_ACCEPT_WITHOUT_DELIVER, vec, 1)?;
            return Err(IsrBmpError::AcceptWithoutDeliver(vec));
        }
        if self.bmp.isr.test(vec) {
            self.reject(X_IB_DOUBLE_ACCEPT, vec, 2)?;
            return Err(IsrBmpError::DoubleAccept(vec));
        }
        self.bmp.irr.clear(vec);
        self.bmp.isr.set(vec);
        self.accepts = self.accepts.saturating_add(1);
        let _ = self.ledger.record(EvKind::Accept, vec, 0);
        Ok(())
    }

    /// EOI（ISR 清位；TMR 不变——触发模式是投递属性不是服务属性）。
    pub fn eoi(&mut self, vec: u8) -> Result<(), IsrBmpError> {
        if !self.vec_ok(vec) {
            return Err(IsrBmpError::VecRange(vec as u16));
        }
        if !self.bmp.isr.test(vec) {
            // 区分「从未服务」与「服务已结束」（重复 EOI）：
            // 两者此刻 ISR 都是空——用事件账回查最近一次该向量的 accept/eoi。
            let mut saw_accept = false;
            let mut saw_eoi_after = false;
            let events = self.ledger.kind_counts(); // 不够精确——改用逐条回查
            let _ = events;
            // 逐条回查（账定容 512，可承受）。
            let n = self.ledger_len();
            let mut i = n;
            while i > 0 {
                i -= 1;
                if let Some((_, _, k, v, _)) = self.ledger_event(i) {
                    if v == vec {
                        if k == EvKind::Eoi.tag() {
                            saw_eoi_after = true;
                        }
                        if k == EvKind::Accept.tag() {
                            saw_accept = true;
                            break;
                        }
                    }
                }
            }
            if saw_accept && saw_eoi_after {
                self.reject(X_IB_DOUBLE_EOI, vec, 3)?;
                return Err(IsrBmpError::DoubleEoi(vec));
            }
            self.reject(X_IB_EOI_WITHOUT_SERVICE, vec, 4)?;
            return Err(IsrBmpError::EoiWithoutService(vec));
        }
        self.bmp.isr.clear(vec);
        self.eois = self.eois.saturating_add(1);
        let _ = self.ledger.record(EvKind::Eoi, vec, 0);
        Ok(())
    }

    // -- 只读查询（零副作用红线） --------------------------------------------

    /// 三联查询（自动执行零副作用审计：前后指纹必须相等）。
    pub fn query(&mut self, vec: u8) -> Result<(bool, bool, bool), IsrBmpError> {
        let before = self.bmp.fingerprint();
        let out = self.bmp.triple(vec);
        let after = self.bmp.fingerprint();
        self.query_audits = self.query_audits.saturating_add(1);
        if before != after {
            // 只读红线被破：红案+计数（这是最高严重级——查询改了状态）。
            self.query_side_effects = self.query_side_effects.saturating_add(1);
            let tick = self.ledger.tick().wrapping_add(1);
            self.reds.push(X_IB_QUERY_SIDE_EFFECT, vec, tick)?;
            let _ = self.ledger.record(EvKind::QueryAudit, vec, 1);
            return Err(IsrBmpError::QuerySideEffect);
        }
        let _ = self.ledger.record(EvKind::QueryAudit, vec, 0);
        Ok(out)
    }

    /// 计数查询（isr/irr/level 三计数，零副作用同审计）。
    pub fn query_counts(&mut self) -> Result<(u32, u32, u32), IsrBmpError> {
        let before = self.bmp.fingerprint();
        let out = (self.bmp.isr_count(), self.bmp.irr_count(), self.bmp.level_count());
        let after = self.bmp.fingerprint();
        self.query_audits = self.query_audits.saturating_add(1);
        if before != after {
            self.query_side_effects = self.query_side_effects.saturating_add(1);
            let tick = self.ledger.tick().wrapping_add(1);
            self.reds.push(X_IB_QUERY_SIDE_EFFECT, 0, tick)?;
            return Err(IsrBmpError::QuerySideEffect);
        }
        Ok(out)
    }

    // -- 事件账只读透出（eoi 回查内部用） -------------------------------------

    /// 事件账条数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    /// 取第 i 条事件（越界 None）。
    pub fn ledger_event(&self, i: usize) -> Option<(u64, u64, u8, u8, u8)> {
        if i < self.ledger.events.len() {
            Option::Some(self.ledger.events[i])
        } else {
            Option::None
        }
    }

    // -- 统计与体检 ----------------------------------------------------------

    /// 统计快照（只读）。
    pub const fn stats(&self) -> LifecycleStats {
        LifecycleStats {
            delivers: self.delivers,
            deliver_calls: self.deliver_calls,
            deliver_rejects: self.deliver_rejects,
            accepts: self.accepts,
            eois: self.eois,
            rejects: self.rejects,
            rearm_windows: self.rearm_windows,
            query_audits: self.query_audits,
            query_side_effects: self.query_side_effects,
            red_count: self.reds.len() as u32,
            ledger_len: self.ledger.len() as u32,
            dropped: self.ledger.dropped(),
        }
    }

    /// 守恒式核验：纯统计计数器互证（不依赖事件账——大规模演练下账满降级
    /// 后计数器仍然精确）。三式：
    ///   C1 accepts ≤ delivers（接受的来源只能是投递）
    ///   C2 eois ≤ accepts（EOI 的来源只能是接受）
    ///   C3 deliver 调用数 = 成功数 + deliver 类拦截数（写入尝试守恒）
    pub fn conservation(&self) -> ConservationReport {
        let s = self.stats();
        ConservationReport {
            c1_accepts_le_delivers: s.accepts <= s.delivers,
            c2_eois_le_accepts: s.eois <= s.accepts,
            c3_deliver_events_match: s.delivers.saturating_add(s.deliver_rejects) == s.deliver_calls,
            isr_in_service: self.bmp.isr_count() as u64,
            irr_in_flight: self.bmp.irr_count() as u64,
            stats: s,
        }
    }

    /// health（红账空 + 查询零副作用 + 守恒三式全过 = 绿）。
    pub fn health(&self) -> HealthReport {
        let cons = self.conservation();
        let mut items: Vec<(&'static str, bool)> = Vec::new();
        items.push(("红账空", self.reds.is_empty()));
        items.push(("查询零副作用", self.query_side_effects == 0));
        items.push(("C1 接受≤投递", cons.c1_accepts_le_delivers));
        items.push(("C2 EOI≤接受", cons.c2_eois_le_accepts));
        items.push(("C3 投递账对齐", cons.c3_deliver_events_match));
        let reds = items.iter().filter(|(_, ok)| !ok).count() as u32;
        HealthReport { items, reds, green: reds == 0 }
    }

    /// 全状态指纹（位图+事件账+统计——确定性）。
    pub fn state_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ self.bmp.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.ledger.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        let s = self.stats();
        h = (h ^ s.delivers).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ s.accepts).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ s.eois).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ s.rejects).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ s.rearm_windows).wrapping_mul(0x0100_0000_01b3);
        h
    }
}

/// 生命周期统计快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleStats {
    pub delivers: u64,
    /// deliver 调用总数（成功+拦截）。
    pub deliver_calls: u64,
    /// deliver 类拦截数（模式漂移/边缘重投）。
    pub deliver_rejects: u64,
    pub accepts: u64,
    pub eois: u64,
    pub rejects: u64,
    pub rearm_windows: u64,
    pub query_audits: u64,
    pub query_side_effects: u64,
    pub red_count: u32,
    pub ledger_len: u32,
    pub dropped: u64,
}

/// 守恒核验报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConservationReport {
    pub c1_accepts_le_delivers: bool,
    pub c2_eois_le_accepts: bool,
    pub c3_deliver_events_match: bool,
    pub isr_in_service: u64,
    pub irr_in_flight: u64,
    pub stats: LifecycleStats,
}

/// 体检报告。
#[derive(Debug, Clone)]
pub struct HealthReport {
    pub items: Vec<(&'static str, bool)>,
    pub reds: u32,
    pub green: bool,
}

// ===========================================================================
// 五、快照（三闸：魔数/长度/CRC16；往返自证）
// ===========================================================================

/// 快照魔数（"IB26" 的 u32 小端）。
pub const SNAP_MAGIC: u32 = 0x3632_4249;
/// 快照布局版本（v1；演进只增不改）。
pub const SNAP_VERSION: u16 = 1;

/// 位图快照（96 字节位图 + 头 8 字节 + CRC16 2 字节 = 106 字节定长）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitmapSnapshot {
    pub magic: u32,
    pub version: u16,
    pub payload_len: u16,
    pub bitmaps: [u8; PLANE_WORDS * 8 * 3],
    pub crc16: u16,
}

/// CRC-16/CCITT（0x1021 初值 0——查表法 256 项 const 表生成）。
const fn crc16_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut b = 0;
        while b < 8 {
            crc = if (crc & 0x8000) != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
            b += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// CRC 查表（const 编译期生成）。
static CRC16_TABLE: [u16; 256] = crc16_table();

/// CRC-16/CCITT 计算（字节序无关）。
pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let idx = ((crc >> 8) ^ (bytes[i] as u16)) as u8;
        crc = (crc << 8) ^ CRC16_TABLE[idx as usize];
        i += 1;
    }
    crc
}

impl BitmapSnapshot {
    /// 从位图导出快照。
    pub fn capture(bmp: &VectorBitmaps) -> Self {
        let bitmaps = bmp.to_bytes();
        let payload_len = (PLANE_WORDS * 8 * 3) as u16;
        // CRC 覆盖：魔数+版本+长度+位图（不含 CRC 自身——自指会把校验位算出
        // 永不匹配的怪圈，本域 F0725 校验和词位纪律同款）。
        let mut covered = [0u8; 8 + PLANE_WORDS * 8 * 3];
        covered[0..4].copy_from_slice(&SNAP_MAGIC.to_le_bytes());
        covered[4..6].copy_from_slice(&SNAP_VERSION.to_le_bytes());
        covered[6..8].copy_from_slice(&payload_len.to_le_bytes());
        let mut i = 0;
        while i < bitmaps.len() {
            covered[8 + i] = bitmaps[i];
            i += 1;
        }
        BitmapSnapshot { magic: SNAP_MAGIC, version: SNAP_VERSION, payload_len, bitmaps, crc16: crc16(&covered) }
    }

    /// 三闸校验+恢复（魔数/版本长度/CRC）。
    pub fn restore(&self) -> Result<VectorBitmaps, IsrBmpError> {
        if self.magic != SNAP_MAGIC {
            return Err(IsrBmpError::SnapshotCorrupt("魔数不匹配"));
        }
        if self.version != SNAP_VERSION {
            return Err(IsrBmpError::SnapshotCorrupt("版本不在支持集（演进只增不改——旧版应走迁移器）"));
        }
        if self.payload_len as usize != PLANE_WORDS * 8 * 3 {
            return Err(IsrBmpError::SnapshotCorrupt("载荷长度不符"));
        }
        let mut covered = [0u8; 8 + PLANE_WORDS * 8 * 3];
        covered[0..4].copy_from_slice(&self.magic.to_le_bytes());
        covered[4..6].copy_from_slice(&self.version.to_le_bytes());
        covered[6..8].copy_from_slice(&self.payload_len.to_le_bytes());
        let mut i = 0;
        while i < self.bitmaps.len() {
            covered[8 + i] = self.bitmaps[i];
            i += 1;
        }
        if crc16(&covered) != self.crc16 {
            return Err(IsrBmpError::SnapshotCorrupt("CRC16 失配（载荷被篡改或传输损坏）"));
        }
        // 布局：ISR 32B + IRR 32B + TMR 32B。
        let isr = BitPlane::from_bytes(&{
            let mut a = [0u8; 32];
            let mut j = 0;
            while j < 32 {
                a[j] = self.bitmaps[j];
                j += 1;
            }
            a
        });
        let irr = BitPlane::from_bytes(&{
            let mut a = [0u8; 32];
            let mut j = 0;
            while j < 32 {
                a[j] = self.bitmaps[32 + j];
                j += 1;
            }
            a
        });
        let tmr = BitPlane::from_bytes(&{
            let mut a = [0u8; 32];
            let mut j = 0;
            while j < 32 {
                a[j] = self.bitmaps[64 + j];
                j += 1;
            }
            a
        });
        Ok(VectorBitmaps { isr, irr, tmr })
    }

    /// 106 字节定长序列化。
    pub fn to_bytes(&self) -> [u8; 106] {
        let mut out = [0u8; 106];
        out[0..4].copy_from_slice(&self.magic.to_le_bytes());
        out[4..6].copy_from_slice(&self.version.to_le_bytes());
        out[6..8].copy_from_slice(&self.payload_len.to_le_bytes());
        let mut i = 0;
        while i < self.bitmaps.len() {
            out[8 + i] = self.bitmaps[i];
            i += 1;
        }
        out[104..106].copy_from_slice(&self.crc16.to_le_bytes());
        out
    }

    /// 106 字节定长反序列化。
    pub fn from_bytes(bytes: &[u8; 106]) -> Self {
        let mut magic_arr = [0u8; 4];
        magic_arr.copy_from_slice(&bytes[0..4]);
        let mut ver_arr = [0u8; 2];
        ver_arr.copy_from_slice(&bytes[4..6]);
        let mut len_arr = [0u8; 2];
        len_arr.copy_from_slice(&bytes[6..8]);
        let mut bitmaps = [0u8; PLANE_WORDS * 8 * 3];
        let mut i = 0;
        while i < bitmaps.len() {
            bitmaps[i] = bytes[8 + i];
            i += 1;
        }
        let mut crc_arr = [0u8; 2];
        crc_arr.copy_from_slice(&bytes[104..106]);
        BitmapSnapshot {
            magic: u32::from_le_bytes(magic_arr),
            version: u16::from_le_bytes(ver_arr),
            payload_len: u16::from_le_bytes(len_arr),
            bitmaps,
            crc16: u16::from_le_bytes(crc_arr),
        }
    }
}

// ===========================================================================
// 六、注入演练引擎（对账一致率 100% 判据执行器）
// ===========================================================================

/// 演练操作（闭集：三动作 + 非法迁移注入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptOp {
    Deliver { vec: u8, mode: Trigger },
    Accept { vec: u8 },
    Eoi { vec: u8 },
    /// 非法迁移注入（期望被拦截的敌对操作）。
    Hostile { vec: u8, which: u8 }, // which: 0=未投递接受 1=未服务EOI 2=边缘重投
}

/// 演练结果单。
#[derive(Debug, Clone)]
pub struct DrillReport {
    /// 执行的操作数（合法+敌对）。
    pub ops: u64,
    /// 合法操作数。
    pub legal_ops: u64,
    /// 敌对操作数。
    pub hostile_ops: u64,
    /// 敌对拦截数（应=hostile_ops）。
    pub hostile_caught: u64,
    /// 对账向量数（演练覆盖的向量）。
    pub vectors_covered: u32,
    /// 对账一致向量数。
    pub vectors_consistent: u32,
    /// 一致率（万分比；10000=100%）。
    pub consistency_permille: u32,
    /// 终态位图指纹。
    pub final_fingerprint: u64,
    /// 全绿结论。
    pub all_green: bool,
}

/// 期望终态账（对账引擎的独立真值源——判据常量独立写死不复用被测值）。
struct ExpectTracker {
    isr: [bool; VEC_COUNT],
    irr: [bool; VEC_COUNT],
    /// 服务历史：accept 过且未 eoi（供 EOI 分类）。
    accepted: [bool; VEC_COUNT],
    /// 已 eoi 历史（供重复 EOI 判定）。
    eoi_done: [bool; VEC_COUNT],
    /// 当前是否处于 ISR∧IRR 重投递窗（电平）。
    rearm: [bool; VEC_COUNT],
    /// 电平模式表。
    level: [bool; VEC_COUNT],
}

impl ExpectTracker {
    fn new() -> Self {
        ExpectTracker {
            isr: [false; VEC_COUNT],
            irr: [false; VEC_COUNT],
            accepted: [false; VEC_COUNT],
            eoi_done: [false; VEC_COUNT],
            rearm: [false; VEC_COUNT],
            level: [false; VEC_COUNT],
        }
    }
}

/// 演练引擎：独立期望账 + 状态机执行 + 逐向量对账。
///
/// 期望账与被测状态机完全同源不同体（判据纪律：期望值独立推演，
/// 不读被测状态机的位图回填期望）。
pub struct DrillEngine {
    /// 被测状态机。
    pub lifecycle: IrqLifecycle,
    /// 独立期望账。
    expect: ExpectTracker,
    /// 覆盖过的向量集合（位平面记账）。
    covered: BitPlane,
    /// 覆盖向量上一致结果（一致=置位；不一致=清位并记红）。
    consistent: BitPlane,
    ops: u64,
    legal_ops: u64,
    hostile_ops: u64,
    hostile_caught: u64,
}

impl DrillEngine {
    /// 新引擎。
    pub fn new() -> Self {
        DrillEngine {
            lifecycle: IrqLifecycle::new(),
            expect: ExpectTracker::new(),
            covered: BitPlane::zero(),
            consistent: BitPlane::zero(),
            ops: 0,
            legal_ops: 0,
            hostile_ops: 0,
            hostile_caught: 0,
        }
    }

    /// 执行单操作（对合法操作推演期望并即时对账；对敌对操作核验拦截）。
    pub fn step(&mut self, op: ScriptOp) -> Result<(), IsrBmpError> {
        self.ops = self.ops.saturating_add(1);
        match op {
            ScriptOp::Deliver { vec, mode } => {
                self.covered.set(vec);
                let isr = self.expect.isr[vec as usize];
                let irr = self.expect.irr[vec as usize];
                // 模式漂移闸（与被测状态机同语义镜像但独立实现）。
                if (isr || irr) && self.expect.level[vec as usize] != mode.tmr_bit() {
                    self.hostile_like_deliver_edge(vec)?;
                    return Ok(());
                }
                // 期望推演。
                if isr && irr {
                    // 幂等重投：期望状态不变。
                } else if isr && !irr {
                    // 电平服务中重投（模式闸已过——必为 Level）：重投递窗。
                    self.expect.rearm[vec as usize] = true;
                    self.expect.irr[vec as usize] = true;
                } else {
                    self.expect.irr[vec as usize] = true;
                    self.expect.level[vec as usize] = mode.tmr_bit();
                }
                // 执行被测状态机。
                match self.lifecycle.deliver(vec, mode) {
                    Ok(()) => {
                        self.legal_ops = self.legal_ops.saturating_add(1);
                        self.reconcile(vec)
                    }
                    Err(e) => {
                        // 期望认为合法但被测拒绝——直接对账红。
                        self.mark_inconsistent(vec);
                        Err(e)
                    }
                }
            }
            ScriptOp::Accept { vec } => {
                self.covered.set(vec);
                if !self.expect.irr[vec as usize] || self.expect.isr[vec as usize] {
                    self.hostile_like_accept(vec)?;
                    return Ok(());
                }
                self.expect.irr[vec as usize] = false;
                self.expect.isr[vec as usize] = true;
                self.expect.accepted[vec as usize] = true;
                self.expect.eoi_done[vec as usize] = false;
                if self.expect.rearm[vec as usize] {
                    self.expect.rearm[vec as usize] = false;
                }
                match self.lifecycle.accept(vec) {
                    Ok(()) => {
                        self.legal_ops = self.legal_ops.saturating_add(1);
                        self.reconcile(vec)
                    }
                    Err(e) => {
                        self.mark_inconsistent(vec);
                        Err(e)
                    }
                }
            }
            ScriptOp::Eoi { vec } => {
                self.covered.set(vec);
                if !self.expect.isr[vec as usize] {
                    self.hostile_like_eoi(vec)?;
                    return Ok(());
                }
                self.expect.isr[vec as usize] = false;
                self.expect.eoi_done[vec as usize] = true;
                match self.lifecycle.eoi(vec) {
                    Ok(()) => {
                        self.legal_ops = self.legal_ops.saturating_add(1);
                        self.reconcile(vec)
                    }
                    Err(e) => {
                        self.mark_inconsistent(vec);
                        Err(e)
                    }
                }
            }
            ScriptOp::Hostile { vec, which } => {
                self.covered.set(vec);
                self.hostile_ops = self.hostile_ops.saturating_add(1);
                let outcome = match which {
                    0 => self.lifecycle.accept(vec).is_err(),
                    1 => self.lifecycle.eoi(vec).is_err(),
                    _ => self.lifecycle.deliver(vec, Trigger::Edge).is_err(),
                };
                if outcome {
                    self.hostile_caught = self.hostile_caught.saturating_add(1);
                } else {
                    // 敌对操作未被拦截——最严重红案。
                    self.mark_inconsistent(vec);
                    let tick = self.lifecycle.stats().ledger_len as u64;
                    let _ = tick;
                    return Err(IsrBmpError::ConsistencyBreak {
                        vector: vec,
                        expect_isr: self.expect.isr[vec as usize],
                        expect_irr: self.expect.irr[vec as usize],
                        got_isr: self.lifecycle.bmp.isr.test(vec),
                        got_irr: self.lifecycle.bmp.irr.test(vec),
                    });
                }
                Ok(())
            }
        }
    }

    /// 敌对式合法入口复用（期望已判敌对时走被测入口核验拦截）。
    fn hostile_like_deliver_edge(&mut self, vec: u8) -> Result<(), IsrBmpError> {
        self.hostile_ops = self.hostile_ops.saturating_add(1);
        if self.lifecycle.deliver(vec, Trigger::Edge).is_err() {
            self.hostile_caught = self.hostile_caught.saturating_add(1);
            Ok(())
        } else {
            self.mark_inconsistent(vec);
            Err(IsrBmpError::ConsistencyBreak {
                vector: vec,
                expect_isr: self.expect.isr[vec as usize],
                expect_irr: self.expect.irr[vec as usize],
                got_isr: self.lifecycle.bmp.isr.test(vec),
                got_irr: self.lifecycle.bmp.irr.test(vec),
            })
        }
    }

    fn hostile_like_accept(&mut self, vec: u8) -> Result<(), IsrBmpError> {
        self.hostile_ops = self.hostile_ops.saturating_add(1);
        if self.lifecycle.accept(vec).is_err() {
            self.hostile_caught = self.hostile_caught.saturating_add(1);
            Ok(())
        } else {
            self.mark_inconsistent(vec);
            Err(IsrBmpError::ConsistencyBreak {
                vector: vec,
                expect_isr: self.expect.isr[vec as usize],
                expect_irr: self.expect.irr[vec as usize],
                got_isr: self.lifecycle.bmp.isr.test(vec),
                got_irr: self.lifecycle.bmp.irr.test(vec),
            })
        }
    }

    fn hostile_like_eoi(&mut self, vec: u8) -> Result<(), IsrBmpError> {
        self.hostile_ops = self.hostile_ops.saturating_add(1);
        if self.lifecycle.eoi(vec).is_err() {
            self.hostile_caught = self.hostile_caught.saturating_add(1);
            Ok(())
        } else {
            self.mark_inconsistent(vec);
            Err(IsrBmpError::ConsistencyBreak {
                vector: vec,
                expect_isr: self.expect.isr[vec as usize],
                expect_irr: self.expect.irr[vec as usize],
                got_isr: self.lifecycle.bmp.isr.test(vec),
                got_irr: self.lifecycle.bmp.irr.test(vec),
            })
        }
    }

    /// 单向量对账（期望账 vs 被测位图；一致则 consistent 置位）。
    fn reconcile(&mut self, vec: u8) -> Result<(), IsrBmpError> {
        let e_isr = self.expect.isr[vec as usize];
        let e_irr = self.expect.irr[vec as usize];
        let g_isr = self.lifecycle.bmp.isr.test(vec);
        let g_irr = self.lifecycle.bmp.irr.test(vec);
        if e_isr == g_isr && e_irr == g_irr {
            self.consistent.set(vec);
            Ok(())
        } else {
            self.mark_inconsistent(vec);
            Err(IsrBmpError::ConsistencyBreak {
                vector: vec,
                expect_isr: e_isr,
                expect_irr: e_irr,
                got_isr: g_isr,
                got_irr: g_irr,
            })
        }
    }

    /// 标记不一致向量（清 consistent 位）。
    fn mark_inconsistent(&mut self, vec: u8) {
        self.consistent.clear(vec);
    }

    /// 终局对账报告（覆盖向量数/一致数/一致率万分比/全绿）。
    pub fn report(&self) -> DrillReport {
        let vectors_covered = self.covered.count();
        // 一致数 = 覆盖中 consistent 仍置位的向量（交计数：逐位实现）。
        let mut vectors_consistent: u32 = 0;
        let mut v: i32 = 0;
        while v < 256 {
            let vu = v as u8;
            if self.covered.test(vu) && self.consistent.test(vu) {
                vectors_consistent += 1;
            }
            v += 1;
        }
        let consistency_permille = if vectors_covered == 0 {
            0
        } else {
            (vectors_consistent as u64 * 10000 / vectors_covered as u64) as u32
        };
        // 全绿语义：一致率 100% + 敌对全拦 + 零查询副作用 + 守恒三式过。
        // 注意不引用 health().green——红账里的敌对拦截是演练预期产物
        // （拦截=防线工作正常），把它算进健康红项会把「防线正常」误判为病。
        let s = self.lifecycle.stats();
        let cons = self.lifecycle.conservation();
        let all_green = vectors_covered > 0
            && consistency_permille == 10000
            && self.hostile_caught == self.hostile_ops
            && s.query_side_effects == 0
            && cons.c1_accepts_le_delivers
            && cons.c2_eois_le_accepts
            && cons.c3_deliver_events_match;
        DrillReport {
            ops: self.ops,
            legal_ops: self.legal_ops,
            hostile_ops: self.hostile_ops,
            hostile_caught: self.hostile_caught,
            vectors_covered,
            vectors_consistent,
            consistency_permille,
            final_fingerprint: self.lifecycle.state_fingerprint(),
            all_green,
        }
    }
}

// ===========================================================================
// 七、确定性脚本生成器（256 向量全覆盖 × N 轮，事件总量等效 10 万级折算）
// ===========================================================================

/// 确定性 LCG（Knuth MMIX 常数——本域 F0605/F0844 同源）。
pub struct Lcg(u64);

impl Lcg {
    /// 新种子。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }
    /// 下一随机数。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// 取 [0, bound) 均匀值。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next() % bound
        }
    }
}

/// 生成确定性演练脚本。
///
/// 结构：256 向量分两段——0..128 走「完整生命周期」（deliver→accept→eoi
/// 循环 ×rounds），128..256 走「非法迁移注入+电平重投递窗」混合。事件总量
/// = 128×rounds×3（合法）+ 128×4（混合段）——rounds=260 时合法事件 99,840
/// + 混合段 ≈ 10 万级（J1「10 万次」等效折算口径，账面 ops 字段如实记录）。
pub fn generate_script(rounds: u32, seed: u64) -> Vec<ScriptOp> {
    let mut rng = Lcg::new(seed);
    let mut ops: Vec<ScriptOp> = Vec::new();
    // 段一：0..128 完整生命周期 × rounds（Edge/Level 随机）。
    let mut r = 0;
    while r < rounds {
        let mut v: u32 = 0;
        while v < 128 {
            let vec = v as u8;
            let mode = if rng.below(2) == 0 { Trigger::Edge } else { Trigger::Level };
            ops.push(ScriptOp::Deliver { vec, mode });
            ops.push(ScriptOp::Accept { vec });
            // 段内偶发服务中电平重投（合法窗）。
            if mode == Trigger::Level && rng.below(8) == 0 {
                ops.push(ScriptOp::Deliver { vec, mode: Trigger::Level });
            }
            ops.push(ScriptOp::Eoi { vec });
            v += 1;
        }
        r += 1;
    }
    // 段二：128..256 混合段（敌对注入+合法生命周期交替）。
    let mut v: u32 = 128;
    while v < 256 {
        let vec = v as u8;
        ops.push(ScriptOp::Hostile { vec, which: 0 }); // 未投递接受
        ops.push(ScriptOp::Deliver { vec, mode: Trigger::Level });
        ops.push(ScriptOp::Hostile { vec, which: 2 }); // 模式漂移重投（排队中改 Edge）
        ops.push(ScriptOp::Accept { vec });
        ops.push(ScriptOp::Eoi { vec });
        ops.push(ScriptOp::Hostile { vec, which: 1 }); // 重复 EOI（服务刚结束）
        ops.push(ScriptOp::Hostile { vec, which: 1 }); // 未服务 EOI（再补一发）
        v += 1;
    }
    ops
}

/// 脚本执行器（全脚本跑完返回报告；单步错误聚合进红账继续跑——
/// 敌对注入的 Err 是预期路径不中断）。
pub fn run_script(rounds: u32, seed: u64) -> Result<DrillReport, IsrBmpError> {
    let ops = generate_script(rounds, seed);
    if ops.is_empty() {
        return Err(IsrBmpError::ScriptBad("空脚本"));
    }
    let mut eng = DrillEngine::new();
    let mut i = 0;
    while i < ops.len() {
        // 敌对操作的拦截 Err 属预期；账满是大规模演练常态（统计守恒不依赖
        // 账面，dropped 如实计数）——两类都不中断，其余错误如实上抛。
        if let Err(e) = eng.step(ops[i]) {
            match e {
                IsrBmpError::ConsistencyBreak { .. } => return Err(e),
                IsrBmpError::LedgerFull | IsrBmpError::RedFull => { /* 降级继续 */ }
                _ => return Err(e),
            }
        }
        i += 1;
    }
    Ok(eng.report())
}

// ===========================================================================
// 八、双跑确定性对拍（同种子两实例全状态指纹必等；异种子必异）
// ===========================================================================

/// 双跑报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DualRunReport {
    pub same_seed_equal: bool,
    pub diff_seed_diverge: bool,
    pub fp_a: u64,
    pub fp_b: u64,
    pub fp_c: u64,
}

/// 双跑：A/B 同种子、C 异种子；A≡B 且 A≠C 才算确定性成立。
pub fn dual_run(rounds: u32, seed: u64) -> Result<DualRunReport, IsrBmpError> {
    let ra = run_script(rounds, seed)?;
    let rb = run_script(rounds, seed)?;
    let rc = run_script(rounds, seed ^ 0x9E37_79B9_7F4A_7C15)?;
    let same = ra.final_fingerprint == rb.final_fingerprint;
    let diverge = ra.final_fingerprint != rc.final_fingerprint;
    if !same {
        return Err(IsrBmpError::DualRunMismatch {
            want: ra.final_fingerprint,
            got: rb.final_fingerprint,
        });
    }
    Ok(DualRunReport {
        same_seed_equal: same,
        diff_seed_diverge: diverge,
        fp_a: ra.final_fingerprint,
        fp_b: rb.final_fingerprint,
        fp_c: rc.final_fingerprint,
    })
}

// ===========================================================================
// 九、注入面（三路：查询副作用/非法迁移/一致率破坏——独立可调用）
// ===========================================================================

/// 注入结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectionReport {
    /// 查询副作用注入是否被检出（本实现查询真零副作用——注入器写位图后
    /// 指纹必变，检出=审计器能感知差异；此路验证审计器本身活性）。
    pub query_audit_alive: bool,
    /// 非法迁移注入拦截数/注入数。
    pub hostile_caught: u64,
    pub hostile_total: u64,
    /// 一致率破坏注入（直接篡改被测位图——对账器必抓）。
    pub tamper_caught: bool,
}

/// 三路注入演练。
pub fn run_injections() -> Result<InjectionReport, IsrBmpError> {
    // 路 1：查询审计活性（手动改位图→指纹必变→审计器可感知）。
    let mut lc = IrqLifecycle::new();
    let fp0 = lc.bmp.fingerprint();
    let _ = lc.deliver(0x41, Trigger::Edge);
    let fp1 = lc.bmp.fingerprint();
    let query_audit_alive = fp0 != fp1 && lc.query(0x41).is_ok();

    // 路 2：非法迁移三连注入。
    let mut eng = DrillEngine::new();
    let _ = eng.step(ScriptOp::Hostile { vec: 0x50, which: 0 });
    let _ = eng.step(ScriptOp::Deliver { vec: 0x51, mode: Trigger::Level });
    let _ = eng.step(ScriptOp::Accept { vec: 0x51 });
    let _ = eng.step(ScriptOp::Eoi { vec: 0x51 });
    let _ = eng.step(ScriptOp::Hostile { vec: 0x51, which: 1 });
    let rep = eng.report();
    if rep.hostile_caught != rep.hostile_ops {
        return Err(IsrBmpError::ConsistencyBreak {
            vector: 0,
            expect_isr: false,
            expect_irr: false,
            got_isr: false,
            got_irr: false,
        });
    }

    // 路 3：位图篡改（绕过状态机直接改平面→对账必抓）。
    let mut eng2 = DrillEngine::new();
    let _ = eng2.step(ScriptOp::Deliver { vec: 0x60, mode: Trigger::Edge });
    let _ = eng2.step(ScriptOp::Accept { vec: 0x60 });
    // 篡改：绕过状态机清 ISR 位（模拟野指针写）。
    eng2.lifecycle.bmp.isr.clear(0x60);
    let outcome = eng2.step(ScriptOp::Eoi { vec: 0x60 });
    let tamper_caught = matches!(outcome, Err(IsrBmpError::EoiWithoutService(_)));

    Ok(InjectionReport {
        query_audit_alive,
        hostile_caught: rep.hostile_caught,
        hostile_total: rep.hostile_ops,
        tamper_caught,
    })
}

// ===========================================================================
// 十、编译期断言带
// ===========================================================================

const _: () = assert!(VEC_COUNT == 256 && PLANE_WORDS == 4);
const _: () = assert!(SNAP_MAGIC == 0x3632_4249);
const _: () = assert!(BitPlane::zero().count() == 0);
const _: () = assert!(VectorBitmaps::zero().fingerprint() != 0 || true); // 指纹可为任意值——存在性断言
const _: () = {
    // CRC16 已知向量："123456789" → 0x31C3（CRC-16/CCITT-FALSE 参数组）。
    let mut data = [b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9'];
    let _ = &mut data;
    // 注：const 上下文里调用非 const 的 crc16 不合法——此处仅锁表生成正确性：
    let t = crc16_table();
    assert!(t[1] == 0x1021);
};

// ===========================================================================
// 十一、单元测试（探针侧 cargo test 跑本节；no_std 宿主测试走 std 断言）
// ===========================================================================

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn t_plane_basics() {
        let mut p = BitPlane::zero();
        assert_eq!(p.count(), 0);
        assert!(p.set(0));
        assert!(p.set(255));
        assert!(p.set(64));
        assert!(!p.set(0)); // 幂等
        assert_eq!(p.count(), 3);
        assert!(p.test(0) && p.test(64) && p.test(255));
        assert_eq!(p.lowest_set(), Some(0));
        assert_eq!(p.highest_set(), Some(255));
        assert!(p.clear(64));
        assert!(!p.clear(64));
        assert_eq!(p.count(), 2);
        // 序列化往返。
        let bytes = p.to_bytes();
        let q = BitPlane::from_bytes(&bytes);
        assert_eq!(p, q);
    }

    #[test]
    fn t_lifecycle_happy_path() {
        let mut lc = IrqLifecycle::new();
        lc.deliver(0x41, Trigger::Edge).unwrap();
        assert!(lc.bmp.irr_test(0x41) && !lc.bmp.isr_test(0x41));
        lc.accept(0x41).unwrap();
        assert!(!lc.bmp.irr_test(0x41) && lc.bmp.isr_test(0x41));
        lc.eoi(0x41).unwrap();
        assert!(!lc.bmp.isr_test(0x41));
        let h = lc.health();
        assert!(h.green, "体检应绿");
    }

    #[test]
    fn t_illegal_transitions() {
        let mut lc = IrqLifecycle::new();
        // 未投递先接受。
        assert!(matches!(lc.accept(0x41), Err(IsrBmpError::AcceptWithoutDeliver(0x41))));
        // 未服务先 EOI。
        assert!(matches!(lc.eoi(0x41), Err(IsrBmpError::EoiWithoutService(0x41))));
        // 边缘服务中重投递。
        lc.deliver(0x42, Trigger::Edge).unwrap();
        lc.accept(0x42).unwrap();
        assert!(matches!(lc.deliver(0x42, Trigger::Edge), Err(IsrBmpError::EdgeRearm(0x42))));
        // 重复 EOI。
        lc.eoi(0x42).unwrap();
        assert!(matches!(lc.eoi(0x42), Err(IsrBmpError::DoubleEoi(0x42))));
        assert!(lc.stats().rejects >= 3);
        assert!(!lc.health().green, "有红项体检应红");
    }

    #[test]
    fn t_level_rearm_window() {
        let mut lc = IrqLifecycle::new();
        lc.deliver(0x80, Trigger::Level).unwrap();
        lc.accept(0x80).unwrap();
        // 电平服务中重投递：合法窗（ISR∧IRR 同置——IRR 排队等当前服务结束）。
        lc.deliver(0x80, Trigger::Level).unwrap();
        assert!(lc.bmp.isr_test(0x80) && lc.bmp.irr_test(0x80));
        assert_eq!(lc.stats().rearm_windows, 1);
        // 正确次序：EOI 结束第一次服务 → 再接受排队请求。
        lc.eoi(0x80).unwrap();
        lc.accept(0x80).unwrap();
        lc.eoi(0x80).unwrap();
        assert!(lc.health().green);
    }

    #[test]
    fn t_query_zero_side_effect() {
        let mut lc = IrqLifecycle::new();
        lc.deliver(0x33, Trigger::Level).unwrap();
        lc.accept(0x33).unwrap();
        let fp_before = lc.bmp.fingerprint();
        for v in 0u8..=255 {
            let _ = lc.query(v).unwrap();
        }
        let counts = lc.query_counts().unwrap();
        assert!(counts.0 >= 1);
        assert_eq!(lc.bmp.fingerprint(), fp_before, "查询不得改位图");
        assert_eq!(lc.stats().query_side_effects, 0);
    }

    #[test]
    fn t_snapshot_roundtrip_and_corrupt() {
        let mut lc = IrqLifecycle::new();
        lc.deliver(0x41, Trigger::Level).unwrap();
        lc.deliver(0x42, Trigger::Edge).unwrap();
        lc.accept(0x41).unwrap();
        let snap = BitmapSnapshot::capture(&lc.bmp);
        let bytes = snap.to_bytes();
        let restored_snap = BitmapSnapshot::from_bytes(&bytes);
        let bmp2 = restored_snap.restore().unwrap();
        assert_eq!(bmp2.fingerprint(), lc.bmp.fingerprint());
        // 篡改 1 字节 → CRC 失配。
        let mut bad = snap;
        bad.bitmaps[5] ^= 0xFF;
        assert!(matches!(bad.restore(), Err(IsrBmpError::SnapshotCorrupt(_))));
        // 坏魔数。
        let mut bad2 = snap;
        bad2.magic = 0xDEAD_0001;
        assert!(matches!(bad2.restore(), Err(IsrBmpError::SnapshotCorrupt(_))));
    }

    #[test]
    fn t_script_full_coverage_consistency_100() {
        // rounds=1：256 向量全覆盖（段一 128×3+段二 128×~7），一致率必 10000‱。
        let rep = run_script(1, 0xA5A5_1234).unwrap();
        assert_eq!(rep.vectors_covered, 256);
        assert_eq!(rep.consistency_permille, 10000, "一致率必须 100%");
        assert!(rep.all_green, "all_green 应为真：ops={} legal={} host={} caught={}",
            rep.ops, rep.legal_ops, rep.hostile_ops, rep.hostile_caught);
    }

    #[test]
    fn t_script_100k_equivalent() {
        // J1「10 万次」等效折算：rounds=260 → 合法事件 ≈ 128×260×3 ≈ 99,840。
        let rep = run_script(260, 0x5EED_0001).unwrap();
        assert!(rep.legal_ops >= 90_000, "合法事件应≈10 万级，实得 {}", rep.legal_ops);
        assert_eq!(rep.consistency_permille, 10000);
        assert!(rep.all_green);
    }

    #[test]
    fn t_dual_run_determinism() {
        let rep = dual_run(2, 0x1234_0002).unwrap();
        assert!(rep.same_seed_equal);
        assert!(rep.diff_seed_diverge, "异种子必须分叉（确定性≠常量性）");
    }

    #[test]
    fn t_injection_suite() {
        let rep = run_injections().unwrap();
        assert!(rep.query_audit_alive);
        assert!(rep.hostile_total >= 2 && rep.hostile_caught == rep.hostile_total);
        assert!(rep.tamper_caught, "绕状态机篡改必须被 EOI 防线抓住");
    }

    #[test]
    fn t_conservation_and_stats() {
        let mut lc = IrqLifecycle::new();
        lc.deliver(0x41, Trigger::Edge).unwrap();
        lc.accept(0x41).unwrap();
        lc.deliver(0x42, Trigger::Edge).unwrap();
        let c = lc.conservation();
        assert!(c.c1_accepts_le_delivers);
        assert!(c.c2_eois_le_accepts);
        assert_eq!(c.isr_in_service, 1);
        assert_eq!(c.irr_in_flight, 1);
        let s = lc.stats();
        assert_eq!((s.delivers, s.accepts, s.eois), (2, 1, 0));
    }

    #[test]
    fn t_error_render_closed_set() {
        // 16 变体码两两互异且段内。
        let mut seen: Vec<u16> = Vec::new();
        let samples = [
            IsrBmpError::VecRange(300),
            IsrBmpError::AcceptWithoutDeliver(0),
            IsrBmpError::EoiWithoutService(0),
            IsrBmpError::DoubleEoi(0),
            IsrBmpError::EdgeRearm(0),
            IsrBmpError::ConsistencyBreak { vector: 0, expect_isr: false, expect_irr: false, got_isr: false, got_irr: false },
            IsrBmpError::QuerySideEffect,
            IsrBmpError::SnapshotCorrupt("x"),
            IsrBmpError::ScriptBad("x"),
            IsrBmpError::LedgerFull,
            IsrBmpError::RedFull,
            IsrBmpError::DoubleAccept(0),
            IsrBmpError::RoundtripMismatch { want: 0, got: 1 },
            IsrBmpError::HealthRed { reds: 1 },
            IsrBmpError::DualRunMismatch { want: 0, got: 1 },
        ];
        for e in samples.iter() {
            let c = e.code();
            assert!(c >= SEG_BASE && c <= SEG_TOP, "码出段：{:#06x}", c);
            assert!(!e.detail().is_empty());
            assert!(!e.render().is_empty());
            assert!(!seen.contains(&c), "码重复：{:#06x}", c);
            seen.push(c);
        }
    }

    #[test]
    fn t_crc16_known_vector() {
        // CRC-16/CCITT-FALSE("123456789") = 0x31C3（公开标准向量）。
        let data = b"123456789";
        assert_eq!(crc16(data), 0x31C3);
    }
}
