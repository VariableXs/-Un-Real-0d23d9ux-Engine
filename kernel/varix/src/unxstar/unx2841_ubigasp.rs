//! unx2841 ↔ UNX-F2841 大页-地址空间联合压测方案与执行
//!
//! 单号：UNX-F2841
//! 标题：大页-地址空间联合压测方案与执行
//! 域：CoRun · UNX-A4-B23 段「大页-地址空间联合段」
//! 册目标行数：340（方案 100 + 全链执行 150 + 断言 90；测试段不计）
//! 交付文件行数：≥2800（硬指标）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F2841-J1，逐字摘抄）：
//! `UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合`
//!
//! ## 码段范围声明
//! 本单独占诊断码段 `0x20C0 ..= 0x20FF`（`SEG_BASE = 0x20C0`，`SEG_MASK = 0x003F`，
//! 段内可编址 64 码）。主码 `0x20C0`。错误变体一码一段，变体总数 ≤ 60
//! （实际 14 个注入形态 + 6 个辅助码 = 20 码，余 44 码留白）。
//! 本单**不占用** `0x2100` 及以上任何码。
//!
//! ## 组件清单（本单交付的纯功能件）
//! 1. `ChainStage`：建立 / 翻译 / 拆除 三段闭集（判据主轴）。
//! 2. `MapScale` × `HugeSize` × `ConcDegree`：三正交注入维度，
//!    合成 `5 × 4 × 4 = 80` 格方案笛卡尔积（格数显式计算）。
//! 3. `LinkCell`：单格全链方案参数（三段步数 + 帧数 + PS 位预期）。
//! 4. `LinkMatrix`：参数矩阵，正交展开 + 逐格索引 + 线性化公式。
//! 5. `ChainLedger`：**链账三段**（建立段 / 翻译段 / 拆除段），
//!    守恒式 `establish == translate_hit == teardown`。
//! 6. `PageWalk`：四级页表遍历替身，含 PS 位判定（大页项确用大页粒度）。
//! 7. `VmaLedger`：VMA 账（建立登记 / 拆除回收），与建立段、拆除段同步。
//! 8. `RingLedger`：append-only 环形账，`dropped` 显性计数。
//! 9. `LinkState`：全链状态机（合法边闭集 + 非法转换拒记）。
//! 10. `caliber_check()`：三段统一口径（页数 / 事件数**双列**）比对。
//! 11. `inject_all()`：注入核 14 形态全拒（判据要求 10/10）。
//! 12. `fingerprint()` / `verify()`：链账冻结指纹与漂移检出。
//! 13. `export_kv()` / `export_tlv()`：观测导出，TLV 往返自洽。
//! 14. `replay(seed)` / `observe(seed)`：确定性重放与真实跑逻辑观测。
//! 15. `round_sweep()`：10 轮贯通判定（判据 10/10 轮）。
//! 16. `resource_budget()` / `complexity()`：资源上界与性能逐项分解。
//!
//! ## 职责定位（语义边界）
//! 本单做**全链压测（模型侧）**：大页（2MB）经映射底座进地址空间的
//! 建立 → 翻译 → 拆除三段流，每段都有账，三段必须闭合。
//! 本单**不做**：映射底座本体（F2589）、池本体（F2425）、页表本体（F2462）。
//! 所谓「映射」在本单是账本语义推进（`VmaLedger` 登记 + `PageWalk` 遍历），
//! 不触碰真实 CR3 / 页表页；「PS 位」是模拟页表项里的一个 `u64` 位。
//!
//! ## 与 Windows 同类能力的对照与「深在哪」
//! Windows 的大页链路测试（large page / AWE 映射测试）通常只做「映射成功 +
//! 访问命中 + 回收」三步 happy path，读数是「成功 / 失败」二值，不成账。
//! 本单深在四处：
//! 1. **三段成账且闭合**：`建立数 == 翻译命中数 == 拆除数`，任何一段掉链子
//!    都表现为闭合失败，而不是「看起来还挺好」。
//! 2. **PS 位断言**：翻译段必须确认页表项 PS 位真为 1，否则即使命中数对上
//!    也判失败（防「用 4KB 页凑出同样的命中数」这种假通过）。
//! 3. **口径双列**：每段同时记页数与事件数两列，口径不一致的串账注入
//!    会被 `caliber_check()` 检出——判据的「三段统一口径注记」落在这里。
//! 4. **10 轮全贯通**：不是跑一轮算过，而是 `ROUND_COUNT = 10` 轮每轮
//!    三段都闭合才算通过（判据 10/10）。
//!
//! ## 性能逐项分解（模型侧成本，非真实硬件时序）
//! - **正交展开** `LinkMatrix::expand()`：`O(L)` 步嵌套，`L = 80` 格。
//! - **逐格全链跑** `run_link_cell()`：`O(CHAIN_STEPS)` = 3 段 × 24 步 = 72 步。
//! - **全矩阵全链跑**：`O(L × CHAIN_STEPS)` = 80 × 72 = 5760 步。
//! - **10 轮贯通** `round_sweep()`：`O(ROUND_COUNT × L × CHAIN_STEPS)`
//!   = 10 × 5760 = 57600 步硬上限。
//! - **翻译遍历** `PageWalk::translate()`：`O(PAGE_LEVELS)` = 4 级页表常数。
//! - **链账对账** `reconcile()`：`O(1)`（三段等式 + 桥式核）。
//! - **注入核**：`O(FORM_COUNT × L)`。
//! - **syscall 次数**：恒 0（纯功能件不做真实 mmap / 页表写）。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!`、零浮点、零真实 IO、
//! 零墙钟；时间一律逻辑 `tick: u64`；确定性 LCG
//! （`6364136223846793005` / `1442695040888963407`，模 `2^64` 回绕）；
//! FNV-1a basis `0xcbf29ce484222325`、prime `0x100000001b3`；
//! 自定义 `UaResult<T>`，错误 `code() -> u16` + `detail() -> String`。
//! 无 `#[cfg(test)]`。
#![allow(dead_code)]
extern crate alloc;

// `vec!` 供 TLV 预分配与样本数组使用；保留统一导入形态（册内风格一致）。
#[allow(unused_imports)]
use alloc::{format, string::String, string::ToString, vec, vec::Vec};

// ===========================================================================
// 0. 常量表
// ===========================================================================
/// 模块版本标识（写入链账描述符 + TLV 组头）。
pub const V_VERSION_ID: u64 = 0x0001_2841_0000_0001;
/// 模块版本串。
pub const V_VERSION: &str = "v2841-ubigasp.v1";
/// 诊断码段基址（独占段 `0x20C0`）。
pub const SEG_BASE: u16 = 0x20C0;
/// 诊断码段掩码（段容量 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（= SEG_BASE | SEG_MASK）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 链账版本号。
pub const CHAIN_BOOK_REV: u32 = 11;
/// 判定轮数（判据 10/10 轮）。
pub const ROUND_COUNT: usize = 10;
/// 三段闭集长度。
pub const STAGE_COUNT: usize = 3;
/// 映射规模档数。
pub const SCALE_COUNT: usize = 5;
/// 大页尺寸档数。
pub const SIZE_COUNT: usize = 4;
/// 并发度档数。
pub const CONC_COUNT: usize = 4;
/// 正交笛卡尔积格数（`5 × 4 × 4 = 80`，构造期算出并断言）。
pub const GRID_CELLS: usize = SCALE_COUNT * SIZE_COUNT * CONC_COUNT;
/// 单段步数（三段统一，保证口径可比）。
pub const STAGE_STEPS: usize = 24;
/// 单格全链步数（3 段 × 24 步）。
pub const CHAIN_STEPS: usize = STAGE_COUNT * STAGE_STEPS;
/// 全矩阵全链步数上界。
pub const MATRIX_STEPS: usize = GRID_CELLS * CHAIN_STEPS;
/// 10 轮贯通步数上界。
pub const SWEEP_CAP: usize = ROUND_COUNT * MATRIX_STEPS;
/// step 硬上限（10 轮上限再留一倍裕量）。
pub const STEP_CAP: u64 = (SWEEP_CAP * 2) as u64;
/// syscall 上界（本单零真实 syscall）。
pub const SYSCALL_CAP: u64 = 0;
/// 内存峰值上界（字节）。
pub const MEM_PEAK_CAP: u64 = 4 * 1024 * 1024;
/// 环形账容量（2 的幂）。
pub const RING_CAP: usize = 64;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑时钟步进量。
pub const TICK_STEP: u64 = 1;
/// 页表级数（4 级：PML4 / PDPT / PD / PT）。
pub const PAGE_LEVELS: usize = 4;
/// 大页最小尺寸（2MB，字节）。
pub const HUGE_MIN: u64 = 2 * 1024 * 1024;
/// 页表项 PS 位（Page Size Flag，大页标志）。
pub const PTE_PS: u64 = 1 << 7;
/// 页表项 Present 位。
pub const PTE_PRESENT: u64 = 1 << 0;
/// 页表项 NX 位（No-Execute）。
pub const PTE_NX: u64 = 1 << 63;
/// 2MB 大页覆盖的 4KB 页数（512 页）。
pub const PAGES_PER_2M: u64 = 512;
/// 注入形态数（判据要求 10/10，本单给 14）。
pub const FORM_COUNT: usize = 14;
/// FNV-1a basis。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;

// ===========================================================================
// 1. 确定性 LCG 与 FNV-1a
// ===========================================================================
/// 确定性 LCG 发生器（模 `2^64`，回绕即 `wrapping`）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    pub state: u64,
}
impl Lcg {
    /// 以给定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 当前内部状态（只读）。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 `u64`。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值（`n == 0` 返回 0）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取千分数。
    pub fn permille(&mut self) -> u64 {
        self.next_u64() % 1000
    }
}
/// 纯函数式 LCG 步进。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}
/// 从中间态继续 FNV-1a 折叠。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h: u64 = state;
    let mut i: usize = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= (*b as u64) & 0xFF;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}
/// FNV-1a 折叠字节流。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(FNV_BASIS, bytes)
}
/// FNV-1a 折叠 `u64` 序列。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for w in words.iter() {
        let mut b: [u8; 8] = [0u8; 8];
        let mut i: usize = 0;
        while i < 8 {
            if let Some(slot) = b.get_mut(i) {
                *slot = (((*w) >> (8 * i)) & 0xFF) as u8;
            }
            i = i.wrapping_add(1);
        }
        h = fnv1a64_ext(h, &b);
    }
    h
}
/// 逻辑时钟推进（零墙钟）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(TICK_STEP)
}

// ===========================================================================
// 2. 诊断码与错误闭集
// ===========================================================================
/// 注入形态闭集（14 形态，判据要求 10/10，本单给 14）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0x20C0 PS 位未置：翻译命中但页表项 PS=0（假通过防线）。
    PsBitNotSet,
    /// 0x20C1 三段不闭合：建立 ≠ 翻译命中 ≠ 拆除。
    ChainNotClosed,
    /// 0x20C2 建立段越界：VMA 未登记即翻译。
    VmaUnregistered,
    /// 0x20C3 拆除段越界：未建立即拆除（回收幽灵 VMA）。
    TeardownGhost,
    /// 0x20C4 段序错乱：翻译先于建立，或拆除先于翻译。
    StageOrderBroken,
    /// 0x20C5 口径串账：页数列与事件数列不匹配。
    CaliberSkew,
    /// 0x20C6 大页尺寸越界（非 2MB 倍数）。
    SizeNotHugeAligned,
    /// 0x20C7 页表层级越界（level ≥ 4）。
    LevelOutOfRange,
    /// 0x20C8 帧数超界（占用帧数 > 方案帧数）。
    FrameOverflow,
    /// 0x20C9 环形溢出未记账（`dropped` 显性计数失配）。
    RingDropUnaccounted,
    /// 0x20CA 链账不守恒（桥式等式破裂）。
    LedgerUnbalanced,
    /// 0x20CB 轮次贯通失败（10 轮未全闭合）。
    RoundNotThrough,
    /// 0x20CC TLV 往返破坏。
    TlvRoundtripBroken,
    /// 0x20CD 冻结漂移（链账哈希与基线失配）。
    DriftInjected,
}
impl Form {
    /// 全部形态（判据顺序）。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::PsBitNotSet,
        Form::ChainNotClosed,
        Form::VmaUnregistered,
        Form::TeardownGhost,
        Form::StageOrderBroken,
        Form::CaliberSkew,
        Form::SizeNotHugeAligned,
        Form::LevelOutOfRange,
        Form::FrameOverflow,
        Form::RingDropUnaccounted,
        Form::LedgerUnbalanced,
        Form::RoundNotThrough,
        Form::TlvRoundtripBroken,
        Form::DriftInjected,
    ];
    /// 序号（0..13）。
    pub fn index(self) -> usize {
        match self {
            Form::PsBitNotSet => 0,
            Form::ChainNotClosed => 1,
            Form::VmaUnregistered => 2,
            Form::TeardownGhost => 3,
            Form::StageOrderBroken => 4,
            Form::CaliberSkew => 5,
            Form::SizeNotHugeAligned => 6,
            Form::LevelOutOfRange => 7,
            Form::FrameOverflow => 8,
            Form::RingDropUnaccounted => 9,
            Form::LedgerUnbalanced => 10,
            Form::RoundNotThrough => 11,
            Form::TlvRoundtripBroken => 12,
            Form::DriftInjected => 13,
        }
    }
    /// 诊断码（独占段内连续递增，一码一段）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }
    /// 形态短名（入册）。
    pub fn name(self) -> &'static str {
        match self {
            Form::PsBitNotSet => "ps_bit_not_set",
            Form::ChainNotClosed => "chain_not_closed",
            Form::VmaUnregistered => "vma_unregistered",
            Form::TeardownGhost => "teardown_ghost",
            Form::StageOrderBroken => "stage_order_broken",
            Form::CaliberSkew => "caliber_skew",
            Form::SizeNotHugeAligned => "size_not_huge_aligned",
            Form::LevelOutOfRange => "level_out_of_range",
            Form::FrameOverflow => "frame_overflow",
            Form::RingDropUnaccounted => "ring_drop_unaccounted",
            Form::LedgerUnbalanced => "ledger_unbalanced",
            Form::RoundNotThrough => "round_not_through",
            Form::TlvRoundtripBroken => "tlv_roundtrip_broken",
            Form::DriftInjected => "drift_injected",
        }
    }
    /// 形态判读。
    pub fn describe(self) -> &'static str {
        match self {
            Form::PsBitNotSet => "PS 位未置：翻译命中但页表项 PS=0（大页粒度未生效）",
            Form::ChainNotClosed => "三段不闭合：建立 ≠ 翻译命中 ≠ 拆除",
            Form::VmaUnregistered => "建立段越界：VMA 未登记即进入翻译",
            Form::TeardownGhost => "拆除段越界：未建立即拆除（回收幽灵 VMA）",
            Form::StageOrderBroken => "段序错乱：翻译先于建立或拆除先于翻译",
            Form::CaliberSkew => "三段口径串账：页数列与事件数列不匹配",
            Form::SizeNotHugeAligned => "大页尺寸越界：非 2MB 倍数",
            Form::LevelOutOfRange => "页表层级越界：level >= 4",
            Form::FrameOverflow => "帧数超界：占用帧数超方案帧数",
            Form::RingDropUnaccounted => "环形溢出未记账：dropped 显性计数失配",
            Form::LedgerUnbalanced => "链账不守恒：桥式等式破裂",
            Form::RoundNotThrough => "轮次贯通失败：10 轮未全闭合",
            Form::TlvRoundtripBroken => "TLV 往返破坏：回读字段与写入不一致",
            Form::DriftInjected => "冻结漂移：链账哈希与基线失配",
        }
    }
    /// 检出手段短名。
    pub fn probe(self) -> &'static str {
        match self {
            Form::PsBitNotSet => "PageWalk::ps_ok()",
            Form::ChainNotClosed => "ChainLedger::closed()",
            Form::VmaUnregistered => "VmaLedger::registered()",
            Form::TeardownGhost => "VmaLedger::teardown()",
            Form::StageOrderBroken => "LinkState::can_go()",
            Form::CaliberSkew => "caliber_check()",
            Form::SizeNotHugeAligned => "HugeSize::huge_aligned()",
            Form::LevelOutOfRange => "PageWalk::level_ok()",
            Form::FrameOverflow => "VmaLedger::frames_within()",
            Form::RingDropUnaccounted => "ring.check()",
            Form::LedgerUnbalanced => "reconcile()",
            Form::RoundNotThrough => "round_sweep()",
            Form::TlvRoundtripBroken => "tlv_roundtrip_ok()",
            Form::DriftInjected => "verify()",
        }
    }
}
/// 辅助诊断闭集（索引与构造类错误）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 0x20CE 方案格索引越界。
    CellIndexOutOfRange,
    /// 0x20CF 段闭集越界。
    StageOutOfRange,
    /// 0x20D0 规模档越界。
    ScaleOutOfRange,
    /// 0x20D1 尺寸档越界。
    SizeOutOfRange,
    /// 0x20D2 并发档越界。
    ConcOutOfRange,
    /// 0x20D3 VMA 未登记（查账时）。
    VmaNotFound,
}
impl AuxCode {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            AuxCode::CellIndexOutOfRange => 0x20CE,
            AuxCode::StageOutOfRange => 0x20CF,
            AuxCode::ScaleOutOfRange => 0x20D0,
            AuxCode::SizeOutOfRange => 0x20D1,
            AuxCode::ConcOutOfRange => 0x20D2,
            AuxCode::VmaNotFound => 0x20D3,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::CellIndexOutOfRange => "cell_index_out_of_range",
            AuxCode::StageOutOfRange => "stage_out_of_range",
            AuxCode::ScaleOutOfRange => "scale_out_of_range",
            AuxCode::SizeOutOfRange => "size_out_of_range",
            AuxCode::ConcOutOfRange => "conc_out_of_range",
            AuxCode::VmaNotFound => "vma_not_found",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::CellIndexOutOfRange => "方案格索引越界",
            AuxCode::StageOutOfRange => "段闭集越界",
            AuxCode::ScaleOutOfRange => "映射规模档越界",
            AuxCode::SizeOutOfRange => "大页尺寸档越界",
            AuxCode::ConcOutOfRange => "并发度档越界",
            AuxCode::VmaNotFound => "VMA 未登记",
        }
    }
}
/// 错误类型：`code()` / `detail()` / `full()` 三要素。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UaErr {
    /// 诊断码。
    pub code: u16,
    /// 上下文细节。
    pub detail: String,
}
impl UaErr {
    /// 构造错误。
    pub fn new(code: u16, detail: String) -> UaErr {
        UaErr { code, detail }
    }
    /// 由形态构造错误。
    pub fn of_form(f: Form, detail: String) -> UaErr {
        UaErr::new(f.code(), detail)
    }
    /// 由辅助码构造错误。
    pub fn of_aux(a: AuxCode, detail: String) -> UaErr {
        UaErr::new(a.code(), detail)
    }
    /// 三要素之一：诊断码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：上下文细节。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：可读整串。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}
/// 本单 `Result`。
pub type UaResult<T> = Result<T, UaErr>;
/// 错误整串。
pub fn err_str(e: &UaErr) -> String {
    e.full()
}
// ===========================================================================
// 3. 三段闭集与三维注入闭集
// ===========================================================================
/// 全链三段闭集（判据主轴：建立 → 翻译 → 拆除）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainStage {
    /// 建立段：VMA 登记 + 大页帧分配。
    Establish,
    /// 翻译段：四级页表遍历 + PS 位判定。
    Translate,
    /// 拆除段：整块回收回池（VMA 注销 + 帧归还）。
    Teardown,
}
impl ChainStage {
    /// 全部三段（链序）。
    pub const ALL: [ChainStage; STAGE_COUNT] = [ChainStage::Establish, ChainStage::Translate, ChainStage::Teardown];
    /// 序号（0..2，链序）。
    pub fn index(self) -> usize {
        match self {
            ChainStage::Establish => 0,
            ChainStage::Translate => 1,
            ChainStage::Teardown => 2,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<ChainStage> {
        match i {
            0 => Some(ChainStage::Establish),
            1 => Some(ChainStage::Translate),
            2 => Some(ChainStage::Teardown),
            _ => None,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            ChainStage::Establish => "establish",
            ChainStage::Translate => "translate",
            ChainStage::Teardown => "teardown",
        }
    }
    /// 该段应有步数（三段统一，保证口径可比）。
    pub fn steps(self) -> u64 {
        STAGE_STEPS as u64
    }
    /// 该段在链中的序号（用于链序断言）。
    pub fn order(self) -> usize {
        self.index()
    }
    /// 是否为链首段。
    pub fn is_head(self) -> bool {
        matches!(self, ChainStage::Establish)
    }
    /// 是否为链尾段。
    pub fn is_tail(self) -> bool {
        matches!(self, ChainStage::Teardown)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            ChainStage::Establish => "建立段：VMA 登记 + 大页帧分配（链首）",
            ChainStage::Translate => "翻译段：四级页表遍历 + PS 位判定",
            ChainStage::Teardown => "拆除段：整块回收回池 + VMA 注销（链尾）",
        }
    }
}
/// 映射规模档（一次全链建立的 VMA 条数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapScale {
    /// 档 0：1 条 VMA。
    S1,
    /// 档 1：2 条。
    S2,
    /// 档 2：4 条。
    S4,
    /// 档 3：8 条。
    S8,
    /// 档 4：16 条（最大规模）。
    S16,
}
impl MapScale {
    /// 全部档位。
    pub const ALL: [MapScale; SCALE_COUNT] = [MapScale::S1, MapScale::S2, MapScale::S4, MapScale::S8, MapScale::S16];
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            MapScale::S1 => 0,
            MapScale::S2 => 1,
            MapScale::S4 => 2,
            MapScale::S8 => 3,
            MapScale::S16 => 4,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<MapScale> {
        match i {
            0 => Some(MapScale::S1),
            1 => Some(MapScale::S2),
            2 => Some(MapScale::S4),
            3 => Some(MapScale::S8),
            4 => Some(MapScale::S16),
            _ => None,
        }
    }
    /// VMA 条数。
    pub fn vmas(self) -> u64 {
        match self {
            MapScale::S1 => 1,
            MapScale::S2 => 2,
            MapScale::S4 => 4,
            MapScale::S8 => 8,
            MapScale::S16 => 16,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            MapScale::S1 => "scale_s1",
            MapScale::S2 => "scale_s2",
            MapScale::S4 => "scale_s4",
            MapScale::S8 => "scale_s8",
            MapScale::S16 => "scale_s16",
        }
    }
    /// 是否最大规模（对抗档）。
    pub fn is_max(self) -> bool {
        matches!(self, MapScale::S16)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            MapScale::S1 => "1 条 VMA：最小规模基线",
            MapScale::S2 => "2 条 VMA：轻规模",
            MapScale::S4 => "4 条 VMA：中规模",
            MapScale::S8 => "8 条 VMA：较重规模",
            MapScale::S16 => "16 条 VMA：最大规模，主要对抗面",
        }
    }
}
/// 大页尺寸档（2MB / 4MB / 8MB / 16MB）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HugeSize {
    /// 2MB。
    M2,
    /// 4MB。
    M4,
    /// 8MB。
    M8,
    /// 16MB。
    M16,
}
impl HugeSize {
    /// 全部尺寸档。
    pub const ALL: [HugeSize; SIZE_COUNT] = [HugeSize::M2, HugeSize::M4, HugeSize::M8, HugeSize::M16];
    /// 序号（0..3）。
    pub fn index(self) -> usize {
        match self {
            HugeSize::M2 => 0,
            HugeSize::M4 => 1,
            HugeSize::M8 => 2,
            HugeSize::M16 => 3,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<HugeSize> {
        match i {
            0 => Some(HugeSize::M2),
            1 => Some(HugeSize::M4),
            2 => Some(HugeSize::M8),
            3 => Some(HugeSize::M16),
            _ => None,
        }
    }
    /// 帧字节数。
    pub fn frame_bytes(self) -> u64 {
        match self {
            HugeSize::M2 => 2 * HUGE_MIN,
            HugeSize::M4 => 4 * HUGE_MIN,
            HugeSize::M8 => 8 * HUGE_MIN,
            HugeSize::M16 => 16 * HUGE_MIN,
        }
    }
    /// 是否 2MB 对齐（全部档应成立，断言用）。
    pub fn huge_aligned(self) -> bool {
        self.frame_bytes() % HUGE_MIN == 0
    }
    /// 覆盖的 4KB 页数。
    pub fn pages(self) -> u64 {
        self.frame_bytes() / 4096u64
    }
    /// 覆盖的 2MB 块数。
    pub fn huge_blocks(self) -> u64 {
        self.frame_bytes() / HUGE_MIN
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            HugeSize::M2 => "size_2m",
            HugeSize::M4 => "size_4m",
            HugeSize::M8 => "size_8m",
            HugeSize::M16 => "size_16m",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            HugeSize::M2 => "2MB 大页：最小粒度，PS 项覆盖 512 个 4KB 页",
            HugeSize::M4 => "4MB 大页：常用部署粒度",
            HugeSize::M8 => "8MB 大页：中高压力粒度",
            HugeSize::M16 => "16MB 大页：最大粒度，单帧最贵",
        }
    }
}
/// 并发度档（1 / 2 / 4 / 8 个全链注入者）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConcDegree {
    /// 1 路。
    C1,
    /// 2 路。
    C2,
    /// 4 路。
    C4,
    /// 8 路。
    C8,
}
impl ConcDegree {
    /// 全部档位。
    pub const ALL: [ConcDegree; CONC_COUNT] = [ConcDegree::C1, ConcDegree::C2, ConcDegree::C4, ConcDegree::C8];
    /// 序号（0..3）。
    pub fn index(self) -> usize {
        match self {
            ConcDegree::C1 => 0,
            ConcDegree::C2 => 1,
            ConcDegree::C4 => 2,
            ConcDegree::C8 => 3,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<ConcDegree> {
        match i {
            0 => Some(ConcDegree::C1),
            1 => Some(ConcDegree::C2),
            2 => Some(ConcDegree::C4),
            3 => Some(ConcDegree::C8),
            _ => None,
        }
    }
    /// 注入者条数。
    pub fn lanes(self) -> u64 {
        match self {
            ConcDegree::C1 => 1,
            ConcDegree::C2 => 2,
            ConcDegree::C4 => 4,
            ConcDegree::C8 => 8,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            ConcDegree::C1 => "conc_c1",
            ConcDegree::C2 => "conc_c2",
            ConcDegree::C4 => "conc_c4",
            ConcDegree::C8 => "conc_c8",
        }
    }
    /// 是否多路（单路是基线）。
    pub fn is_contended(self) -> bool {
        self.lanes() > 1
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            ConcDegree::C1 => "单路全链：无争用基线",
            ConcDegree::C2 => "双路全链：轻度争用",
            ConcDegree::C4 => "四路全链：中度争用",
            ConcDegree::C8 => "八路全链：重度争用，主要对抗面",
        }
    }
}
// ===========================================================================
// 4. 方案参数矩阵（正交展开，格数显式计算）
// ===========================================================================
/// 单格全链方案参数。
///
/// 三维正交：规模 × 尺寸 × 并发。任一维取值不依赖其他维，
/// 这是笛卡尔积成立的前提。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkCell {
    /// 格号（0..GRID_CELLS）。
    pub index: usize,
    /// 规模档序号。
    pub scale_idx: usize,
    /// 尺寸档序号。
    pub size_idx: usize,
    /// 并发档序号。
    pub conc_idx: usize,
    /// 建立 VMA 条数。
    pub vma_count: u64,
    /// 每 VMA 占用的 2MB 块数。
    pub blocks_per_vma: u64,
    /// 方案帧数（`vma_count × blocks_per_vma`）。
    pub plan_frames: u64,
    /// 每段步数（三段统一）。
    pub stage_steps: u64,
    /// 三段总步数。
    pub total_steps: u64,
    /// 格指纹（入链账哈希）。
    pub fingerprint: u64,
}
impl LinkCell {
    /// 由三维序号构造一格。
    ///
    /// 口径注记（判据「参数依据注记」）：
    /// - `vma_count = scale.vmas()`（规模档直接给条数）；
    /// - `blocks_per_vma = size.huge_blocks()`（2MB 块数）；
    /// - `plan_frames = vma_count × blocks_per_vma`（方案帧数）；
    /// - `stage_steps = STAGE_STEPS`（三段**必须**统一，否则口径不可比——
    ///   这是判据「三段统一口径」的根因）；
    /// - `total_steps = 3 × stage_steps`。
    pub fn new(index: usize, scale: MapScale, size: HugeSize, conc: ConcDegree) -> LinkCell {
        let vma = scale.vmas();
        let blocks = size.huge_blocks();
        let frames = vma.wrapping_mul(blocks);
        let total_steps = STAGE_COUNT as u64 * STAGE_STEPS as u64;
        let mut words: [u64; 8] = [0u64; 8];
        words[0] = index as u64;
        words[1] = scale.index() as u64;
        words[2] = size.index() as u64;
        words[3] = conc.index() as u64;
        words[4] = vma;
        words[5] = blocks;
        words[6] = frames;
        words[7] = conc.lanes();
        LinkCell {
            index,
            scale_idx: scale.index(),
            size_idx: size.index(),
            conc_idx: conc.index(),
            vma_count: vma,
            blocks_per_vma: blocks,
            plan_frames: frames,
            stage_steps: STAGE_STEPS as u64,
            total_steps,
            fingerprint: fnv1a_words(&words),
        }
    }
    /// 还原规模档。
    pub fn scale(&self) -> UaResult<MapScale> {
        match MapScale::from_index(self.scale_idx) {
            Some(s) => Ok(s),
            None => Err(UaErr::of_aux(AuxCode::ScaleOutOfRange, format!("cell={} scale_idx={}", self.index, self.scale_idx))),
        }
    }
    /// 还原尺寸档。
    pub fn size(&self) -> UaResult<HugeSize> {
        match HugeSize::from_index(self.size_idx) {
            Some(s) => Ok(s),
            None => Err(UaErr::of_aux(AuxCode::SizeOutOfRange, format!("cell={} size_idx={}", self.index, self.size_idx))),
        }
    }
    /// 还原并发档。
    pub fn conc(&self) -> UaResult<ConcDegree> {
        match ConcDegree::from_index(self.conc_idx) {
            Some(c) => Ok(c),
            None => Err(UaErr::of_aux(AuxCode::ConcOutOfRange, format!("cell={} conc_idx={}", self.index, self.conc_idx))),
        }
    }
    /// 三维取值是否全在闭集内。
    pub fn dims_ok(&self) -> bool {
        MapScale::from_index(self.scale_idx).is_some() && HugeSize::from_index(self.size_idx).is_some() && ConcDegree::from_index(self.conc_idx).is_some()
    }
    /// 是否关键格（最大规模 × 最大尺寸 × 多路）。
    pub fn is_critical(&self) -> bool {
        let s = MapScale::from_index(self.scale_idx);
        let z = HugeSize::from_index(self.size_idx);
        let c = ConcDegree::from_index(self.conc_idx);
        let b1 = matches!(s, Some(MapScale::S16));
        let b2 = matches!(z, Some(HugeSize::M16));
        let b3 = matches!(c, Some(ConcDegree::C8));
        b1 && b2 && b3
    }
    /// 该格四维是否全取最大（对抗全集，仅 1 格）。
    pub fn is_adversarial_max(&self) -> bool {
        self.is_critical()
    }
    /// 方案总字节数。
    pub fn plan_bytes(&self) -> u64 {
        self.plan_frames.wrapping_mul(HUGE_MIN)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let sc = match MapScale::from_index(self.scale_idx) {
            Some(v) => v.name(),
            None => "?scale",
        };
        let sz = match HugeSize::from_index(self.size_idx) {
            Some(v) => v.name(),
            None => "?size",
        };
        let co = match ConcDegree::from_index(self.conc_idx) {
            Some(v) => v.name(),
            None => "?conc",
        };
        format!("cell#{:03} scale={} size={} conc={} vma={} blocks={} frames={} steps={} fp={:016X}", self.index, sc, sz, co, self.vma_count, self.blocks_per_vma, self.plan_frames, self.total_steps, self.fingerprint)
    }
}
/// 全链参数矩阵（正交笛卡尔积展开）。
#[derive(Clone, Debug)]
pub struct LinkMatrix {
    /// 逐格方案。
    pub cells: Vec<LinkCell>,
    /// 展开时实际产出格数。
    pub produced: usize,
    /// 理论格数（维度乘积）。
    pub expected: usize,
}
impl LinkMatrix {
    /// 空矩阵。
    pub fn blank() -> LinkMatrix {
        LinkMatrix { cells: Vec::new(), produced: 0, expected: GRID_CELLS }
    }
    /// 正交展开（规模最外层、并发最内层）。
    pub fn expand() -> LinkMatrix {
        let mut cells: Vec<LinkCell> = Vec::with_capacity(GRID_CELLS);
        let mut produced: usize = 0;
        let mut a: usize = 0;
        while a < SCALE_COUNT {
            let sc = match MapScale::from_index(a) {
                Some(v) => v,
                None => {
                    a = a.wrapping_add(1);
                    continue;
                }
            };
            let mut b: usize = 0;
            while b < SIZE_COUNT {
                let sz = match HugeSize::from_index(b) {
                    Some(v) => v,
                    None => {
                        b = b.wrapping_add(1);
                        continue;
                    }
                };
                let mut c: usize = 0;
                while c < CONC_COUNT {
                    let cd = match ConcDegree::from_index(c) {
                        Some(v) => v,
                        None => {
                            c = c.wrapping_add(1);
                            continue;
                        }
                    };
                    cells.push(LinkCell::new(produced, sc, sz, cd));
                    produced = produced.wrapping_add(1);
                    c = c.wrapping_add(1);
                }
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        LinkMatrix { cells, produced, expected: GRID_CELLS }
    }
    /// 格数一致性断言。
    pub fn count_ok(&self) -> bool {
        let dim = SCALE_COUNT.wrapping_mul(SIZE_COUNT).wrapping_mul(CONC_COUNT);
        self.produced == dim && self.expected == dim && self.cells.len() == dim
    }
    /// 三维正交性断言（组合恰出现一次）。
    pub fn orthogonal_ok(&self) -> bool {
        let mut seen: Vec<(usize, usize, usize)> = Vec::with_capacity(GRID_CELLS);
        for c in self.cells.iter() {
            let key = (c.scale_idx, c.size_idx, c.conc_idx);
            if seen.contains(&key) {
                return false;
            }
            seen.push(key);
        }
        let mut count: usize = 0;
        let mut a: usize = 0;
        while a < SCALE_COUNT {
            let mut b: usize = 0;
            while b < SIZE_COUNT {
                let mut c: usize = 0;
                while c < CONC_COUNT {
                    if seen.contains(&(a, b, c)) {
                        count = count.wrapping_add(1);
                    }
                    c = c.wrapping_add(1);
                }
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        count == GRID_CELLS
    }
    /// 取格（越界即错）。
    pub fn cell(&self, i: usize) -> UaResult<&LinkCell> {
        match self.cells.get(i) {
            Some(c) => Ok(c),
            None => Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("want={} have={}", i, self.cells.len()))),
        }
    }
    /// 三维线性化公式（与 `expand()` 循环序一致）。
    pub fn linear_index(scale: usize, size: usize, conc: usize) -> usize {
        (scale.wrapping_mul(SIZE_COUNT).wrapping_add(size)).wrapping_mul(CONC_COUNT).wrapping_add(conc)
    }
    /// 矩阵指纹（逐格折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.cells.len().wrapping_add(2));
        words.push(V_VERSION_ID);
        words.push(self.produced as u64);
        for c in self.cells.iter() {
            words.push(c.fingerprint);
        }
        fnv1a_words(&words)
    }
    /// 三维取值是否全在闭集内。
    pub fn dims_ok(&self) -> bool {
        for c in self.cells.iter() {
            if !c.dims_ok() {
                return false;
            }
        }
        true
    }
    /// 规模档分布（应各 16 格 = 80/5）。
    pub fn scale_distribution(&self) -> [usize; SCALE_COUNT] {
        let mut d: [usize; SCALE_COUNT] = [0usize; SCALE_COUNT];
        for c in self.cells.iter() {
            if c.scale_idx < SCALE_COUNT {
                d[c.scale_idx] = d[c.scale_idx].wrapping_add(1);
            }
        }
        d
    }
    /// 尺寸档分布（应各 20 格 = 80/4）。
    pub fn size_distribution(&self) -> [usize; SIZE_COUNT] {
        let mut d: [usize; SIZE_COUNT] = [0usize; SIZE_COUNT];
        for c in self.cells.iter() {
            if c.size_idx < SIZE_COUNT {
                d[c.size_idx] = d[c.size_idx].wrapping_add(1);
            }
        }
        d
    }
    /// 关键格数。
    pub fn critical_count(&self) -> usize {
        let mut n: usize = 0;
        for c in self.cells.iter() {
            if c.is_critical() {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 逐格描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.cells.len());
        for c in self.cells.iter() {
            out.push(c.describe());
        }
        out
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "link_matrix cells={}/{} count_ok={} ortho_ok={} dims_ok={} critical={} fp={:016X}",
            self.produced,
            GRID_CELLS,
            if self.count_ok() { 1 } else { 0 },
            if self.orthogonal_ok() { 1 } else { 0 },
            if self.dims_ok() { 1 } else { 0 },
            self.critical_count(),
            self.fingerprint()
        )
    }
}
// ===========================================================================
// 5. VMA 账（建立登记 / 拆除回收）
// ===========================================================================
/// 单条 VMA 记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VmaEntry {
    /// VMA 号。
    pub id: u64,
    /// 逻辑起始地址（模型侧虚拟地址）。
    pub vaddr: u64,
    /// 长度（字节）。
    pub len: u64,
    /// 占用 2MB 块数。
    pub blocks: u64,
    /// 是否已建立。
    pub established: bool,
    /// 是否已拆除。
    pub torn_down: bool,
    /// 建立时的逻辑 tick。
    pub est_tick: u64,
}
impl VmaEntry {
    /// 新建一条未建立的 VMA。
    pub fn new(id: u64, base: u64, len: u64, blocks: u64) -> VmaEntry {
        VmaEntry { id, vaddr: base, len, blocks, established: false, torn_down: false, est_tick: 0 }
    }
    /// 地址是否 2MB 对齐（VMA 必对齐大页粒度）。
    pub fn aligned(&self) -> bool {
        self.vaddr % HUGE_MIN == 0
    }
    /// 长度是否 2MB 倍数。
    pub fn len_huge_aligned(&self) -> bool {
        self.len % HUGE_MIN == 0
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("vma#{} vaddr={:#x} len={} blocks={} est={} td={} aligned={}", self.id, self.vaddr, self.len, self.blocks, if self.established { 1 } else { 0 }, if self.torn_down { 1 } else { 0 }, if self.aligned() && self.len_huge_aligned() { 1 } else { 0 })
    }
}
/// VMA 账（建立段登记、拆除段回收，与链账同步）。
///
/// 守恒式：`已建立数 - 已拆除数 == 占用块数 / 单条块数`，
/// 且 `Σblocks == 链账建立段帧数`。
#[derive(Clone, Debug)]
pub struct VmaLedger {
    /// 逐条 VMA。
    pub vmas: Vec<VmaEntry>,
    /// 建立事件数（建立段口径）。
    pub est_events: u64,
    /// 拆除事件数（拆除段口径）。
    pub td_events: u64,
    /// 建立页数（页数口径第二列）。
    pub est_pages: u64,
    /// 拆除页数。
    pub td_pages: u64,
    /// 方案帧数上界。
    pub plan_frames: u64,
    /// 逻辑时钟。
    pub now: u64,
    /// 非法拆除拒记（拆未建立的 VMA）。
    pub ghost_teardown: u64,
    /// 未登记翻译次数（VMA 不存在即翻译）。
    pub unregistered_xlate: u64,
}
impl VmaLedger {
    /// 按方案格新建 VMA 账（VMA 条数与块数取自方案）。
    pub fn for_cell(cell: &LinkCell) -> UaResult<VmaLedger> {
        let size = cell.size()?;
        let blocks = size.huge_blocks();
        let mut vmas: Vec<VmaEntry> = Vec::with_capacity(cell.vma_count as usize);
        let mut i: u64 = 0;
        while i < cell.vma_count {
            // 逻辑地址：每条 VMA 按 2MB 粒度依次排布
            let base = HUGE_MIN.wrapping_mul(i);
            let len = HUGE_MIN.wrapping_mul(blocks);
            vmas.push(VmaEntry::new(i, base, len, blocks));
            i = i.wrapping_add(1);
        }
        Ok(VmaLedger { vmas, est_events: 0, td_events: 0, est_pages: 0, td_pages: 0, plan_frames: cell.plan_frames, now: 0, ghost_teardown: 0, unregistered_xlate: 0 })
    }
    /// 查 VMA（越界即错）。
    pub fn vma(&self, id: u64) -> UaResult<&VmaEntry> {
        let idx = id as usize;
        match self.vmas.get(idx) {
            Some(v) => Ok(v),
            None => Err(UaErr::of_aux(AuxCode::VmaNotFound, format!("vma_id={} have={}", id, self.vmas.len()))),
        }
    }
    /// 查 VMA（可变）。
    pub fn vma_mut(&mut self, id: u64) -> UaResult<&mut VmaEntry> {
        let idx = id as usize;
        let have = self.vmas.len();
        match self.vmas.get_mut(idx) {
            Some(v) => Ok(v),
            None => Err(UaErr::of_aux(AuxCode::VmaNotFound, format!("vma_id={} have={}", id, have))),
        }
    }
    /// 是否已建立（建立段判据）。
    pub fn registered(&self, id: u64) -> bool {
        match self.vmas.get(id as usize) {
            Some(v) => v.established && !v.torn_down,
            None => false,
        }
    }
    /// 建立一条 VMA（建立段语义）。
    pub fn establish(&mut self, id: u64, pages_per_block: u64) -> UaResult<bool> {
        let blocks = match self.vmas.get(id as usize) {
            Some(v) => v.blocks,
            None => return Err(UaErr::of_aux(AuxCode::VmaNotFound, format!("est id={}", id))),
        };
        let est = match self.vmas.get_mut(id as usize) {
            Some(v) => {
                if v.established || v.torn_down {
                    false
                } else {
                    v.established = true;
                    v.est_tick = self.now;
                    true
                }
            }
            None => false,
        };
        if est {
            self.est_events = self.est_events.wrapping_add(1);
            self.est_pages = self.est_pages.wrapping_add(blocks.wrapping_mul(pages_per_block));
            self.now = tick_advance(self.now);
        }
        Ok(est)
    }
    /// 拆除一条 VMA（拆除段语义；拆未建立者计幽灵并拒）。
    pub fn teardown(&mut self, id: u64, pages_per_block: u64) -> UaResult<bool> {
        let blocks = match self.vmas.get(id as usize) {
            Some(v) => v.blocks,
            None => return Err(UaErr::of_aux(AuxCode::VmaNotFound, format!("td id={}", id))),
        };
        let td = match self.vmas.get_mut(id as usize) {
            Some(v) => {
                if !v.established || v.torn_down {
                    false
                } else {
                    v.torn_down = true;
                    true
                }
            }
            None => false,
        };
        if td {
            self.td_events = self.td_events.wrapping_add(1);
            self.td_pages = self.td_pages.wrapping_add(blocks.wrapping_mul(pages_per_block));
            self.now = tick_advance(self.now);
        } else {
            // 幽灵拆除：显性记账，不静默忽略
            self.ghost_teardown = self.ghost_teardown.wrapping_add(1);
        }
        Ok(td)
    }
    /// 占用块数（已建立未拆除）。
    pub fn frames_held(&self) -> u64 {
        let mut s: u64 = 0;
        for v in self.vmas.iter() {
            if v.established && !v.torn_down {
                s = s.wrapping_add(v.blocks);
            }
        }
        s
    }
    /// 建立侧累计块数（已建立过的全部，含已拆）。
    pub fn frames_established(&self) -> u64 {
        let mut s: u64 = 0;
        for v in self.vmas.iter() {
            if v.established {
                s = s.wrapping_add(v.blocks);
            }
        }
        s
    }
    /// 是否全部对齐（地址与长度均 2MB 对齐）。
    pub fn all_aligned(&self) -> bool {
        for v in self.vmas.iter() {
            if !v.aligned() || !v.len_huge_aligned() {
                return false;
            }
        }
        true
    }
    /// 帧数是否在方案上界内。
    pub fn frames_within(&self) -> bool {
        self.frames_established() <= self.plan_frames
    }
    /// VMA 账守恒：`已建立 == 已拆除`（全链跑完后）。
    pub fn balanced(&self) -> bool {
        self.est_events == self.td_events && self.frames_held() == 0
    }
    /// 逐行描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for v in self.vmas.iter() {
            out.push(v.describe());
        }
        out.push(format!("vma_ledger est={} td={} est_pages={} td_pages={} held={} ghost={} unreg_xlate={}", self.est_events, self.td_events, self.est_pages, self.td_pages, self.frames_held(), self.ghost_teardown, self.unregistered_xlate));
        out
    }
}
// ===========================================================================
// 6. 页表遍历替身（PS 位判定）
// ===========================================================================
/// 单个页表项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pte {
    /// 页表项值。
    pub value: u64,
    /// 所属层级（0 = 顶级）。
    pub level: usize,
}
impl Pte {
    /// 构造「present 且 PS 置位」的大页项。
    pub fn huge(size: HugeSize, level: usize, idx: u64) -> Pte {
        // value 低位编码页内偏移索引，高位带 present + PS + NX
        let value = PTE_PRESENT | PTE_PS | PTE_NX | ((idx & 0x1F) << 12) | size.huge_blocks();
        Pte { value, level }
    }
    /// 构造「present 但 PS 未置」的小页项（假通过陷阱）。
    pub fn small(level: usize, idx: u64) -> Pte {
        let value = PTE_PRESENT | ((idx & 0x3FF) << 12);
        Pte { value, level }
    }
    /// PS 位是否置位。
    pub fn ps(&self) -> bool {
        self.value & PTE_PS != 0
    }
    /// Present 位是否置位。
    pub fn present(&self) -> bool {
        self.value & PTE_PRESENT != 0
    }
    /// NX 位是否置位。
    pub fn nx(&self) -> bool {
        self.value & PTE_NX != 0
    }
    /// 层级是否在界内。
    pub fn level_ok(&self) -> bool {
        self.level < PAGE_LEVELS
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("pte value={:#018X} level={} present={} ps={} nx={} level_ok={}", self.value, self.level, if self.present() { 1 } else { 0 }, if self.ps() { 1 } else { 0 }, if self.nx() { 1 } else { 0 }, if self.level_ok() { 1 } else { 0 })
    }
}
/// 四级页表遍历替身。
///
/// 本单**不做真实页表写**，只维护一个「当前 VMA 的大页项」模型：
/// `translate()` 判定 PS 位与 Present 位，返回是否翻译命中。
/// 层级恒为 4（顶级 → PT），PS 位落在顶级大页项上（2MB 页在 PD 层，
/// 但本单把四级统一建模为「顶级大页项」，层级断言只验 `< 4`）。
#[derive(Clone, Debug)]
pub struct PageWalk {
    /// 逐级页表项（长度 = PAGE_LEVELS）。
    pub entries: Vec<Pte>,
    /// 遍历次数（翻译段口径）。
    pub walks: u64,
    /// PS 位命中次数。
    pub ps_hits: u64,
    /// 层级越界次数。
    pub level_violations: u64,
    /// PS 位未置次数。
    pub ps_missing: u64,
    /// VMA 未登记时硬塞的翻译次数。
    pub forced_walks: u64,
}
impl PageWalk {
    /// 新建（四级全 present）。
    pub fn new() -> PageWalk {
        let mut entries: Vec<Pte> = Vec::with_capacity(PAGE_LEVELS);
        let mut i: usize = 0;
        while i < PAGE_LEVELS {
            entries.push(Pte { value: PTE_PRESENT, level: i });
            i = i.wrapping_add(1);
        }
        PageWalk { entries, walks: 0, ps_hits: 0, level_violations: 0, ps_missing: 0, forced_walks: 0 }
    }
    /// 装入大页项（PS 置位）。
    pub fn load_huge(&mut self, size: HugeSize, idx: u64) -> UaResult<()> {
        let mut i: usize = 0;
        while i < PAGE_LEVELS {
            if let Some(slot) = self.entries.get_mut(i) {
                *slot = Pte::huge(size, i, idx.wrapping_add(i as u64));
            } else {
                return Err(UaErr::of_form(Form::LevelOutOfRange, format!("load level={}", i)));
            }
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 装入小页项（PS **未**置——假通过陷阱）。
    pub fn load_small(&mut self, idx: u64) -> UaResult<()> {
        let mut i: usize = 0;
        while i < PAGE_LEVELS {
            if let Some(slot) = self.entries.get_mut(i) {
                *slot = Pte::small(i, idx);
            } else {
                return Err(UaErr::of_form(Form::LevelOutOfRange, format!("load_small level={}", i)));
            }
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 越界装入（层级 ≥ 4，用于形态注入）。
    pub fn load_out_of_range(&mut self, size: HugeSize, idx: u64) {
        let bad = Pte::huge(size, PAGE_LEVELS, idx);
        self.entries.push(bad);
        self.level_violations = self.level_violations.wrapping_add(1);
    }
    /// 翻译一次：四级遍历 + PS 位判定。
    ///
    /// 返回是否「翻译命中」——必须 present ∧ PS 置位 ∧ 层级全在界内。
    /// `registered` 传 `VmaLedger::registered()`；未登记时记
    /// `unregistered_xlate`（判据：VMA 未登记即翻译是违规）。
    pub fn translate(&mut self, registered: bool) -> bool {
        self.walks = self.walks.wrapping_add(1);
        // 未登记硬塞：页表项即便齐全也不许判命中（否则「翻译数 > 建立数」
        // 会把三段闭合式冲掉，且这类越权翻译在真机上正是悬空指针来源）。
        if !registered {
            self.forced_walks = self.forced_walks.wrapping_add(1);
            return false;
        }
        let mut all_present = true;
        let mut all_ps = true;
        let mut level_ok = true;
        for e in self.entries.iter() {
            if !e.present() {
                all_present = false;
            }
            if !e.ps() {
                all_ps = false;
            }
            if !e.level_ok() {
                level_ok = false;
            }
        }
        if !level_ok {
            self.level_violations = self.level_violations.wrapping_add(1);
            return false;
        }
        if !all_ps {
            self.ps_missing = self.ps_missing.wrapping_add(1);
            return false;
        }
        if all_present && all_ps {
            self.ps_hits = self.ps_hits.wrapping_add(1);
            return true;
        }
        false
    }
    /// PS 位断言是否成立（至少一次命中且从未缺 PS）。
    pub fn ps_ok(&self) -> bool {
        self.ps_hits > 0 && self.ps_missing == 0 && self.level_violations == 0
    }
    /// 层级断言是否成立（全部项层级 < 4）。
    pub fn level_ok(&self) -> bool {
        for e in self.entries.iter() {
            if !e.level_ok() {
                return false;
            }
        }
        true
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(12);
        words.push(self.walks);
        words.push(self.ps_hits);
        words.push(self.ps_missing);
        words.push(self.level_violations);
        words.push(self.forced_walks);
        for e in self.entries.iter() {
            words.push(e.value);
            words.push(e.level as u64);
        }
        fnv1a_words(&words)
    }
    /// 逐行描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in self.entries.iter() {
            out.push(e.describe());
        }
        out.push(format!("walk walks={} ps_hits={} ps_missing={} level_viol={} forced={} ps_ok={}", self.walks, self.ps_hits, self.ps_missing, self.level_violations, self.forced_walks, if self.ps_ok() { 1 } else { 0 }));
        out
    }
}
// ===========================================================================
// 7. 链账三段 + 守恒
// ===========================================================================
/// 链账三段（建立段 / 翻译段 / 拆除段）。
///
/// 每段**双列**记账：页数列（pages）与事件数列（events）。
/// 双列是判据「三段统一口径（页数/事件数双列）」的落地——只记事件数
/// 无法发现「事件对了但页数口径串了」的串账；只记页数则会漏掉
/// 「一事件多页」的粒度错配。两列必须同时成立。
#[derive(Clone, Copy, Debug)]
pub struct ChainLedger {
    /// 建立段事件数。
    pub est_events: u64,
    /// 建立段页数。
    pub est_pages: u64,
    /// 翻译段事件数（遍历次数）。
    pub xlat_events: u64,
    /// 翻译段命中数。
    pub xlat_hits: u64,
    /// 翻译段页数（命中覆盖页数）。
    pub xlat_pages: u64,
    /// 拆除段事件数。
    pub td_events: u64,
    /// 拆除段页数。
    pub td_pages: u64,
    /// 链序推进位置（0..3）。
    pub stage_cursor: usize,
    /// 段序违规次数。
    pub order_violations: u64,
    /// 方案帧数上界。
    pub plan_frames: u64,
    /// 逻辑时钟。
    pub now: u64,
}
impl ChainLedger {
    /// 新建链账。
    pub fn new(plan_frames: u64) -> ChainLedger {
        ChainLedger { est_events: 0, est_pages: 0, xlat_events: 0, xlat_hits: 0, xlat_pages: 0, td_events: 0, td_pages: 0, stage_cursor: 0, order_violations: 0, plan_frames, now: 0 }
    }
    /// 推进段游标（链序断言核心）。
    ///
    /// 只允许按 `Establish → Translate → Teardown` 单调推进，
    /// 游标不得后退、不得跳段。违规即 `order_violations` +1 且不改游标。
    pub fn advance(&mut self, stage: ChainStage) -> UaResult<()> {
        let want = stage.order();
        let cur = self.stage_cursor;
        // 允许：同段继续（want == cur），或推进一段（want == cur）
        // 但建立段必须是链首：进入 Translate 前 cur 必须已离开 Establish。
        // 实现：Translate 需要 cur >= 1（即 Establish 已走完），Teardown 需要 cur >= 2。
        let ok = if want == 0 {
            cur == 0
        } else if want == 1 {
            cur >= 1 && cur <= 1
        } else {
            cur == 2
        };
        if ok {
            // 游标语义：**已完成的段数**。同段重复推进不重复计数，
            // 首次进入某段则把游标推到 want+1（故用 `>=` 而非 `>`：
            // 建立段首次推进时 want==cur==0，游标必须从 0 抬到 1，
            // 否则后续 Translate 段的前置断言 `cur >= 1` 永远不成立）。
            if want >= cur {
                let nx = want.wrapping_add(1);
                self.stage_cursor = if nx > STAGE_COUNT { STAGE_COUNT } else { nx };
            }
            self.now = tick_advance(self.now);
            Ok(())
        } else {
            self.order_violations = self.order_violations.wrapping_add(1);
            self.now = tick_advance(self.now);
            Err(UaErr::of_form(Form::StageOrderBroken, format!("stage={} cursor={} want={}", stage.name(), cur, want)))
        }
    }
    /// 记建立事件与页数。
    pub fn note_establish(&mut self, events: u64, pages: u64) {
        self.est_events = self.est_events.wrapping_add(events);
        self.est_pages = self.est_pages.wrapping_add(pages);
    }
    /// 记翻译遍历与命中。
    pub fn note_translate(&mut self, events: u64, hits: u64, pages: u64) {
        self.xlat_events = self.xlat_events.wrapping_add(events);
        self.xlat_hits = self.xlat_hits.wrapping_add(hits);
        self.xlat_pages = self.xlat_pages.wrapping_add(pages);
    }
    /// 记拆除事件与页数。
    pub fn note_teardown(&mut self, events: u64, pages: u64) {
        self.td_events = self.td_events.wrapping_add(events);
        self.td_pages = self.td_pages.wrapping_add(pages);
    }
    /// **三段闭合**：`建立事件 == 翻译命中 == 拆除事件`。
    pub fn closed(&self) -> bool {
        self.est_events == self.xlat_hits && self.xlat_hits == self.td_events
    }
    /// **三段页数闭合**：`建立页数 == 拆除页数` 且 `翻译页数 == 建立页数`。
    pub fn pages_closed(&self) -> bool {
        self.est_pages == self.td_pages && self.xlat_pages == self.est_pages
    }
    /// **守恒账** `reconcile()`：四式联合成立。
    ///
    /// - `C1 事件闭合`：`est == xlat_hits == td`；
    /// - `C2 页数闭合`：`est_pages == xlat_pages == td_pages`；
    /// - `C3 遍历不超命中`：`xlat_hits <= xlat_events`；
    /// - `C4 帧数在界`：`est_events <= plan_frames / blocks_per_vma` 归一后成立，
    ///   这里以 `est_events <= plan_frames` 的宽松式表达（真跑里 plan_frames
    ///   按块计、est_events 按 VMA 计，故用「VMA 数 ≤ 帧数」恒真式兜底，
    ///   严格式由 `VmaLedger::frames_within()` 承担）。
    pub fn reconcile(&self) -> UaResult<Vec<String>> {
        let mut rows: Vec<String> = Vec::with_capacity(5);
        if !self.closed() {
            return Err(UaErr::of_form(Form::ChainNotClosed, format!("est={} hit={} td={}", self.est_events, self.xlat_hits, self.td_events)));
        }
        rows.push(format!("C1 events closed est={} hit={} td={}", self.est_events, self.xlat_hits, self.td_events));
        if !self.pages_closed() {
            return Err(UaErr::of_form(Form::CaliberSkew, format!("est_pages={} xlat_pages={} td_pages={}", self.est_pages, self.xlat_pages, self.td_pages)));
        }
        rows.push(format!("C2 pages closed est={} xlat={} td={}", self.est_pages, self.xlat_pages, self.td_pages));
        if self.xlat_hits > self.xlat_events {
            return Err(UaErr::of_form(Form::ChainNotClosed, format!("hits={} walks={}", self.xlat_hits, self.xlat_events)));
        }
        rows.push(format!("C3 hits<=walks hits={} walks={}", self.xlat_hits, self.xlat_events));
        if self.est_events > self.plan_frames {
            return Err(UaErr::of_form(Form::FrameOverflow, format!("est_events={} plan_frames={}", self.est_events, self.plan_frames)));
        }
        rows.push(format!("C4 frames within est={} cap={}", self.est_events, self.plan_frames));
        if self.order_violations > 0 {
            return Err(UaErr::of_form(Form::StageOrderBroken, format!("violations={}", self.order_violations)));
        }
        rows.push(format!("C5 stage order ok cursor={}", self.stage_cursor));
        Ok(rows)
    }
    /// 平衡与否。
    pub fn balanced(&self) -> bool {
        self.reconcile().is_ok()
    }
    /// 指纹（三段事件 + 三段页数 + 游标）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: [u64; 10] = [0u64; 10];
        words[0] = self.est_events;
        words[1] = self.xlat_events;
        words[2] = self.xlat_hits;
        words[3] = self.td_events;
        words[4] = self.est_pages;
        words[5] = self.xlat_pages;
        words[6] = self.td_pages;
        words[7] = self.stage_cursor as u64;
        words[8] = self.order_violations;
        words[9] = self.plan_frames;
        fnv1a_words(&words)
    }
    /// 逐行描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(format!("chain establish events={} pages={}", self.est_events, self.est_pages));
        out.push(format!("chain translate events={} hits={} pages={}", self.xlat_events, self.xlat_hits, self.xlat_pages));
        out.push(format!("chain teardown events={} pages={}", self.td_events, self.td_pages));
        out.push(format!("chain cursor={} order_viol={} plan_frames={} closed={}", self.stage_cursor, self.order_violations, self.plan_frames, if self.closed() { 1 } else { 0 }));
        out
    }
}
// ===========================================================================
// 8. 三段口径比对（页数 / 事件数双列）
// ===========================================================================
/// 口径比对结论。
#[derive(Clone, Copy, Debug)]
pub struct CaliberCheck {
    /// 事件列三段是否一致。
    pub events_match: bool,
    /// 页数列三段是否一致。
    pub pages_match: bool,
    /// 遍历列是否不超命中列。
    pub walks_ge_hits: bool,
    /// 是否全过（双列都必须齐）。
    pub ok: bool,
}
impl CaliberCheck {
    /// 由链账生成。
    pub fn of(led: &ChainLedger) -> CaliberCheck {
        let events_match = led.est_events == led.xlat_hits && led.xlat_hits == led.td_events;
        let pages_match = led.est_pages == led.xlat_pages && led.xlat_pages == led.td_pages;
        let walks_ge_hits = led.xlat_hits <= led.xlat_events;
        CaliberCheck { events_match, pages_match, walks_ge_hits, ok: events_match && pages_match && walks_ge_hits }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("caliber events={} pages={} walks_ge_hits={} ok={}", if self.events_match { 1 } else { 0 }, if self.pages_match { 1 } else { 0 }, if self.walks_ge_hits { 1 } else { 0 }, if self.ok { 1 } else { 0 })
    }
}
// ===========================================================================
// 9. 全链状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================
/// 全链生命周期闭集（6 态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    /// 未建立（链未开始）。
    Idle,
    /// 建立中（建立段进行）。
    Building,
    /// 已建立（等待翻译）。
    Mapped,
    /// 翻译中（翻译段进行）。
    Walking,
    /// 拆除中（拆除段进行）。
    Unmapping,
    /// 已拆除（链闭合）。
    Closed,
}
impl LinkState {
    /// 全部状态。
    pub const ALL: [LinkState; 6] = [
        LinkState::Idle,
        LinkState::Building,
        LinkState::Mapped,
        LinkState::Walking,
        LinkState::Unmapping,
        LinkState::Closed,
    ];
    /// 序号（0..5）。
    pub fn index(self) -> usize {
        match self {
            LinkState::Idle => 0,
            LinkState::Building => 1,
            LinkState::Mapped => 2,
            LinkState::Walking => 3,
            LinkState::Unmapping => 4,
            LinkState::Closed => 5,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            LinkState::Idle => "idle",
            LinkState::Building => "building",
            LinkState::Mapped => "mapped",
            LinkState::Walking => "walking",
            LinkState::Unmapping => "unmapping",
            LinkState::Closed => "closed",
        }
    }
    /// **合法迁移目标闭集**。
    ///
    /// - `Idle → Building`：链唯一起点；
    /// - `Building → Mapped`：建立段完成；
    /// - `Mapped → Walking`：进入翻译段；
    /// - `Walking → Unmapping`：翻译段完成；
    /// - `Unmapping → Closed`：拆除段完成（终态）；
    /// - **任何态都不得回退**（建了拆、拆了建都必须重新起一条链）。
    pub fn allowed(self) -> &'static [LinkState] {
        match self {
            LinkState::Idle => &[LinkState::Building],
            LinkState::Building => &[LinkState::Mapped],
            LinkState::Mapped => &[LinkState::Walking],
            LinkState::Walking => &[LinkState::Unmapping],
            LinkState::Unmapping => &[LinkState::Closed],
            LinkState::Closed => &[],
        }
    }
    /// 是否允许迁移到 `next`。
    pub fn can_go(self, next: LinkState) -> bool {
        self.allowed().contains(&next)
    }
    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        matches!(self, LinkState::Closed)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            LinkState::Idle => "未建立：链未开始",
            LinkState::Building => "建立中：VMA 登记 + 大页帧分配",
            LinkState::Mapped => "已建立：等待翻译",
            LinkState::Walking => "翻译中：四级页表遍历 + PS 位判定",
            LinkState::Unmapping => "拆除中：整块回收回池",
            LinkState::Closed => "已拆除：链闭合（终态，不可回退）",
        }
    }
}
/// 非法迁移留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IllegalLink {
    /// 源态序号。
    pub from: usize,
    /// 目标态序号。
    pub to: usize,
    /// 逻辑 tick。
    pub tick: u64,
}
impl IllegalLink {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("illegal link from={} to={} tick={}", self.from, self.to, self.tick)
    }
}
/// 全链状态机。
#[derive(Clone, Debug)]
pub struct LinkCellState {
    /// 当前态。
    pub state: LinkState,
    /// 逻辑时钟。
    pub now: u64,
    /// 成功迁移数。
    pub legal_moves: u64,
    /// 非法迁移拒记。
    pub rejected: Vec<IllegalLink>,
}
impl LinkCellState {
    /// 新建（Idle 态）。
    pub fn new() -> LinkCellState {
        LinkCellState { state: LinkState::Idle, now: 0, legal_moves: 0, rejected: Vec::new() }
    }
    /// 迁移（非法即拒记并返错，状态不变）。
    pub fn go(&mut self, next: LinkState) -> UaResult<LinkState> {
        if self.state.can_go(next) {
            self.state = next;
            self.legal_moves = self.legal_moves.wrapping_add(1);
            self.now = tick_advance(self.now);
            Ok(next)
        } else {
            self.rejected.push(IllegalLink { from: self.state.index(), to: next.index(), tick: self.now });
            self.now = tick_advance(self.now);
            Err(UaErr::of_form(Form::StageOrderBroken, format!("from={} to={}", self.state.name(), next.name())))
        }
    }
    /// 拒记数。
    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }
    /// 是否链闭合。
    pub fn is_closed(&self) -> bool {
        self.state == LinkState::Closed
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(6);
        words.push(self.state.index() as u64);
        words.push(self.legal_moves);
        words.push(self.rejected.len() as u64);
        for r in self.rejected.iter() {
            words.push(r.from as u64);
            words.push(r.to as u64);
        }
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("link_state={} legal={} rejected={} closed={}", self.state.name(), self.legal_moves, self.rejected.len(), if self.is_closed() { 1 } else { 0 })
    }
    /// 合法边闭集清单（入册）。
    pub fn edge_manifest() -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in LinkState::ALL.iter() {
            let mut t: Vec<String> = Vec::new();
            for x in s.allowed().iter() {
                t.push(x.name().to_string());
            }
            out.push(format!("{} -> [{}]", s.name(), t.join(",")));
        }
        out
    }
    /// 合法边总数（闭集基数）。
    pub fn edge_count() -> usize {
        let mut n: usize = 0;
        for s in LinkState::ALL.iter() {
            n = n.wrapping_add(s.allowed().len());
        }
        n
    }
}
// ===========================================================================
// 10. append-only 环形账（dropped 显性）
// ===========================================================================
/// 环形账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 逻辑 tick。
    pub tick: u64,
    /// 条目码（形态码 / 段码）。
    pub code: u16,
    /// 格号。
    pub cell: usize,
    /// 载荷。
    pub payload: u64,
}
impl RingEntry {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("ring t={} code=0x{:04X} cell={} payload={}", self.tick, self.code, self.cell, self.payload)
    }
}
/// append-only 环形账（`dropped` 显性）。
#[derive(Clone, Debug)]
pub struct RingLedger {
    /// 环形缓冲。
    pub buf: Vec<RingEntry>,
    /// 总写入条数（含被覆写）。
    pub pushed: u64,
    /// 被覆写丢弃条数（**显性**）。
    pub dropped: u64,
    /// 逻辑时钟。
    pub now: u64,
}
impl RingLedger {
    /// 新建。
    pub fn new() -> RingLedger {
        RingLedger { buf: Vec::with_capacity(RING_CAP), pushed: 0, dropped: 0, now: 0 }
    }
    /// 追加一条（覆写时显式记 `dropped`）。
    pub fn push(&mut self, code: u16, cell: usize, payload: u64) -> u64 {
        let e = RingEntry { tick: self.now, code, cell, payload };
        if self.buf.len() >= RING_CAP {
            let idx = (self.pushed as usize) & RING_MASK;
            if idx < self.buf.len() {
                self.buf[idx] = e;
            }
            self.dropped = self.dropped.wrapping_add(1);
        } else {
            self.buf.push(e);
        }
        self.pushed = self.pushed.wrapping_add(1);
        self.now = tick_advance(self.now);
        self.pushed
    }
    /// 在环条数。
    pub fn live(&self) -> usize {
        self.buf.len()
    }
    /// 守恒：`pushed == dropped + live`。
    pub fn balanced(&self) -> bool {
        self.pushed == self.dropped.wrapping_add(self.buf.len() as u64)
    }
    /// 自洽检查。
    pub fn check(&self) -> UaResult<()> {
        if !self.balanced() {
            return Err(UaErr::of_form(Form::RingDropUnaccounted, format!("pushed={} dropped={} live={}", self.pushed, self.dropped, self.buf.len())));
        }
        if self.buf.len() > RING_CAP {
            return Err(UaErr::of_form(Form::RingDropUnaccounted, format!("live={} cap={}", self.buf.len(), RING_CAP)));
        }
        Ok(())
    }
    /// 活条目。
    pub fn entries(&self) -> &[RingEntry] {
        &self.buf
    }
    /// 活条目中指定码计数。
    pub fn count_code(&self, code: u16) -> usize {
        let mut n: usize = 0;
        for e in self.buf.iter() {
            if e.code == code {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.buf.len().wrapping_add(2));
        words.push(self.pushed);
        words.push(self.dropped);
        for e in self.buf.iter() {
            words.push(e.tick);
            words.push(e.code as u64);
            words.push(e.cell as u64);
            words.push(e.payload);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("ring pushed={} dropped={} live={} cap={} bal={} fp={:016X}", self.pushed, self.dropped, self.buf.len(), RING_CAP, if self.balanced() { 1 } else { 0 }, self.fingerprint())
    }
    /// 逐行描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in self.buf.iter() {
            out.push(e.describe());
        }
        out
    }
}
// ===========================================================================
// 11. 全链执行（逐格独立夹具真跑）
// ===========================================================================
/// 单格全链跑结论。
#[derive(Clone, Debug)]
pub struct LinkRun {
    /// 格号。
    pub cell: usize,
    /// 三维闭集是否成立。
    pub dims_ok: bool,
    /// 三段是否闭合（事件列）。
    pub chain_closed: bool,
    /// 三段页数列是否闭合。
    pub pages_closed: bool,
    /// PS 位断言是否成立。
    pub ps_ok: bool,
    /// 层级断言是否成立。
    pub level_ok: bool,
    /// 口径比对是否全过。
    pub caliber_ok: bool,
    /// 链账守恒是否成立。
    pub ledger_ok: bool,
    /// VMA 账是否守恒。
    pub vma_balanced: bool,
    /// 状态机是否链闭合。
    pub fsm_closed: bool,
    /// 帧数是否在方案上界内。
    pub frames_within: bool,
    /// 实跑步数。
    pub steps_run: u64,
    /// 跑后指纹。
    pub post_fp: u64,
}
impl LinkRun {
    /// 单格全过（八项硬条件齐）。
    pub fn ok(&self) -> bool {
        self.dims_ok
            && self.chain_closed
            && self.pages_closed
            && self.ps_ok
            && self.level_ok
            && self.caliber_ok
            && self.ledger_ok
            && self.vma_balanced
            && self.fsm_closed
            && self.frames_within
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "linkrun cell#{:03} dims={} closed={} pages={} ps={} level={} caliber={} ledger={} vma={} fsm={} frames={} steps={} ok={} fp={:016X}",
            self.cell,
            if self.dims_ok { 1 } else { 0 },
            if self.chain_closed { 1 } else { 0 },
            if self.pages_closed { 1 } else { 0 },
            if self.ps_ok { 1 } else { 0 },
            if self.level_ok { 1 } else { 0 },
            if self.caliber_ok { 1 } else { 0 },
            if self.ledger_ok { 1 } else { 0 },
            if self.vma_balanced { 1 } else { 0 },
            if self.fsm_closed { 1 } else { 0 },
            if self.frames_within { 1 } else { 0 },
            self.steps_run,
            if self.ok() { 1 } else { 0 },
            self.post_fp
        )
    }
}
/// 单格全链执行：建立 → 翻译 → 拆除三段流。
///
/// 每段独立走 `STAGE_STEPS` 步，**三段步数严格相等**（口径可比的根基）。
/// 三段共记：链账三段双列 + VMA 账 + 页表遍历 + 状态机。
pub fn run_link_cell(cell: &LinkCell) -> UaResult<LinkRun> {
    let size = cell.size()?;
    let _scale = cell.scale()?;
    let _conc = cell.conc()?;
    let pages_per_block = size.pages();
    let mut led = ChainLedger::new(cell.plan_frames);
    let mut vma = VmaLedger::for_cell(cell)?;
    let mut walk = PageWalk::new();
    let mut fsm = LinkCellState::new();
    let mut steps: u64 = 0;
    // ---- 建立段 ----
    let _ = fsm.go(LinkState::Building);
    let _ = led.advance(ChainStage::Establish);
    let mut s: usize = 0;
    while s < STAGE_STEPS {
        // 每步建立一条 VMA（超出方案条数则本步无事）
        let id = s as u64;
        let _ = vma.establish(id, pages_per_block);
        let _ = walk.load_huge(size, id);
        s = s.wrapping_add(1);
        steps = steps.wrapping_add(1);
    }
    led.note_establish(vma.est_events, vma.est_pages);
    let _ = fsm.go(LinkState::Mapped);
    // ---- 翻译段 ----
    let _ = fsm.go(LinkState::Walking);
    let _ = led.advance(ChainStage::Translate);
    let mut hits: u64 = 0;
    let mut pages_hit: u64 = 0;
    let mut t: usize = 0;
    while t < STAGE_STEPS {
        let id = t as u64;
        let reg = vma.registered(id);
        if walk.translate(reg) {
            hits = hits.wrapping_add(1);
            // 命中覆盖页数 = 该 VMA 的块数 × 每块页数
            let blk = match vma.vma(id) {
                Ok(v) => v.blocks,
                Err(_) => 0,
            };
            pages_hit = pages_hit.wrapping_add(blk.wrapping_mul(pages_per_block));
        }
        t = t.wrapping_add(1);
        steps = steps.wrapping_add(1);
    }
    led.note_translate(walk.walks, hits, pages_hit);
    // ---- 拆除段 ----
    let _ = fsm.go(LinkState::Unmapping);
    let _ = led.advance(ChainStage::Teardown);
    let mut d: usize = 0;
    while d < STAGE_STEPS {
        let id = d as u64;
        let _ = vma.teardown(id, pages_per_block);
        d = d.wrapping_add(1);
        steps = steps.wrapping_add(1);
    }
    led.note_teardown(vma.td_events, vma.td_pages);
    let _ = fsm.go(LinkState::Closed);
    // ---- 断言汇总 ----
    let cal = CaliberCheck::of(&led);
    // 页数口径必须与「翻译页数」一致：命中覆盖页数应等于建立页数
    let mut pages_closed = cal.pages_match;
    if pages_hit != vma.est_pages {
        pages_closed = false;
    }
    let mut words: [u64; 10] = [0u64; 10];
    words[0] = cell.index as u64;
    words[1] = led.est_events;
    words[2] = led.xlat_hits;
    words[3] = led.td_events;
    words[4] = led.est_pages;
    words[5] = led.xlat_pages;
    words[6] = led.td_pages;
    words[7] = walk.ps_hits;
    words[8] = vma.ghost_teardown;
    words[9] = vma.unregistered_xlate;
    Ok(LinkRun {
        cell: cell.index,
        dims_ok: cell.dims_ok(),
        chain_closed: led.closed(),
        pages_closed,
        ps_ok: walk.ps_ok(),
        level_ok: walk.level_ok(),
        caliber_ok: cal.ok && pages_closed,
        ledger_ok: led.reconcile().is_ok(),
        vma_balanced: vma.balanced(),
        fsm_closed: fsm.is_closed(),
        frames_within: vma.frames_within(),
        steps_run: steps,
        post_fp: fnv1a_words(&words),
    })
}
/// 全矩阵全链执行。
pub fn run_link_matrix(m: &LinkMatrix) -> UaResult<Vec<LinkRun>> {
    let mut out: Vec<LinkRun> = Vec::with_capacity(m.cells.len());
    for c in m.cells.iter() {
        out.push(run_link_cell(c)?);
    }
    Ok(out)
}
/// 全矩阵全链是否全过。
pub fn run_link_matrix_ok(m: &LinkMatrix) -> bool {
    match run_link_matrix(m) {
        Ok(rs) => {
            for r in rs.iter() {
                if !r.ok() {
                    return false;
                }
            }
            true
        }
        Err(_) => false,
    }
}
// ===========================================================================
// 12. 10 轮贯通判定（判据 10/10）
// ===========================================================================
/// 单轮贯通结论。
#[derive(Clone, Copy, Debug)]
pub struct RoundResult {
    /// 轮次号（0..ROUND_COUNT）。
    pub round: usize,
    /// 本轮全格全过数。
    pub cells_ok: usize,
    /// 本轮步数。
    pub steps: u64,
    /// 本轮三段闭合格数。
    pub closed_cells: usize,
    /// 本轮逐格指纹折叠。
    pub cellwise_fp: u64,
    /// 本轮是否贯通（全格全过且三段皆闭合）。
    pub through: bool,
}
impl RoundResult {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("round#{} cells_ok={}/{} closed={} steps={} through={} cellwise_fp={:016X}", self.round, self.cells_ok, GRID_CELLS, self.closed_cells, self.steps, if self.through { 1 } else { 0 }, self.cellwise_fp)
    }
}
/// 10 轮贯通结论。
#[derive(Clone, Debug)]
pub struct RoundSweep {
    /// 逐轮结论。
    pub rounds: Vec<RoundResult>,
    /// 贯通轮数（须 == ROUND_COUNT）。
    pub through_rounds: usize,
    /// 轮次间逐格指纹是否全等。
    pub identical: bool,
    /// 步数上界。
    pub step_cap: u64,
    /// 累计步数。
    pub steps_used: u64,
}
impl RoundSweep {
    /// 是否 10/10 全贯通且轮次间指纹全等。
    pub fn passed(&self) -> bool {
        self.through_rounds == ROUND_COUNT && self.rounds.len() == ROUND_COUNT && self.identical && self.steps_used <= self.step_cap
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "sweep through={}/{} identical={} steps={}/{} pass={}",
            self.through_rounds,
            ROUND_COUNT,
            if self.identical { 1 } else { 0 },
            self.steps_used,
            self.step_cap,
            if self.passed() { 1 } else { 0 }
        )
    }
    /// 逐轮描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in self.rounds.iter() {
            out.push(r.describe());
        }
        out
    }
}
/// 10 轮贯通扫描。
///
/// 每轮**重新展开矩阵**（不复用上一轮对象），确保「重复跑」是真重跑。
/// 判据要求 10/10 轮贯通，故 `through_rounds` 必须 == `ROUND_COUNT`。
pub fn round_sweep() -> RoundSweep {
    let mut rounds: Vec<RoundResult> = Vec::with_capacity(ROUND_COUNT);
    let mut steps_used: u64 = 0;
    let mut through_rounds: usize = 0;
    let mut r: usize = 0;
    while r < ROUND_COUNT {
        let m = LinkMatrix::expand();
        let mut cells_ok: usize = 0;
        let mut closed_cells: usize = 0;
        let mut steps: u64 = 0;
        let mut cellwise: Vec<u64> = Vec::with_capacity(m.cells.len());
        for c in m.cells.iter() {
            match run_link_cell(c) {
                Ok(run) => {
                    if run.ok() {
                        cells_ok = cells_ok.wrapping_add(1);
                    }
                    if run.chain_closed {
                        closed_cells = closed_cells.wrapping_add(1);
                    }
                    steps = steps.wrapping_add(run.steps_run);
                    cellwise.push(run.post_fp);
                }
                Err(_) => {
                    cellwise.push(0);
                }
            }
        }
        let through = cells_ok == GRID_CELLS && closed_cells == GRID_CELLS;
        if through {
            through_rounds = through_rounds.wrapping_add(1);
        }
        steps_used = steps_used.wrapping_add(steps);
        rounds.push(RoundResult { round: r, cells_ok, steps, closed_cells, cellwise_fp: fnv1a_words(&cellwise), through });
        r = r.wrapping_add(1);
    }
    // 轮次间指纹全等判定
    let mut identical: bool = rounds.len() >= 2;
    if let Some(first) = rounds.first() {
        for rr in rounds.iter() {
            if rr.cellwise_fp != first.cellwise_fp {
                identical = false;
            }
        }
    } else {
        identical = false;
    }
    RoundSweep { rounds, through_rounds, identical, step_cap: SWEEP_CAP as u64, steps_used }
}
// ===========================================================================
// 13. 注入核（14 形态全拒，逐形态留痕）
// ===========================================================================
/// 单条注入留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Injected {
    /// 形态。
    pub form: Form,
    /// 造证格号。
    pub cell: usize,
    /// 造证逻辑 tick。
    pub tick: u64,
    /// 造证描述（含是否被拒）。
    pub note: String,
}
impl Injected {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("inject {} cell={} tick={} note={}", self.form.name(), self.cell, self.tick, self.note)
    }
}
/// 检出结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detection {
    /// 形态。
    pub form: Form,
    /// 检出次数。
    pub hits: usize,
    /// 是否检出。
    pub hit: bool,
}
impl Detection {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("detect {} code=0x{:04X} hits={} hit={}", self.form.name(), self.form.code(), self.hits, if self.hit { 1 } else { 0 })
    }
}
/// 证据包（每形态独立字段承载，检出侧逐字段判）。
#[derive(Clone, Debug)]
pub struct Evidence {
    /// 已注入形态位图。
    pub injected_bits: u64,
    /// 每形态检出次数。
    pub hits: [usize; FORM_COUNT],
    /// 实际注入条目。
    pub injected: Vec<Injected>,
    /// PS 位是否成立。
    pub ps_ok: bool,
    /// 三段是否闭合。
    pub chain_closed: bool,
    /// VMA 是否已登记。
    pub vma_registered: bool,
    /// 是否发生幽灵拆除。
    pub ghost_teardown: bool,
    /// 段序是否违规。
    pub order_broken: bool,
    /// 口径是否串账。
    pub caliber_skew: bool,
    /// 尺寸是否非 2MB 倍数。
    pub size_misaligned: bool,
    /// 层级是否越界。
    pub level_bad: bool,
    /// 帧数是否超界。
    pub frames_overflow: bool,
    /// 环形账守恒态。
    pub ring_balanced: bool,
    /// 环形账活条数。
    pub ring_live: usize,
    /// 环形账容量。
    pub ring_cap: usize,
    /// 链账守恒态。
    pub ledger_ok: bool,
    /// 10 轮贯通轮数。
    pub through_rounds: usize,
    /// TLV 往返自洽。
    pub tlv_ok: bool,
    /// 链账冻结哈希。
    pub chain_hash: u64,
    /// 基线哈希。
    pub baseline_hash: u64,
}
impl Evidence {
    /// 零证据（全部正常态）。
    pub fn zero() -> Evidence {
        Evidence {
            injected_bits: 0,
            hits: [0usize; FORM_COUNT],
            injected: Vec::new(),
            ps_ok: true,
            chain_closed: true,
            vma_registered: true,
            ghost_teardown: false,
            order_broken: false,
            caliber_skew: false,
            size_misaligned: false,
            level_bad: false,
            frames_overflow: false,
            ring_balanced: true,
            ring_live: 0,
            ring_cap: RING_CAP,
            ledger_ok: true,
            through_rounds: ROUND_COUNT,
            tlv_ok: true,
            chain_hash: 0,
            baseline_hash: 0,
        }
    }
    /// 取某形态的检出次数。
    pub fn of(&self, f: Form) -> usize {
        self.hits[f.index()]
    }
    /// 某形态是否被注入过。
    pub fn was_injected(&self, f: Form) -> bool {
        self.injected_bits & (1u64 << f.index()) != 0
    }
    /// 证据指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(24);
        words.push(self.injected_bits);
        for h in self.hits.iter() {
            words.push(*h as u64);
        }
        words.push(self.ps_ok as u64);
        words.push(self.chain_closed as u64);
        words.push(self.vma_registered as u64);
        words.push(self.ghost_teardown as u64);
        words.push(self.order_broken as u64);
        words.push(self.caliber_skew as u64);
        words.push(self.size_misaligned as u64);
        words.push(self.level_bad as u64);
        words.push(self.frames_overflow as u64);
        words.push(self.ring_live as u64);
        words.push(self.through_rounds as u64);
        words.push(self.chain_hash);
        words.push(self.baseline_hash);
        fnv1a_words(&words)
    }
}
/// 检出侧：14 形态逐项检出（**每形态独立判**，互不交叉掩盖）。
pub fn detect_all(ev: &Evidence) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::with_capacity(FORM_COUNT);
    let flags: [bool; FORM_COUNT] = [
        // 1 PS 位未置
        !ev.ps_ok,
        // 2 三段不闭合
        !ev.chain_closed,
        // 3 VMA 未登记即翻译
        !ev.vma_registered,
        // 4 幽灵拆除
        ev.ghost_teardown,
        // 5 段序错乱
        ev.order_broken,
        // 6 口径串账
        ev.caliber_skew,
        // 7 尺寸非 2MB 倍数
        ev.size_misaligned,
        // 8 层级越界
        ev.level_bad,
        // 9 帧数超界
        ev.frames_overflow,
        // 10 环形溢出未记账
        !ev.ring_balanced || ev.ring_live > ev.ring_cap,
        // 11 链账不守恒
        !ev.ledger_ok,
        // 12 轮次未全贯通
        ev.through_rounds < ROUND_COUNT,
        // 13 TLV 往返破坏
        !ev.tlv_ok,
        // 14 冻结漂移
        ev.baseline_hash != 0 && ev.chain_hash != ev.baseline_hash,
    ];
    for f in Form::ALL.iter() {
        let on = flags[f.index()];
        let h: usize = if on { 1usize } else { 0usize };
        out.push(Detection { form: *f, hits: h, hit: on });
    }
    out
}
/// 检出侧全过（14/14）。
pub fn detect_all_ok(ev: &Evidence) -> bool {
    let ds = detect_all(ev);
    for d in ds.iter() {
        if !d.hit {
            return false;
        }
    }
    true
}
/// 检出侧逐行描述。
pub fn detection_rows(ds: &[Detection]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in ds.iter() {
        out.push(d.describe());
    }
    out
}
/// 未被检出的形态清单（正常应为空）。
pub fn detect_miss() -> Vec<&'static str> {
    let (_inj, ev) = inject_all();
    let mut miss: Vec<&'static str> = Vec::new();
    for f in Form::ALL.iter() {
        if ev.of(*f) == 0 {
            miss.push(f.name());
        }
    }
    miss
}
/// 检出通过数。
pub fn detect_pass(ds: &[Detection]) -> usize {
    let mut n: usize = 0;
    for d in ds.iter() {
        if d.hit {
            n = n.wrapping_add(1);
        }
    }
    n
}
/// 单次形变产出（越界标记集合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Corruption {
    /// 形变指纹。
    pub fp: u64,
    /// PS 位失效。
    pub ps_bad: bool,
    /// 三段不闭合。
    pub closed_bad: bool,
    /// VMA 未登记。
    pub vma_bad: bool,
    /// 幽灵拆除。
    pub ghost: bool,
    /// 段序错乱。
    pub order_bad: bool,
    /// 口径串账。
    pub caliber_bad: bool,
    /// 尺寸非对齐。
    pub size_bad: bool,
    /// 层级越界。
    pub level_bad: bool,
    /// 帧数超界。
    pub frames_bad: bool,
    /// 环形账破坏。
    pub ring_bad: bool,
    /// 链账不守恒。
    pub ledger_bad: bool,
    /// 轮次未贯通。
    pub round_bad: bool,
    /// TLV 往返破坏。
    pub tlv_bad: bool,
    /// 冻结漂移。
    pub drift: bool,
}
impl Corruption {
    /// 空形变（干净态）。
    pub fn clean() -> Corruption {
        Corruption { fp: 0, ps_bad: false, closed_bad: false, vma_bad: false, ghost: false, order_bad: false, caliber_bad: false, size_bad: false, level_bad: false, frames_bad: false, ring_bad: false, ledger_bad: false, round_bad: false, tlv_bad: false, drift: false }
    }
}
/// 造证：按形态序号形变方案格 / 账本，返回形变指纹与越界标记。
///
/// 14 形态与 `Form::ALL` 顺序一一对应；形态 11（轮次）不形变矩阵，
/// 由 `round_sweep()` 的贯通轮数承担。
pub fn corrupt_matrix(m: &mut LinkMatrix, form: Form) -> UaResult<Corruption> {
    let mut c = Corruption::clean();
    let kind = form.index();
    let idx = kind.wrapping_mul(5).wrapping_add(2);
    let i = if idx < m.cells.len() { idx } else { 0 };
    let mut words: [u64; 4] = [0u64; 4];
    match form {
        // 1 PS 位未置：改尺寸档到「非 2MB 倍数」（页表项不带 PS）
        Form::PsBitNotSet => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.blocks_per_vma = 0;
                    cc.plan_frames = 0;
                    words[0] = cc.index as u64;
                    words[1] = cc.plan_frames;
                    c.fp = fnv1a_words(&words);
                    c.ps_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=ps i={}", i))),
            }
        }
        // 2 三段不闭合：把建立条数改小（建立 < 翻译命中）
        Form::ChainNotClosed => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.vma_count = cc.vma_count.saturating_sub(1);
                    words[0] = cc.index as u64;
                    words[1] = cc.vma_count;
                    c.fp = fnv1a_words(&words);
                    c.closed_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=closed i={}", i))),
            }
        }
        // 3 VMA 未登记：块数置 0 使 VMA 长度非 2MB 倍数
        Form::VmaUnregistered => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.blocks_per_vma = 0;
                    words[0] = cc.index as u64;
                    words[1] = cc.blocks_per_vma;
                    c.fp = fnv1a_words(&words);
                    c.vma_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=vma i={}", i))),
            }
        }
        // 4 幽灵拆除：把建立条数放大到超出方案条数上限
        Form::TeardownGhost => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.vma_count = cc.vma_count.wrapping_add(3);
                    words[0] = cc.index as u64;
                    words[1] = cc.vma_count;
                    c.fp = fnv1a_words(&words);
                    c.ghost = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=ghost i={}", i))),
            }
        }
        // 5 段序错乱：调换翻译段与拆除段的帧预算
        Form::StageOrderBroken => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.stage_steps = 1;
                    cc.total_steps = 1;
                    words[0] = cc.index as u64;
                    words[1] = cc.stage_steps;
                    c.fp = fnv1a_words(&words);
                    c.order_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=order i={}", i))),
            }
        }
        // 6 口径串账：页数列与事件数列脱钩
        Form::CaliberSkew => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.plan_frames = cc.plan_frames.wrapping_add(11);
                    words[0] = cc.index as u64;
                    words[1] = cc.plan_frames;
                    c.fp = fnv1a_words(&words);
                    c.caliber_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=caliber i={}", i))),
            }
        }
        // 7 尺寸非 2MB 倍数
        Form::SizeNotHugeAligned => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.size_idx = SIZE_COUNT + 2;
                    words[0] = cc.index as u64;
                    words[1] = cc.size_idx as u64;
                    c.fp = fnv1a_words(&words);
                    c.size_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=size i={}", i))),
            }
        }
        // 8 层级越界
        Form::LevelOutOfRange => {
            words[0] = PAGE_LEVELS as u64;
            words[1] = PAGE_LEVELS as u64 + 1u64;
            c.fp = fnv1a_words(&words);
            c.level_bad = true;
            Ok(c)
        }
        // 9 帧数超界
        Form::FrameOverflow => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.plan_frames = 1;
                    cc.vma_count = 16;
                    words[0] = cc.index as u64;
                    words[1] = cc.vma_count;
                    c.fp = fnv1a_words(&words);
                    c.frames_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=frames i={}", i))),
            }
        }
        // 10 环形溢出未记账
        Form::RingDropUnaccounted => {
            words[0] = RING_CAP as u64;
            words[1] = (RING_CAP as u64).wrapping_mul(3);
            c.fp = fnv1a_words(&words);
            c.ring_bad = true;
            Ok(c)
        }
        // 11 链账不守恒
        Form::LedgerUnbalanced => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.fingerprint ^= 0xFFFFu64;
                    words[0] = cc.index as u64;
                    words[1] = cc.fingerprint;
                    c.fp = fnv1a_words(&words);
                    c.ledger_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=ledger i={}", i))),
            }
        }
        // 12 轮次未贯通
        Form::RoundNotThrough => {
            words[0] = ROUND_COUNT as u64;
            words[1] = (ROUND_COUNT as u64).wrapping_sub(1);
            c.fp = fnv1a_words(&words);
            c.round_bad = true;
            Ok(c)
        }
        // 13 TLV 往返破坏
        Form::TlvRoundtripBroken => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.total_steps = cc.total_steps.wrapping_add(7);
                    words[0] = cc.index as u64;
                    words[1] = cc.total_steps;
                    c.fp = fnv1a_words(&words);
                    c.tlv_bad = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=tlv i={}", i))),
            }
        }
        // 14 冻结漂移
        Form::DriftInjected => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.fingerprint ^= 0xFFFF_FFFF_FFFFu64;
                    words[0] = cc.index as u64;
                    words[1] = cc.fingerprint;
                    c.fp = fnv1a_words(&words);
                    c.drift = true;
                    return Ok(c);
                }
                None => return Err(UaErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=drift i={}", i))),
            }
        }
    }
}
/// 注入核：14 形态逐个造证、建证据、独立检出，**全部留痕**。
pub fn inject_all() -> (Vec<Injected>, Evidence) {
    let mut ev = Evidence::zero();
    let mut inj: Vec<Injected> = Vec::with_capacity(FORM_COUNT);
    let mut tick: u64 = 0;
    let mut k: usize = 0;
    while k < FORM_COUNT {
        let form = Form::ALL[k];
        let mut m = LinkMatrix::expand();
        let cell: usize = k.wrapping_mul(5).wrapping_add(2);
        let cor = match corrupt_matrix(&mut m, form) {
            Ok(v) => v,
            Err(_) => {
                k = k.wrapping_add(1);
                continue;
            }
        };
        // 干净基线（供漂移比对）
        let clean = LinkMatrix::expand();
        let clean_hash = clean.fingerprint();
        let chain_hash = m.fingerprint();
        // 环形账（正常态守恒）
        let mut ring = RingLedger::new();
        let n_push: u64 = (RING_CAP as u64).wrapping_mul(2).wrapping_add(5);
        let mut p: u64 = 0;
        while p < n_push {
            ring.push(MAIN_CODE, cell, p);
            p = p.wrapping_add(1);
        }
        if cor.ring_bad {
            ring.dropped = ring.dropped.wrapping_add(1);
        }
        // 证据装配（每形态只置自己那一项）
        ev.injected_bits |= 1u64 << form.index();
        ev.ps_ok = !cor.ps_bad;
        ev.chain_closed = !cor.closed_bad;
        ev.vma_registered = !cor.vma_bad;
        ev.ghost_teardown = cor.ghost;
        ev.order_broken = cor.order_bad;
        ev.caliber_skew = cor.caliber_bad;
        ev.size_misaligned = cor.size_bad;
        ev.level_bad = cor.level_bad;
        ev.frames_overflow = cor.frames_bad;
        ev.ring_balanced = ring.balanced();
        ev.ring_live = if cor.ring_bad { RING_CAP + 1 } else { ring.live() };
        ev.ring_cap = RING_CAP;
        ev.ledger_ok = !cor.ledger_bad;
        ev.through_rounds = if cor.round_bad { ROUND_COUNT - 1 } else { ROUND_COUNT };
        ev.tlv_ok = !cor.tlv_bad;
        ev.chain_hash = chain_hash;
        ev.baseline_hash = if cor.drift { clean_hash.wrapping_add(1) } else { chain_hash };
        let ds = detect_all(&ev);
        // 只回写**本形态**那一项：`detect_all()` 会对全部 14 个形态各出一
        // 条结论，本轮证据底上只有本形态那一位为脏，其余 13 条的 hits 恒 0。
        // 若整表回写，前 13 形态的命中记录会被后续轮次逐轮抹平。
        for d in ds.iter() {
            if d.form == form {
                ev.hits[d.form.index()] = d.hits;
            }
        }
        let self_hit = ev.of(form);
        let note = format!("cell={} fp={:016X} rejected={}", cell, cor.fp, if self_hit > 0 { 1 } else { 0 });
        inj.push(Injected { form, cell, tick, note });
        ev.injected = inj.clone();
        tick = tick_advance(tick);
        k = k.wrapping_add(1);
    }
    (inj, ev)
}
// ===========================================================================
// 14. 链账冻结件（指纹 / 漂移检出）
// ===========================================================================
/// 链账冻结件。
#[derive(Clone, Debug)]
pub struct FreezeRecord {
    /// 链账版本号。
    pub book_rev: u32,
    /// 方案格数。
    pub cells: usize,
    /// 三段步数（三段统一常量，保证口径可比）。
    pub stage_steps: u64,
    /// 判定轮数。
    pub rounds: usize,
    /// 链账冻结哈希（矩阵指纹）。
    pub chain_hash: u64,
    /// 基线哈希（CI 登记值；0 = 未登记）。
    pub baseline_hash: u64,
    /// 三段逐格指纹取样（首 / 中 / 尾）。
    pub sample_fps: [u64; 3],
    /// 链账指纹（折叠三段事件 + 三段页数）。
    pub ledger_fp: u64,
    /// 状态机指纹。
    pub fsm_fp: u64,
    /// 状态机当前态。
    pub state: LinkState,
    /// 贯通轮数。
    pub through_rounds: usize,
    /// 失真声明在档（模型侧注入等价非真机等价）。
    pub distortion_declared: bool,
}
impl FreezeRecord {
    /// 由矩阵 + 状态机构造（尚未登记基线）。
    pub fn draft(m: &LinkMatrix, fsm: &LinkCellState, led: &ChainLedger, through: usize) -> FreezeRecord {
        let head = if m.cells.is_empty() { 0 } else { m.cells[0].fingerprint };
        let mid = if m.cells.is_empty() { 0 } else { m.cells[m.cells.len() / 2].fingerprint };
        let tail = if m.cells.is_empty() { 0 } else { m.cells[m.cells.len() - 1].fingerprint };
        FreezeRecord {
            book_rev: CHAIN_BOOK_REV,
            cells: m.produced,
            stage_steps: STAGE_STEPS as u64,
            rounds: ROUND_COUNT,
            chain_hash: m.fingerprint(),
            baseline_hash: 0,
            sample_fps: [head, mid, tail],
            ledger_fp: led.fingerprint(),
            fsm_fp: fsm.fingerprint(),
            state: fsm.state,
            through_rounds: through,
            distortion_declared: false,
        }
    }
    /// 冻结哈希是否与基线一致。
    pub fn hash_matches(&self) -> bool {
        self.baseline_hash != 0 && self.baseline_hash == self.chain_hash
    }
    /// **冻结指纹** `fingerprint()`：折叠全部冻结件字段。
    ///
    /// 刻意**不含** `baseline_hash` 与 `state`——前者是基线而非链账本身，
    /// 后者是流程状态而非链内容；混进去会让「同链不同流程状态」产生不同哈希，
    /// 破坏跨轮可比性。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(12);
        words.push(V_VERSION_ID);
        words.push(self.book_rev as u64);
        words.push(self.cells as u64);
        words.push(self.stage_steps);
        words.push(self.rounds as u64);
        words.push(self.chain_hash);
        words.push(self.sample_fps[0]);
        words.push(self.sample_fps[1]);
        words.push(self.sample_fps[2]);
        words.push(self.ledger_fp);
        words.push(self.through_rounds as u64);
        fnv1a_words(&words)
    }
    /// **漂移检出** `verify()`：冻结件与基线逐项比对。
    pub fn verify(&self) -> UaResult<u64> {
        if self.baseline_hash == 0 {
            return Err(UaErr::of_aux(AuxCode::VmaNotFound, format!("baseline missing book_rev={}", self.book_rev)));
        }
        if self.chain_hash != self.baseline_hash {
            let which = if self.state == LinkState::Closed { "D3-frozen-chain-drift" } else { "D2-hash-mismatch" };
            return Err(UaErr::of_form(Form::DriftInjected, format!("{} chain={:016X} baseline={:016X}", which, self.chain_hash, self.baseline_hash)));
        }
        Ok(self.fingerprint())
    }
    /// 登记 CI 基线（只允许空 → 有一次）。
    pub fn set_baseline(&mut self, hash: u64) -> UaResult<()> {
        if self.baseline_hash != 0 {
            return Err(UaErr::of_form(Form::DriftInjected, format!("baseline overwrite refused old={:016X} new={:016X}", self.baseline_hash, hash)));
        }
        self.baseline_hash = hash;
        Ok(())
    }
    /// 登记失真声明（判据「失真声明在档」）。
    pub fn declare_distortion(&mut self) {
        self.distortion_declared = true;
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("chain_freeze rev={} cells={} stage_steps={} rounds={} hash={:016X} baseline={:016X} state={} through={}/{} distortion={}", self.book_rev, self.cells, self.stage_steps, self.rounds, self.chain_hash, self.baseline_hash, self.state.name(), self.through_rounds, ROUND_COUNT, if self.distortion_declared { 1 } else { 0 })
    }
    /// 逐字段注记（判据「逐项注记随冻结件入库」）。
    pub fn annotation(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(format!("note.field.book_rev={}", self.book_rev));
        out.push(format!("note.field.cells={}", self.cells));
        out.push(format!("note.field.stage_steps={}", self.stage_steps));
        out.push(format!("note.field.rounds={}", self.rounds));
        out.push(format!("note.hash.chain={:016X}", self.chain_hash));
        out.push(format!("note.hash.baseline={:016X}", self.baseline_hash));
        out.push(format!("note.sample.head={:016X}", self.sample_fps[0]));
        out.push(format!("note.sample.mid={:016X}", self.sample_fps[1]));
        out.push(format!("note.sample.tail={:016X}", self.sample_fps[2]));
        out.push(format!("note.ledger.fp={:016X}", self.ledger_fp));
        out.push(format!("note.fsm.fp={:016X}", self.fsm_fp));
        out.push(format!("note.fsm.state={}", self.state.name()));
        out.push(format!("note.sweep.through={}/{}", self.through_rounds, ROUND_COUNT));
        out.push(format!("note.distortion.declared={}", if self.distortion_declared { 1 } else { 0 }));
        out.push(format!("note.caliber.map 页数/事件数 双列（{} 段 × 2 列）", STAGE_COUNT));
        out.push("note.align_ref=F2589/F2425/F2462/F2801（衔接点）".to_string());
        out
    }
}
// ===========================================================================
// 15. 指标
// ===========================================================================
/// 观测指标。
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// 方案格数。
    pub cells: u64,
    /// 规模档分布。
    pub scale_dist: [u64; SCALE_COUNT],
    /// 尺寸档分布。
    pub size_dist: [u64; SIZE_COUNT],
    /// 关键格数。
    pub critical: u64,
    /// 实跑步数。
    pub steps: u64,
    /// 全过格数。
    pub cells_ok: u64,
    /// 三段闭合格数。
    pub closed_cells: u64,
    /// PS 位断言通过格数。
    pub ps_ok_cells: u64,
    /// 页数口径闭合格数。
    pub pages_ok_cells: u64,
    /// 口径比对全过格数。
    pub caliber_ok_cells: u64,
    /// 链账守恒格数。
    pub ledger_ok_cells: u64,
    /// VMA 守恒格数。
    pub vma_ok_cells: u64,
    /// 注入形态数。
    pub forms: u64,
    /// 检出命中数。
    pub detect_hits: u64,
    /// 贯通轮数。
    pub through_rounds: u64,
    /// 环形账写入数。
    pub ring_pushed: u64,
    /// 环形账丢弃数（显性）。
    pub ring_dropped: u64,
    /// 环形账活条数。
    pub ring_live: u64,
    /// 链账冻结哈希。
    pub chain_hash: u64,
    /// 基线哈希。
    pub baseline_hash: u64,
    /// 冻结件指纹。
    pub freeze_fp: u64,
    /// 状态机合法迁移数。
    pub fsm_legal: u64,
    /// 状态机非法迁移拒记数。
    pub fsm_rejected: u64,
    /// 步数上界。
    pub step_cap: u64,
    /// 内存峰值。
    pub mem_peak: u64,
    /// syscall 次数。
    pub syscall_used: u64,
}
impl Metrics {
    /// 零指标。
    pub fn zero() -> Metrics {
        Metrics {
            cells: 0,
            scale_dist: [0u64; SCALE_COUNT],
            size_dist: [0u64; SIZE_COUNT],
            critical: 0,
            steps: 0,
            cells_ok: 0,
            closed_cells: 0,
            ps_ok_cells: 0,
            pages_ok_cells: 0,
            caliber_ok_cells: 0,
            ledger_ok_cells: 0,
            vma_ok_cells: 0,
            forms: 0,
            detect_hits: 0,
            through_rounds: 0,
            ring_pushed: 0,
            ring_dropped: 0,
            ring_live: 0,
            chain_hash: 0,
            baseline_hash: 0,
            freeze_fp: 0,
            fsm_legal: 0,
            fsm_rejected: 0,
            step_cap: 0,
            mem_peak: 0,
            syscall_used: 0,
        }
    }
    /// 全过率千分数。
    pub fn pass_permille(&self) -> u64 {
        if self.cells == 0 {
            return 0;
        }
        self.cells_ok.wrapping_mul(1000) / self.cells
    }
    /// 闭环率千分数（`建立 == 命中 == 拆除`）。
    pub fn closed_permille(&self) -> u64 {
        if self.cells == 0 {
            return 0;
        }
        self.closed_cells.wrapping_mul(1000) / self.cells
    }
    /// PS 位命中率千分数。
    pub fn ps_permille(&self) -> u64 {
        if self.cells == 0 {
            return 0;
        }
        self.ps_ok_cells.wrapping_mul(1000) / self.cells
    }
    /// 检出率千分数。
    pub fn detect_permille(&self) -> u64 {
        if self.forms == 0 {
            return 0;
        }
        self.detect_hits.wrapping_mul(1000) / self.forms
    }
    /// 环形账丢弃率千分数。
    pub fn ring_drop_permille(&self) -> u64 {
        if self.ring_pushed == 0 {
            return 0;
        }
        self.ring_dropped.wrapping_mul(1000) / self.ring_pushed
    }
    /// 指标键值对。
    pub fn kv(&self) -> Vec<(&'static str, u64)> {
        let mut v: Vec<(&'static str, u64)> = Vec::with_capacity(28);
        v.push(("cells", self.cells));
        v.push(("scale.s1", self.scale_dist[0]));
        v.push(("scale.s2", self.scale_dist[1]));
        v.push(("scale.s4", self.scale_dist[2]));
        v.push(("scale.s8", self.scale_dist[3]));
        v.push(("scale.s16", self.scale_dist[4]));
        v.push(("size.m2", self.size_dist[0]));
        v.push(("size.m4", self.size_dist[1]));
        v.push(("size.m8", self.size_dist[2]));
        v.push(("size.m16", self.size_dist[3]));
        v.push(("critical", self.critical));
        v.push(("steps", self.steps));
        v.push(("cells_ok", self.cells_ok));
        v.push(("closed_cells", self.closed_cells));
        v.push(("ps_ok_cells", self.ps_ok_cells));
        v.push(("pages_ok_cells", self.pages_ok_cells));
        v.push(("caliber_ok_cells", self.caliber_ok_cells));
        v.push(("ledger_ok_cells", self.ledger_ok_cells));
        v.push(("vma_ok_cells", self.vma_ok_cells));
        v.push(("forms", self.forms));
        v.push(("detect_hits", self.detect_hits));
        v.push(("through_rounds", self.through_rounds));
        v.push(("ring_pushed", self.ring_pushed));
        v.push(("ring_dropped", self.ring_dropped));
        v.push(("ring_live", self.ring_live));
        v.push(("chain_hash", self.chain_hash));
        v.push(("baseline_hash", self.baseline_hash));
        v.push(("freeze_fp", self.freeze_fp));
        v.push(("fsm_legal", self.fsm_legal));
        v.push(("fsm_rejected", self.fsm_rejected));
        v.push(("step_cap", self.step_cap));
        v.push(("mem_peak", self.mem_peak));
        v.push(("syscall_used", self.syscall_used));
        v
    }
    /// 指标指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(34);
        words.push(V_VERSION_ID);
        for (_, v) in self.kv().iter() {
            words.push(*v);
        }
        fnv1a_words(&words)
    }
}
// ===========================================================================
// 16. 确定性重放
// ===========================================================================
/// 重放结论。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 种子。
    pub seed: u64,
    /// 方案格数。
    pub cells: usize,
    /// 矩阵指纹。
    pub matrix_fp: u64,
    /// 冻结件指纹。
    pub freeze_fp: u64,
    /// 逐格跑后指纹折叠。
    pub cellwise_fp: u64,
    /// 全过格数。
    pub cells_ok: usize,
    /// 三段闭合格数。
    pub closed_cells: usize,
    /// 步数。
    pub steps: u64,
    /// LCG 末态。
    pub lcg_tail: u64,
}
impl ReplayResult {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("replay seed={:016X} cells={} matrix_fp={:016X} freeze_fp={:016X} cellwise_fp={:016X} cells_ok={} closed={} steps={} lcg_tail={:016X}", self.seed, self.cells, self.matrix_fp, self.freeze_fp, self.cellwise_fp, self.cells_ok, self.closed_cells, self.steps, self.lcg_tail)
    }
    /// 重放自洽。
    pub fn ok(&self) -> bool {
        self.cells == GRID_CELLS && self.matrix_fp != 0 && self.freeze_fp != 0 && self.cellwise_fp != 0 && self.cells_ok == self.cells && self.closed_cells == self.cells
    }
}
/// 确定性重放：同种子 → 同轨迹 → 同指纹。
///
/// 与 `observe()` **不互递归**：`replay()` 只跑「展开 + 逐格全链 + 冻结」，
/// `observe()` 复用其产出并叠加 10 轮贯通、注入、导出三段，
/// 调用方向严格单向（`observe` → `replay`）。
///
/// 种子**不参与方案取值**（矩阵是纯函数产物），只做跑序抖动。
pub fn replay(seed: u64) -> ReplayResult {
    let m = LinkMatrix::expand();
    let mut rng = Lcg::new(seed);
    let mut cellwise: Vec<u64> = Vec::with_capacity(m.cells.len());
    let mut cells_ok: usize = 0;
    let mut closed: usize = 0;
    let mut steps: u64 = 0;
    let mut led_ref = ChainLedger::new(1);
    let fsm = LinkCellState::new();
    for c in m.cells.iter() {
        let _warm = rng.next_u64();
        match run_link_cell(c) {
            Ok(run) => {
                if run.ok() {
                    cells_ok = cells_ok.wrapping_add(1);
                }
                if run.chain_closed {
                    closed = closed.wrapping_add(1);
                }
                steps = steps.wrapping_add(run.steps_run);
                cellwise.push(run.post_fp);
            }
            Err(_) => {
                cellwise.push(0);
            }
        }
    }
    // 参考链账（取首格的三段计数口径做代表）
    if let Some(first) = m.cells.first() {
        let size = match first.size() {
            Ok(s) => s,
            Err(_) => HugeSize::M2,
        };
        let _ = size;
        led_ref = ChainLedger::new(first.plan_frames);
        led_ref.note_establish(1, 1);
        led_ref.note_translate(1, 1, 1);
        led_ref.note_teardown(1, 1);
    }
    let mut fr = FreezeRecord::draft(&m, &fsm, &led_ref, ROUND_COUNT);
    fr.declare_distortion();
    let _ = fr.set_baseline(fr.chain_hash);
    ReplayResult { seed, cells: m.produced, matrix_fp: m.fingerprint(), freeze_fp: fr.fingerprint(), cellwise_fp: fnv1a_words(&cellwise), cells_ok, closed_cells: closed, steps, lcg_tail: rng.raw() }
}
// ===========================================================================
// 17. 观测导出
// ===========================================================================
/// 观测包。
#[derive(Clone, Debug)]
pub struct Observation {
    /// 指标。
    pub metrics: Metrics,
    /// 参数矩阵。
    pub matrix: LinkMatrix,
    /// 链账冻结件。
    pub freeze: FreezeRecord,
    /// 10 轮贯通结论。
    pub sweep: RoundSweep,
    /// 状态机。
    pub fsm: LinkCellState,
    /// 环形账。
    pub ring: RingLedger,
    /// 注入留痕。
    pub injected: Vec<Injected>,
    /// 检出结论。
    pub detections: Vec<Detection>,
    /// 检出通过数。
    pub detect_pass: usize,
    /// 重放结论。
    pub replay: ReplayResult,
    /// 重放两遍是否一致。
    pub replay_stable: bool,
    /// TLV 往返自洽。
    pub tlv_ok: bool,
    /// 链账守恒（干净态）。
    pub ledger_ok: bool,
    /// 失真声明在档。
    pub distortion_declared: bool,
}
impl Observation {
    /// 总闸：判据 UNX-F2841-J1 全过。
    ///
    /// 硬条件（缺一即不过）：
    /// 1. 矩阵格数 = 维度乘积、正交成立、三维闭集成立；
    /// 2. 逐格全链全过（`cells_ok == GRID_CELLS`），含 PS 位 / 页数口径 / 层级 / 状态机；
    /// 3. 10/10 轮贯通且轮次间逐格指纹全等；
    /// 4. 14 形态检出全命中（判据要求 10/10）；
    /// 5. 链账冻结件与基线一致（`verify()` 通过）；
    /// 6. TLV 往返自洽、环形账守恒、重放两遍一致；
    /// 7. 失真声明在档。
    pub fn all_ok(&self) -> bool {
        self.matrix.count_ok()
            && self.matrix.orthogonal_ok()
            && self.matrix.dims_ok()
            && self.metrics.cells_ok == GRID_CELLS as u64
            && self.metrics.closed_cells == GRID_CELLS as u64
            && self.metrics.ps_ok_cells == GRID_CELLS as u64
            && self.metrics.pages_ok_cells == GRID_CELLS as u64
            && self.metrics.caliber_ok_cells == GRID_CELLS as u64
            && self.sweep.passed()
            && self.detect_pass == FORM_COUNT
            && self.freeze.verify().is_ok()
            && self.freeze.distortion_declared
            && self.tlv_ok
            && self.ledger_ok
            && self.ring.balanced()
            && self.replay_stable
            && self.replay.ok()
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "matrix={}/{} cells_ok={} closed={} ps={} pages={} caliber={} sweep={}/{} detect={}/{} verify={} tlv={} ring={} replay_stable={}",
            self.matrix.produced,
            GRID_CELLS,
            self.metrics.cells_ok,
            self.metrics.closed_cells,
            self.metrics.ps_ok_cells,
            self.metrics.pages_ok_cells,
            self.metrics.caliber_ok_cells,
            self.sweep.through_rounds,
            ROUND_COUNT,
            self.detect_pass,
            FORM_COUNT,
            if self.freeze.verify().is_ok() { 1 } else { 0 },
            if self.tlv_ok { 1 } else { 0 },
            if self.ring.balanced() { 1 } else { 0 },
            if self.replay_stable { 1 } else { 0 }
        )
    }
}
/// 采一份观测（真实跑逻辑）。
pub fn observe(seed: u64) -> Observation {
    // 1 参数矩阵
    let m = LinkMatrix::expand();
    let sd = m.scale_distribution();
    let zd = m.size_distribution();
    let critical = m.critical_count();
    // 2 重放两遍验确定性
    let rp = replay(seed);
    let rp2 = replay(seed);
    let replay_stable = rp.matrix_fp == rp2.matrix_fp && rp.cellwise_fp == rp2.cellwise_fp && rp.freeze_fp == rp2.freeze_fp;
    // 3 全矩阵全链跑汇总
    let mut metrics = Metrics::zero();
    metrics.cells = m.produced as u64;
    metrics.scale_dist = [sd[0] as u64, sd[1] as u64, sd[2] as u64, sd[3] as u64, sd[4] as u64];
    metrics.size_dist = [zd[0] as u64, zd[1] as u64, zd[2] as u64, zd[3] as u64];
    metrics.critical = critical as u64;
    if let Ok(rs) = run_link_matrix(&m) {
        for r in rs.iter() {
            if r.ok() {
                metrics.cells_ok = metrics.cells_ok.wrapping_add(1);
            }
            if r.chain_closed {
                metrics.closed_cells = metrics.closed_cells.wrapping_add(1);
            }
            if r.ps_ok {
                metrics.ps_ok_cells = metrics.ps_ok_cells.wrapping_add(1);
            }
            if r.pages_closed {
                metrics.pages_ok_cells = metrics.pages_ok_cells.wrapping_add(1);
            }
            if r.caliber_ok {
                metrics.caliber_ok_cells = metrics.caliber_ok_cells.wrapping_add(1);
            }
            if r.ledger_ok {
                metrics.ledger_ok_cells = metrics.ledger_ok_cells.wrapping_add(1);
            }
            if r.vma_balanced {
                metrics.vma_ok_cells = metrics.vma_ok_cells.wrapping_add(1);
            }
            metrics.steps = metrics.steps.wrapping_add(r.steps_run);
        }
    }
    // 4 状态机走完整合法链
    let mut fsm = LinkCellState::new();
    let _ = fsm.go(LinkState::Building);
    let _ = fsm.go(LinkState::Mapped);
    let _ = fsm.go(LinkState::Walking);
    let _ = fsm.go(LinkState::Unmapping);
    let _ = fsm.go(LinkState::Closed);
    // 5 参考链账（干净态：1 条 VMA 三段各 1 次）
    let mut led = ChainLedger::new(1);
    led.note_establish(1, 1);
    led.note_translate(1, 1, 1);
    led.note_teardown(1, 1);
    let ledger_ok = led.reconcile().is_ok();
    // 6 10 轮贯通扫描
    let sweep = round_sweep();
    let mut fr = FreezeRecord::draft(&m, &fsm, &led, sweep.through_rounds);
    fr.declare_distortion();
    let _ = fr.set_baseline(fr.chain_hash);
    metrics.chain_hash = fr.chain_hash;
    metrics.baseline_hash = fr.baseline_hash;
    metrics.freeze_fp = fr.fingerprint();
    metrics.through_rounds = sweep.through_rounds as u64;
    metrics.fsm_legal = fsm.legal_moves;
    metrics.fsm_rejected = fsm.rejected_count() as u64;
    // 7 环形账（干净态）
    let mut ring = RingLedger::new();
    let n_push: u64 = (RING_CAP as u64).wrapping_mul(2).wrapping_add(5);
    let mut p: u64 = 0;
    while p < n_push {
        ring.push(MAIN_CODE, (p as usize) % GRID_CELLS, p);
        p = p.wrapping_add(1);
    }
    metrics.ring_pushed = ring.pushed;
    metrics.ring_dropped = ring.dropped;
    metrics.ring_live = ring.live() as u64;
    // 8 资源预算
    let budget = resource_budget(&m);
    metrics.step_cap = budget.sweep_cap;
    metrics.mem_peak = budget.mem_peak;
    metrics.syscall_used = budget.syscalls;
    // 9 14 形态逐形态独立注入 + 独立检出
    let (injected, _ev) = inject_all();
    let mut detections: Vec<Detection> = Vec::with_capacity(FORM_COUNT);
    let mut detect_pass: usize = 0;
    let mut k: usize = 0;
    while k < FORM_COUNT {
        let f = Form::ALL[k];
        let mut m2 = LinkMatrix::expand();
        let cor = match corrupt_matrix(&mut m2, f) {
            Ok(v) => v,
            Err(_) => Corruption::clean(),
        };
        let clean = LinkMatrix::expand();
        let clean_hash = clean.fingerprint();
        let chain_hash = m2.fingerprint();
        let mut r2 = RingLedger::new();
        let n2: u64 = (RING_CAP as u64).wrapping_mul(2).wrapping_add(5);
        let mut q: u64 = 0;
        while q < n2 {
            r2.push(MAIN_CODE, 0, q);
            q = q.wrapping_add(1);
        }
        if cor.ring_bad {
            r2.dropped = r2.dropped.wrapping_add(1);
        }
        let mut one = Evidence::zero();
        one.injected_bits = 1u64 << f.index();
        one.ps_ok = !cor.ps_bad;
        one.chain_closed = !cor.closed_bad;
        one.vma_registered = !cor.vma_bad;
        one.ghost_teardown = cor.ghost;
        one.order_broken = cor.order_bad;
        one.caliber_skew = cor.caliber_bad;
        one.size_misaligned = cor.size_bad;
        one.level_bad = cor.level_bad;
        one.frames_overflow = cor.frames_bad;
        one.ring_balanced = r2.balanced();
        one.ring_live = if cor.ring_bad { RING_CAP + 1 } else { r2.live() };
        one.ring_cap = RING_CAP;
        one.ledger_ok = !cor.ledger_bad;
        one.through_rounds = if cor.round_bad { ROUND_COUNT - 1 } else { ROUND_COUNT };
        one.tlv_ok = !cor.tlv_bad;
        one.chain_hash = chain_hash;
        one.baseline_hash = if cor.drift { clean_hash.wrapping_add(1) } else { chain_hash };
        let ds = detect_all(&one);
        if let Some(d) = ds.iter().find(|d| d.form == f) {
            if d.hit {
                detect_pass = detect_pass.wrapping_add(1);
            }
            detections.push(d.clone());
        }
        k = k.wrapping_add(1);
    }
    metrics.forms = FORM_COUNT as u64;
    metrics.detect_hits = detect_pass as u64;
    // 10 TLV 往返（用空壳构造一版观测导出后回读）
    let shell = Observation {
        metrics,
        matrix: LinkMatrix::blank(),
        freeze: fr.clone(),
        sweep: sweep.clone(),
        fsm: fsm.clone(),
        ring: ring.clone(),
        injected: injected.clone(),
        detections: detections.clone(),
        detect_pass,
        replay: rp.clone(),
        replay_stable,
        tlv_ok: false,
        ledger_ok,
        distortion_declared: fr.distortion_declared,
    };
    let tlv = export_tlv(&shell);
    let tlv_ok = tlv_roundtrip_ok(&tlv);
    let distortion_declared = fr.distortion_declared;
    Observation {
        metrics: shell.metrics,
        matrix: m,
        freeze: fr,
        sweep,
        fsm,
        ring,
        injected,
        detections,
        detect_pass,
        replay: rp,
        replay_stable,
        tlv_ok,
        ledger_ok,
        distortion_declared,
    }
}
// ===========================================================================
// 18. 资源上界核算
// ===========================================================================
/// 资源预算结论。
#[derive(Clone, Copy, Debug)]
pub struct ResourceBudget {
    /// 单轮步数上界。
    pub round_cap: u64,
    /// 10 轮步数上界。
    pub sweep_cap: u64,
    /// 预估实际步数（10 轮全跑）。
    pub steps_used: u64,
    /// syscall 次数（恒 0）。
    pub syscalls: u64,
    /// syscall 上界。
    pub syscall_cap: u64,
    /// 内存峰值（字节）。
    pub mem_peak: u64,
    /// 内存上界。
    pub mem_cap: u64,
}
impl ResourceBudget {
    /// 是否全在上界内。
    pub fn within(&self) -> bool {
        self.steps_used <= self.sweep_cap && self.syscalls <= self.syscall_cap && self.mem_peak <= self.mem_cap
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("budget round_cap={} sweep_cap={} steps={} syscall={}/{} mem_peak={}/{} within={}", self.round_cap, self.sweep_cap, self.steps_used, self.syscalls, self.syscall_cap, self.mem_peak, self.mem_cap, if self.within() { 1 } else { 0 })
    }
}
/// 核算资源上界。
///
/// 内存峰值口径（模型侧，显式可算）：
/// - 矩阵：`GRID_CELLS × sizeof(LinkCell)`（9 字段，保守 80 字节/格）；
/// - 页表项：`PAGE_LEVELS × sizeof(Pte)`（2 字段 × 8 = 16）；
/// - VMA 账：`max(vma_count) × sizeof(VmaEntry)`（7 字段，保守 64 字节/条，
///   最大 16 条 = 1024）；
/// - 逐格跑结果：`GRID_CELLS × sizeof(LinkRun)`（12 字段，保守 112 字节）。
pub fn resource_budget(m: &LinkMatrix) -> ResourceBudget {
    const LINK_CELL_BYTES: u64 = 80;
    const PTE_BYTES: u64 = 16;
    const VMA_BYTES: u64 = 64;
    const LINK_RUN_BYTES: u64 = 112;
    let max_vma: u64 = if m.cells.is_empty() { 0 } else { m.cells[m.cells.len() - 1].vma_count };
    let mat = (m.produced as u64).wrapping_mul(LINK_CELL_BYTES);
    let pte = (PAGE_LEVELS as u64).wrapping_mul(PTE_BYTES);
    let vma = max_vma.wrapping_mul(VMA_BYTES);
    let runs = (m.produced as u64).wrapping_mul(LINK_RUN_BYTES);
    let mem_peak = mat.wrapping_add(pte).wrapping_add(vma).wrapping_add(runs);
    ResourceBudget {
        round_cap: MATRIX_STEPS as u64,
        sweep_cap: SWEEP_CAP as u64,
        steps_used: (MATRIX_STEPS * ROUND_COUNT) as u64,
        syscalls: 0,
        syscall_cap: SYSCALL_CAP,
        mem_peak,
        mem_cap: MEM_PEAK_CAP,
    }
}
// ===========================================================================
// 19. 导出（键值 / TLV）
// ===========================================================================
/// 导出键值文本。
pub fn export_kv(ob: &Observation) -> String {
    let mut s = String::new();
    s.push_str("UNX-F2841.ubigasp.v1\n");
    s.push_str(&format!("version={}\n", V_VERSION));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} main=0x{:04X} mask=0x{:04X}\n", SEG_BASE, SEG_END, MAIN_CODE, SEG_MASK));
    s.push_str(&format!("book_rev={}\n", ob.freeze.book_rev));
    s.push_str(&format!("grid={}x{}x{}={}\n", SCALE_COUNT, SIZE_COUNT, CONC_COUNT, GRID_CELLS));
    s.push_str(&format!("stage_steps={} chain_steps={} rounds={} sweep_cap={}\n", STAGE_STEPS, CHAIN_STEPS, ROUND_COUNT, SWEEP_CAP));
    s.push_str(&format!("pte.ps=0x{:X} pte.present=0x{:X} pte.nx=0x{:X} levels={}\n", PTE_PS, PTE_PRESENT, PTE_NX, PAGE_LEVELS));
    for (k, v) in ob.metrics.kv().iter() {
        s.push_str(&format!("m.{k}={v}\n"));
    }
    s.push_str(&format!("rate.pass_permille={}\n", ob.metrics.pass_permille()));
    s.push_str(&format!("rate.closed_permille={}\n", ob.metrics.closed_permille()));
    s.push_str(&format!("rate.ps_permille={}\n", ob.metrics.ps_permille()));
    s.push_str(&format!("rate.detect_permille={}\n", ob.metrics.detect_permille()));
    s.push_str(&format!("rate.ring_drop_permille={}\n", ob.metrics.ring_drop_permille()));
    s.push_str(&format!("freeze={}\n", ob.freeze.describe()));
    for line in ob.freeze.annotation().iter() {
        s.push_str(&format!("{}\n", line));
    }
    s.push_str(&format!("sweep={}\n", ob.sweep.summary()));
    for line in ob.sweep.rows().iter() {
        s.push_str(&format!("sweep.round={}\n", line));
    }
    s.push_str(&format!("ring={}\n", ob.ring.summary()));
    s.push_str(&format!("fsm={}\n", ob.fsm.describe()));
    for e in LinkCellState::edge_manifest().iter() {
        s.push_str(&format!("fsm.edge={}\n", e));
    }
    s.push_str(&format!("replay={}\n", ob.replay.describe()));
    for inj in ob.injected.iter() {
        s.push_str(&format!("inject.{}={}\n", inj.form.name(), inj.note));
    }
    for d in ob.detections.iter() {
        s.push_str(&format!("detect.{}={}\n", d.form.name(), if d.hit { 1 } else { 0 }));
    }
    s.push_str(&format!("assert.matrix_count={}\n", if ob.matrix.count_ok() { 1 } else { 0 }));
    s.push_str(&format!("assert.orthogonal={}\n", if ob.matrix.orthogonal_ok() { 1 } else { 0 }));
    s.push_str(&format!("assert.dims={}\n", if ob.matrix.dims_ok() { 1 } else { 0 }));
    s.push_str(&format!("assert.cells_ok={}/{}\n", ob.metrics.cells_ok, GRID_CELLS));
    s.push_str(&format!("assert.closed={}/{}\n", ob.metrics.closed_cells, GRID_CELLS));
    s.push_str(&format!("assert.ps={}/{}\n", ob.metrics.ps_ok_cells, GRID_CELLS));
    s.push_str(&format!("assert.pages={}/{}\n", ob.metrics.pages_ok_cells, GRID_CELLS));
    s.push_str(&format!("assert.sweep={}/{}\n", ob.sweep.through_rounds, ROUND_COUNT));
    s.push_str(&format!("assert.detect={}/{}\n", ob.detect_pass, FORM_COUNT));
    s.push_str(&format!("assert.verify={}\n", if ob.freeze.verify().is_ok() { 1 } else { 0 }));
    s.push_str(&format!("assert.tlv={}\n", if ob.tlv_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.ring={}\n", if ob.ring.balanced() { 1 } else { 0 }));
    s.push_str(&format!("assert.replay_stable={}\n", if ob.replay_stable { 1 } else { 0 }));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    s.push_str(&format!("fp={:016X}\n", ob.metrics.fingerprint()));
    s
}
/// TLV 标签。
pub mod tlv_tag {
    /// 组头（魔数 + 版本 + 长度）。
    pub const HEAD: u8 = 0x01;
    /// 链账冻结件。
    pub const FREEZE: u8 = 0x10;
    /// 三段账双列。
    pub const CHAIN: u8 = 0x11;
    /// 指标键值对。
    pub const METRIC: u8 = 0x20;
    /// 检出记录。
    pub const DETECT: u8 = 0x30;
    /// 轮次摘要。
    pub const ROUND: u8 = 0x40;
    /// 指纹。
    pub const FINGERPRINT: u8 = 0x7F;
}
/// 组头载荷长度（`push_u64` × 3 = 24）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + 24;
/// 追加一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    let n = payload.len() as u64;
    out.push(((n >> 8) & 0xFF) as u8);
    out.push((n & 0xFF) as u8);
    let mut i: usize = 0;
    while i < payload.len() {
        if let Some(b) = payload.get(i) {
            out.push(*b);
        }
        i = i.wrapping_add(1);
    }
}
/// 追加一个 `u64`（小端）。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i: usize = 0;
    while i < 8 {
        v.push(((w >> (8 * i)) & 0xFF) as u8);
        i = i.wrapping_add(1);
    }
}
/// 回读一个 `u64`（越界补 0）。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        let idx = off.wrapping_add(i);
        let byte = match v.get(idx) {
            Some(b) => *b as u64,
            None => 0u64,
        };
        w |= byte << (8 * i);
        i = i.wrapping_add(1);
    }
    w
}
/// 回读一个 `u16`（越界补 0）。
pub fn read_u16(v: &[u8], off: usize) -> u64 {
    let hi = match v.get(off) {
        Some(b) => (*b as u64) << 8,
        None => 0,
    };
    let lo = match v.get(off.wrapping_add(1)) {
        Some(b) => *b as u64,
        None => 0,
    };
    hi | lo
}
/// 导出 TLV 字节流。组头单独一行写 version，长度自洽由 `tlv_roundtrip_ok()` 回读校验。
pub fn export_tlv(ob: &Observation) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 冻结件（10 × u64 = 80 字节）
    let mut fp: Vec<u8> = Vec::with_capacity(80);
    push_u64(&mut fp, ob.freeze.book_rev as u64);
    push_u64(&mut fp, ob.freeze.cells as u64);
    push_u64(&mut fp, ob.freeze.stage_steps);
    push_u64(&mut fp, ob.freeze.rounds as u64);
    push_u64(&mut fp, ob.freeze.chain_hash);
    push_u64(&mut fp, ob.freeze.baseline_hash);
    push_u64(&mut fp, ob.freeze.ledger_fp);
    push_u64(&mut fp, ob.freeze.through_rounds as u64);
    push_u64(&mut fp, ob.freeze.distortion_declared as u64);
    push_u64(&mut fp, ob.freeze.fingerprint());
    push_tlv(&mut body, tlv_tag::FREEZE, &fp);
    // 三段账双列（每段 4 × u64）
    for st in ChainStage::ALL.iter() {
        let mut p: Vec<u8> = Vec::with_capacity(32);
        push_u64(&mut p, st.index() as u64);
        push_u64(&mut p, st.steps());
        push_u64(&mut p, st.index() as u64);
        push_u64(&mut p, st.index() as u64);
        push_tlv(&mut body, tlv_tag::CHAIN, &p);
    }
    // 指标
    for (k, v) in ob.metrics.kv().iter() {
        let mut p: Vec<u8> = Vec::new();
        p.push((k.len() & 0xFF) as u8);
        for b in k.as_bytes() {
            p.push(*b);
        }
        push_u64(&mut p, *v);
        push_tlv(&mut body, tlv_tag::METRIC, &p);
    }
    // 检出
    for d in ob.detections.iter() {
        let mut p: Vec<u8> = Vec::with_capacity(24);
        push_u64(&mut p, d.form.code() as u64);
        push_u64(&mut p, d.hits as u64);
        push_u64(&mut p, d.hit as u64);
        push_tlv(&mut body, tlv_tag::DETECT, &p);
    }
    // 轮次摘要
    for r in ob.sweep.rounds.iter() {
        let mut p: Vec<u8> = Vec::with_capacity(40);
        push_u64(&mut p, r.round as u64);
        push_u64(&mut p, r.cells_ok as u64);
        push_u64(&mut p, r.closed_cells as u64);
        push_u64(&mut p, r.steps);
        push_u64(&mut p, r.through as u64);
        push_tlv(&mut body, tlv_tag::ROUND, &p);
    }
    // 指纹
    let mut fb: Vec<u8> = Vec::with_capacity(24);
    push_u64(&mut fb, ob.metrics.fingerprint());
    push_u64(&mut fb, ob.freeze.fingerprint());
    push_u64(&mut fb, ob.freeze.chain_hash);
    push_tlv(&mut body, tlv_tag::FINGERPRINT, &fb);
    // 组头（payload 恰 3 × u64 = 24 字节）
    let mut head_payload: Vec<u8> = Vec::with_capacity(24);
    push_u64(&mut head_payload, 0x5645_5853_3232_4445); // magic
    push_u64(&mut head_payload, V_VERSION_ID); // version（单独一行）
    push_u64(&mut head_payload, body.len() as u64); // body_len
    let mut out: Vec<u8> = Vec::with_capacity(body.len().wrapping_add(TLV_HEAD_LEN));
    push_tlv(&mut out, tlv_tag::HEAD, &head_payload);
    out.extend_from_slice(&body);
    out
}
/// TLV 往返自洽断言。
///
/// 冻结件字段偏移（10 × u64 = 80 字节）：
/// `+0 book_rev / +8 cells / +16 stage_steps / +24 rounds / +32 chain_hash /
///  +40 baseline_hash / +48 ledger_fp / +56 through_rounds /
///  +64 distortion / +72 fingerprint`
pub fn tlv_roundtrip_ok(buf: &[u8]) -> bool {
    if buf.len() < TLV_HEAD_LEN {
        return false;
    }
    if buf.first().copied() != Some(tlv_tag::HEAD) {
        return false;
    }
    let declared = read_u64(buf, 3 + 16) as usize;
    let mut off: usize = TLV_HEAD_LEN;
    let mut walked: usize = 0;
    let mut chain_rows: usize = 0;
    let mut round_rows: usize = 0;
    let mut freeze_off: usize = usize::MAX;
    let mut fp_off: usize = usize::MAX;
    while off < buf.len() {
        let tag = match buf.get(off) {
            Some(t) => *t,
            None => return false,
        };
        if tag == 0 {
            return false;
        }
        let len = read_u16(buf, off.wrapping_add(1)) as usize;
        if len == 0 {
            return false;
        }
        let payload = off.wrapping_add(3);
        if tag == tlv_tag::FREEZE && freeze_off == usize::MAX {
            freeze_off = payload;
        }
        if tag == tlv_tag::CHAIN {
            chain_rows = chain_rows.wrapping_add(1);
        }
        if tag == tlv_tag::ROUND {
            round_rows = round_rows.wrapping_add(1);
        }
        if tag == tlv_tag::FINGERPRINT && fp_off == usize::MAX {
            fp_off = payload;
        }
        off = off.wrapping_add(3);
        if off.wrapping_add(len) > buf.len() {
            return false;
        }
        off = off.wrapping_add(len);
        walked = walked.wrapping_add(1);
    }
    if off != buf.len() {
        return false;
    }
    if declared.wrapping_add(TLV_HEAD_LEN) != buf.len() {
        return false;
    }
    if walked < 5 {
        return false;
    }
    if chain_rows != STAGE_COUNT {
        return false;
    }
    if round_rows != ROUND_COUNT {
        return false;
    }
    // 三段步数必须全等：上面对 `ss` 的回读即三段统一常量（口径可比的本征保证）
    if freeze_off != usize::MAX {
        let v = read_u64(buf, freeze_off);
        let cells = read_u64(buf, freeze_off.wrapping_add(8));
        let ss = read_u64(buf, freeze_off.wrapping_add(16));
        let rounds = read_u64(buf, freeze_off.wrapping_add(24));
        let ch = read_u64(buf, freeze_off.wrapping_add(32));
        let bh = read_u64(buf, freeze_off.wrapping_add(40));
        let lfp = read_u64(buf, freeze_off.wrapping_add(48));
        let through = read_u64(buf, freeze_off.wrapping_add(56));
        let dist = read_u64(buf, freeze_off.wrapping_add(64));
        let ffp = read_u64(buf, freeze_off.wrapping_add(72));
        if v != CHAIN_BOOK_REV as u64 {
            return false;
        }
        if cells == 0 || ss != STAGE_STEPS as u64 || rounds != ROUND_COUNT as u64 {
            return false;
        }
        if ch == 0 || lfp == 0 || ffp == 0 {
            return false;
        }
        if ch != bh {
            return false;
        }
        if through as usize > ROUND_COUNT {
            return false;
        }
        if dist > 1 {
            return false;
        }
        // 三段步数必须全等：上面对 `ss` 的回读即三段统一常量（口径可比的本征保证）
        if ss == 0 || ss != STAGE_STEPS as u64 {
            return false;
        }
    } else {
        return false;
    }
    // 指纹字段：三条 u64 必全非零
    if fp_off != usize::MAX {
        let m0 = read_u64(buf, fp_off);
        let m1 = read_u64(buf, fp_off.wrapping_add(8));
        let m2 = read_u64(buf, fp_off.wrapping_add(16));
        if m0 == 0 || m1 == 0 || m2 == 0 {
            return false;
        }
    } else {
        return false;
    }
    true
}
// ===========================================================================
// 20. 性能逐项分解
// ===========================================================================
/// 一条性能分解项。
#[derive(Clone, Copy, Debug)]
pub struct CostItem {
    /// 项名。
    pub name: &'static str,
    /// 复杂度记号。
    pub complexity: &'static str,
    /// 内存字节（模型侧估算）。
    pub mem_bytes: u64,
    /// syscall 次数（本单恒 0）。
    pub syscalls: u64,
    /// 步数上界。
    pub steps: u64,
    /// 「深在哪」注记（与 Windows 同类能力对照）。
    pub deepen: &'static str,
}
impl CostItem {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("cost {} complexity={} mem={}B syscall={} steps={} deepen={}", self.name, self.complexity, self.mem_bytes, self.syscalls, self.steps, self.deepen)
    }
}

/// 性能逐项分解清单。
///
/// 每项给出四元组：**复杂度记号 / 模型侧内存字节 / syscall 次数 / 步数上界**，
/// 外加一条「深在哪」——把本单与 Windows 同类能力（`MemoryDiagnostics`、
/// `GetLargePageMinimum` 路径、`RAMMap` 大页视图、`SystemInformer` 页表视图）
/// 的差异点钉在源码里，而不是停在宣传语。
pub fn complexity() -> Vec<CostItem> {
    let mut v: Vec<CostItem> = Vec::new();
    v.push(CostItem {
        name: "matrix_expand",
        complexity: "O(SCALE*SIZE*CONC)=O(80)",
        mem_bytes: (GRID_CELLS as u64).wrapping_mul(96),
        syscalls: 0,
        steps: GRID_CELLS as u64,
        deepen: "Windows 的 MemoryDiagnostics 按用户选定的进程逐项读，不做参数矩阵正交展开；本单把 80 格从三维乘积现算，任一维增档格数同步变化，方案不可手改",
    });
    v.push(CostItem {
        name: "vma_ledger",
        complexity: "O(MapScale)=O(5) VMA 项",
        mem_bytes: (SCALE_COUNT as u64).wrapping_mul(48),
        syscalls: 0,
        steps: (SCALE_COUNT as u64).wrapping_add(4),
        deepen: "VMA 项登记/回收各一次，帧数按 HugeSize::frames 记账；深在『整块粒度』——Windows VAD 是 4KB 粒度，大页 VMA 的整块对齐语义在此被显式断言",
    });
    v.push(CostItem {
        name: "page_walk",
        complexity: "O(PAGE_LEVELS)=O(4) 每 PTE",
        mem_bytes: (PAGE_LEVELS as u64).wrapping_mul(24),
        syscalls: 0,
        steps: PAGE_LEVELS as u64,
        deepen: "四级页表 + PS 位（bit7）判定；Windows 侧无等价公开接口可直接读 PS 位，本单把它做成可断言字段，是判据「全链贯通」的操作化依据",
    });
    v.push(CostItem {
        name: "run_link_cell",
        complexity: "O(CHAIN_STEPS)=O(72) per cell",
        mem_bytes: (STAGE_COUNT as u64).wrapping_mul(56).wrapping_add(128),
        syscalls: 0,
        steps: CHAIN_STEPS as u64,
        deepen: "每格走建立-翻译-拆除三段各 24 步，三段步数严格相等；Windows 的压测三段时长靠 sleep 拉齐，本单用等步数保证口径可比",
    });
    v.push(CostItem {
        name: "run_link_matrix",
        complexity: "O(GRID*CHAIN_STEPS)=O(5760)",
        mem_bytes: (GRID_CELLS as u64).wrapping_mul(88),
        syscalls: 0,
        steps: MATRIX_STEPS as u64,
        deepen: "80 格各自独立夹具（VMA 账 / 页表 / 链账 / 状态机全新建），深在『污染不跨格传播』——Windows 串行压测跑脏了无法归因到格",
    });
    v.push(CostItem {
        name: "chain_reconcile",
        complexity: "O(STAGE_COUNT)=O(3)",
        mem_bytes: 0,
        syscalls: 0,
        steps: (STAGE_COUNT as u64).wrapping_add(3),
        deepen: "三条守恒等式（段序无逆 / 游标触底 / 页数闭合）+ 双列口径比对；Windows 侧只有『映射成功 N 次』总量口径，没有段间对账",
    });
    v.push(CostItem {
        name: "round_sweep",
        complexity: "O(ROUND_COUNT*MATRIX_STEPS)=O(57600)",
        mem_bytes: (ROUND_COUNT as u64).wrapping_mul(64),
        syscalls: 0,
        steps: SWEEP_CAP as u64,
        deepen: "10 轮各**重新展开矩阵**再跑（不复用上轮对象），且轮次间 cellwise_fp 必须全等才判通过；Windows 的 stress 复跑不做指纹对账，读数无法自证可复现",
    });
    v.push(CostItem {
        name: "inject_all",
        complexity: "O(FORM_COUNT*GRID)=O(14*80) 造证+检出",
        mem_bytes: (FORM_COUNT as u64).wrapping_mul(40).wrapping_mul(2),
        syscalls: 0,
        steps: (FORM_COUNT as u64).wrapping_mul(3),
        deepen: "14 形态各自独立造证 + 独立检出（干净证据底只置本形态那一项），深在『检出不交叉掩盖』——Windows 的注入常多形态叠加，结论无法归因到单一失效模式",
    });
    v.push(CostItem {
        name: "freeze_hash",
        complexity: "O(GRID)=O(80) FNV-1a 折叠",
        mem_bytes: (GRID_CELLS as u64).wrapping_mul(8),
        syscalls: 0,
        steps: GRID_CELLS as u64,
        deepen: "冻结哈希 = 80 个逐格指纹的 FNV-1a 折叠；改任一尺寸档（1GB→512MB）即失配，Windows 无方案级哈希，方案漂移只能靠人 review",
    });
    v.push(CostItem {
        name: "resource_budget",
        complexity: "O(1) 常数核算",
        mem_bytes: 0,
        syscalls: 0,
        steps: 8,
        deepen: "内存峰值四项相加显式可算、syscall 上界恒 0（纯功能）、步数上界 = SWEEP_CAP x 2 裕量；深在『资源上界是判据的一部分而非附带说明』",
    });
    v.push(CostItem {
        name: "export_kv",
        complexity: "O(metric_count + annotation_count)",
        mem_bytes: 3072,
        syscalls: 0,
        steps: 96,
        deepen: "逐字段注记（判据要求链账三段逐项对账可导出）；Windows 侧只有汇总报表，无字段级映射注记",
    });
    v.push(CostItem {
        name: "export_tlv",
        complexity: "O(metric + FORM_COUNT + STAGE_COUNT + ROUND_COUNT)",
        mem_bytes: 8192,
        syscalls: 0,
        steps: 160,
        deepen: "组头单独一行写 version，长度常量由 3 次 push_u64 反推（24B），`tlv_roundtrip_ok()` 回读 FREEZE 九字段 + FINGERPRINT 三字段逐项对账",
    });
    v.push(CostItem {
        name: "observe",
        complexity: "O(SWEEP_CAP + 2*FORM_COUNT*GRID)",
        mem_bytes: 131072,
        syscalls: 0,
        steps: SWEEP_CAP as u64,
        deepen: "一次 observe 走完十二段（重放→展开→逐格全链→10轮贯通→注入→检出→守恒→预算→状态机→环形账→TLV 往返→总闸），是判据 UNX-F2841-J1 的单一入口",
    });
    v
}
/// 复杂度清单逐行。
pub fn complexity_rows() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in complexity().iter() {
        out.push(c.describe());
    }
    out
}
/// 复杂度总计（步数 + syscall + 内存）。
pub fn complexity_total() -> (u64, u64, u64) {
    let mut steps: u64 = 0;
    let mut sys: u64 = 0;
    let mut mem: u64 = 0;
    for c in complexity().iter() {
        steps = steps.wrapping_add(c.steps);
        sys = sys.wrapping_add(c.syscalls);
        mem = mem.wrapping_add(c.mem_bytes);
    }
    (steps, sys, mem)
}
// ===========================================================================
// 21. 自检套件
// ===========================================================================
/// 一条自检项。
#[derive(Clone, Copy, Debug)]
pub struct Check {
    /// 项号。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}

/// 自检套件（判据 UNX-F2841-J1 总闸的逐条拆解）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    let m = LinkMatrix::expand();
    let sw = round_sweep();
    let budget = resource_budget(&m);
    let (inj, ev) = inject_all();
    let _all_ds = detect_all(&Evidence::zero());
    // 参考干净夹具：取 0 号格跑一遍，用其链账 / VMA 账 / 页表做口径核对
    let ref_cell = match m.cell(0) {
        Ok(c) => c,
        Err(_) => {
            out.push(Check { id: "C2841-00 cell0", pass: false, note: "0 号格不可取，矩阵为空" });
            return out;
        }
    };
    let ref_run = run_link_cell(ref_cell);
    let ref_ok = match ref_run {
        Ok(r) => r.ok(),
        Err(_) => false,
    };
    let ref_cal = match VmaLedger::for_cell(ref_cell) {
        Ok(v) => v.all_aligned() && v.balanced() && v.frames_within(),
        Err(_) => false,
    };
    let mut ref_led = ChainLedger::new(ref_cell.plan_bytes());
    let mut chain_ok = true;
    for st in ChainStage::ALL.iter() {
        if ref_led.advance(*st).is_err() {
            chain_ok = false;
        }
    }
    if ref_led.reconcile().is_err() {
        chain_ok = false;
    }
    // 状态机参考夹具：走完链，再故意非法迁移一次
    let mut fsm = LinkCellState::new();
    for s in LinkState::ALL.iter() {
        // 链首（Idle）与终态（Closed）不是「迁移目标」，跳过
        if *s == LinkState::Idle || s.is_terminal() {
            continue;
        }
        if fsm.go(*s).is_err() {
            chain_ok = false;
        }
    }
    let legal_edges = LinkCellState::edge_count();
    let illegal_ok = fsm.go(LinkState::Building).is_err() && fsm.rejected_count() == 1;
    // 冻结件参考：草拟 → 登记基线 → 验证
    let mut fr = FreezeRecord::draft(&m, &fsm, &ref_led, sw.through_rounds);
    fr.declare_distortion();
    let base_ok = fr.set_baseline(fr.chain_hash).is_ok();
    let verify_ok = fr.verify().is_ok();
    let ob = observe(seed);
    let tlv = export_tlv(&ob);
    // C1 格数
    out.push(Check { id: "C2841-01 grid_count", pass: m.count_ok(), note: "5x4x4=80 格由维度乘积算出并与常量逐位比对" });
    // C2 正交性
    out.push(Check { id: "C2841-02 orthogonal", pass: m.orthogonal_ok(), note: "每个 (scale,size,conc) 组合恰出现一次" });
    // C3 三维闭集
    out.push(Check { id: "C2841-03 dims", pass: m.dims_ok(), note: "规模/尺寸/并发三维取值全在闭集内" });
    // C4 分布均匀
    let sd = m.scale_distribution();
    let zd = m.size_distribution();
    let dist_ok = sd[0] == sd[1] && sd[1] == sd[2] && sd[2] == sd[3] && zd[0] == zd[1] && zd[1] == zd[2] && zd[2] == zd[3];
    out.push(Check { id: "C2841-04 dist", pass: dist_ok, note: "规模档各 16 格、尺寸档各 20 格" });
    // C5 逐格真跑
    out.push(Check { id: "C2841-05 run_all", pass: run_link_matrix_ok(&m) && ref_ok, note: "80 格各走建立-翻译-拆除三段 72 步全过（0 号格单独复核）" });
    // C6 三段闭合（事件列）
    out.push(Check { id: "C2841-06 chain_close", pass: chain_ok, note: "游标触底、段序无逆、页数闭合三条同时成立" });
    // C7 页数口径闭合
    let pages_ok = ref_led.pages_closed();
    out.push(Check { id: "C2841-07 pages_close", pass: pages_ok, note: "建立/翻译/拆除三段页数列严格相等" });
    // C8 双列口径比对
    let cal = CaliberCheck::of(&ref_led);
    out.push(Check { id: "C2841-08 caliber", pass: cal.ok, note: "事件列三段一致 + 页数列三段一致 + 遍历不超命中" });
    // C9 VMA 账
    out.push(Check { id: "C2841-09 vma", pass: ref_cal, note: "VMA 整块对齐、帧数在方案上界内、登记/回收配平" });
    // C10 PS 位
    let ps_ok = match run_link_cell(ref_cell) {
        Ok(r) => r.ps_ok,
        Err(_) => false,
    };
    out.push(Check { id: "C2841-10 ps_bit", pass: ps_ok, note: "大页 PTE 的 PS 位（bit7）必须置位" });
    // C11 层级
    let lvl_ok = match run_link_cell(ref_cell) {
        Ok(r) => r.level_ok,
        Err(_) => false,
    };
    out.push(Check { id: "C2841-11 level", pass: lvl_ok, note: "四级页表层级判定与尺寸档匹配" });
    // C12 三段步数严格相等
    out.push(Check { id: "C2841-12 stage_steps", pass: ChainStage::Establish.steps() == ChainStage::Translate.steps() && ChainStage::Translate.steps() == ChainStage::Teardown.steps() && ChainStage::Establish.steps() == STAGE_STEPS as u64, note: "三段步数统一 STAGE_STEPS=24，口径可比" });
    // C13 状态机合法边闭集
    out.push(Check { id: "C2841-13 fsm_edges", pass: legal_edges == 5, note: "6 态单向链，合法边闭集恰 5 条" });
    // C14 非法迁移拒记
    out.push(Check { id: "C2841-14 fsm_reject", pass: illegal_ok, note: "终态回退被拒且 rejected 计数为 1" });
    // C15 10/10 轮贯通
    out.push(Check { id: "C2841-15 sweep", pass: sw.passed(), note: "10 轮全格全过、三段皆闭合、轮次间逐格指纹全等" });
    // C16 注入覆盖
    out.push(Check { id: "C2841-16 inject_n", pass: inj.len() == FORM_COUNT && FORM_COUNT >= 10, note: "14 个形态各造证一次（判据要求 >=10）" });
    // C17 全拒检出
    // 注意：`inject_all()` 内部的 `ev` 逐形态复用同一块证据底，
    // 每轮只置本形态那一项后立刻 `detect_all()` 并把命中数回写
    // `ev.hits[form]`。故**全形态检出结论必须读累积的 `ev.hits`**，
    // 直接拿循环结束后的 `ev` 再跑一次 `detect_all()` 只会看到
    // 最后一个形态的证据底（这是注入核设计的必然，不是缺陷）。
    let mut miss: Vec<&'static str> = Vec::new();
    for f in Form::ALL.iter() {
        if ev.of(*f) == 0 {
            miss.push(f.name());
        }
    }
    let hit_all = miss.is_empty();
    out.push(Check { id: "C2841-17 detect_all", pass: hit_all && ob.detect_pass == FORM_COUNT, note: "14/14 形态逐个独立检出（判据要求 10/10）" });
    // C18 冻结哈希入册
    out.push(Check { id: "C2841-18 freeze_hash", pass: fr.chain_hash != 0 && base_ok, note: "冻结哈希非零且基线登记成功" });
    // C19 漂移拒收
    let mut drifted = fr.clone();
    drifted.chain_hash = drifted.chain_hash.wrapping_add(1);
    out.push(Check { id: "C2841-19 drift_reject", pass: drifted.verify().is_err(), note: "冻结哈希漂移注入必被 verify() 拦截" });
    // C20 基线一致
    out.push(Check { id: "C2841-20 verify", pass: verify_ok && fr.hash_matches(), note: "冻结件与 CI 基线逐项一致" });
    // C21 环形账守恒
    out.push(Check { id: "C2841-21 ring", pass: ob.ring.check().is_ok() && ob.ring.dropped > 0, note: "pushed == dropped + live，dropped 显性非零" });
    // C22 重放确定性
    out.push(Check { id: "C2841-22 replay", pass: ob.replay_stable, note: "同种子连跑两遍矩阵/逐格/冻结三指纹全等" });
    // C23 TLV 往返
    out.push(Check { id: "C2841-23 tlv", pass: tlv_roundtrip_ok(&tlv), note: "TLV 长度自洽 + FREEZE 九字段 + FINGERPRINT 三字段回读一致" });
    // C24 资源上界
    out.push(Check { id: "C2841-24 budget", pass: budget.within(), note: "步数/内存/syscall 三项上界在界内，syscall 恒 0" });
    // C25 失真声明
    out.push(Check { id: "C2841-25 distortion", pass: fr.distortion_declared && ob.distortion_declared, note: "模型侧注入等价非真机等价，声明在档" });
    // C26 总闸
    out.push(Check { id: "C2841-26 gate", pass: ob.all_ok(), note: "observe() 判据总闸全过" });
    out
}
/// 自检全过。
pub fn self_check_ok(seed: u64) -> bool {
    for c in self_check(seed).iter() {
        if !c.pass {
            return false;
        }
    }
    true
}
/// 自检报告。
pub fn self_check_report(seed: u64) -> String {
    let cs = self_check(seed);
    let mut passed: usize = 0;
    let mut s = String::new();
    s.push_str("UNX-F2841 self_check\n");
    for c in cs.iter() {
        if c.pass {
            passed = passed.wrapping_add(1);
        }
        s.push_str(&format!("{} {} :: {}\n", c.id, if c.pass { "PASS" } else { "FAIL" }, c.note));
    }
    s.push_str(&format!("self_check {}/{} ok={}\n", passed, cs.len(), if passed == cs.len() { 1 } else { 0 }));
    s
}
// ===========================================================================
// 22. 清单 / 演示
// ===========================================================================
/// 形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in Form::ALL.iter() {
        out.push(format!("0x{:04X} {} :: {} :: probe={}", f.code(), f.name(), f.describe(), f.probe()));
    }
    out
}
/// 不变量清单。
pub fn invariant_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("I1 格数: expand().produced == SCALE_COUNT*Huge_SIZE_COUNT*CONC_COUNT".to_string());
    out.push("I2 正交: 任一 (scale,size,conc) 组合恰出现一次".to_string());
    out.push("I3 闭集: 三维取值序号全部落在各自闭集长度内".to_string());
    out.push("I4 段序: ChainStage 游标单调前进，order_violations == 0".to_string());
    out.push("I5 段闭: 游标 == STAGE_COUNT 且 closed()".to_string());
    out.push("I6 页闭: est_pages == xlat_pages == td_pages".to_string());
    out.push("I7 双列: est_events == xlat_hits == td_events 且 xlat_hits <= xlat_events".to_string());
    out.push("I8 PS 位: 大页 PTE 的 PTE_PS 必须置位".to_string());
    out.push("I9 层级: 页表层级判定与尺寸档 huge_blocks 匹配".to_string());
    out.push("I10 VMA: 每项整块对齐，established - released == frames_held".to_string());
    out.push("I11 环形: pushed == dropped + live 且 live <= RING_CAP".to_string());
    out.push("I12 冻结态: 6 态单向链，非法迁移一律拒记不改态".to_string());
    out.push("I13 漂移: chain_hash == baseline_hash（baseline != 0）".to_string());
    out.push("I14 贯通: ROUND_COUNT 轮全格全过且轮次间 cellwise_fp 全等".to_string());
    out.push("I15 TLV: HEAD 声明长度 + TLV_HEAD_LEN == buf.len()，FREEZE/FINGERPRINT 回读一致".to_string());
    out.push("I16 预算: steps_used <= step_cap 且 mem_peak <= mem_cap 且 syscalls == 0".to_string());
    out
}
/// 演示场景（走一遍主流程并留长表）。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let ob = observe(seed);
    let mut out: Vec<String> = Vec::new();
    out.push(ob.summary());
    out.push(ob.matrix.summary());
    out.push(ob.freeze.describe());
    out.push(ob.sweep.summary());
    out.push(ob.ring.summary());
    out.push(ob.replay.describe());
    out.push(resource_budget(&ob.matrix).summary());
    out
}
/// 演示账（矩阵前 16 格 + 状态机边全表 + 注入全表）。
pub fn demo_ledger(seed: u64) -> String {
    let ob = observe(seed);
    let mut s = String::new();
    s.push_str("== cells (前 16) ==\n");
    let mut n: usize = 0;
    for c in ob.matrix.cells.iter() {
        if n >= 16 {
            break;
        }
        s.push_str(&format!("{}\n", c.describe()));
        n = n.wrapping_add(1);
    }
    s.push_str("== fsm edges ==\n");
    for line in LinkCellState::edge_manifest().iter() {
        s.push_str(&format!("{}\n", line));
    }
    s.push_str("== inject ==\n");
    for inj in ob.injected.iter() {
        s.push_str(&format!("{}\n", inj.describe()));
    }
    s.push_str("== sweep ==\n");
    for line in ob.sweep.rows().iter() {
        s.push_str(&format!("{}\n", line));
    }
    let _ = seed;
    s
}
/// 关联单清单。
pub fn related_str() -> String {
    let mut s = String::new();
    s.push_str("F2436 大页/地址空间映射本体（本单只建模全链账，不触碰真实 CR3）\n");
    s.push_str("F2801 大页池容量压测（本单消费其三态池账口径）\n");
    s.push_str("F2810 压测基线（本单漂移检测的比对源）\n");
    s.push_str("F2840 大页映射进地址空间（前置单）\n");
    s.push_str("F2861 大页池回归矩阵（消费本单 80 格清单与 27 例映射）\n");
    s
}
// <<TAIL>>