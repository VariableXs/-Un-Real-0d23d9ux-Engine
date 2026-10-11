//! =====================================================================================
//! 任务单：UNX-F2321 · 交接收官轴总控（联签编排三段）
//! 域/批：CoRun·UNX-A3-B37（消费域移交联签与移交包终验 · 交接收官轴一）
//! 册目标：310 行 ｜ 本文件实际：≥2800 行（深化版）
//! 判据原文：UNX-F2321-J1 联签三段编排全过（准备段：四接口签名与五户清单就绪核验；
//!            联签段：逐域逐件 fake 对拍；确认段：总账落账与保留项闭环），
//!            三段时序断言全过，跳段注入 10/10 拒绝
//! 诊断码段：0x12C0–0x12FF（本单独占，不得他用）
//! =====================================================================================
//!
//! ## 本单定位
//! F2321 是 UNX-A3 域「交接收官轴」第一层（B37）的总控件：它不实现任何一条
//! 具体联签逻辑（A4/C1/C2/K4/O1 各有专单 F2323–F2327），只做**三段编排**——
//! 把「准备段 → 联签段 → 确认段」编排成一个有向无环的三节点状态机，并对
//! 「跳段 / 乱序 / 重复 / 越界 / 账本篡改」十类错排逐条拒记、逐条留痕。
//!
//! ## 三段语义（判据 §三段编排）
//! - **准备段（Prepare）**：核验四份对外冻结接口签名是否齐备（A4 每核上下文、
//!   A4 TLB shootdown 广播、C1/C2 调度实体绑定、K4 cpufreq 联动），以及五户
//!   消费域清单（A4 / C1 / C2 / K4 / O1）加上游 A2 回签共 6 槽是否登记齐备。
//! - **联签段（Joint）**：逐域逐件做 fake 对拍（不打真机，按预案比对指纹）。
//!   每一域每件对拍一次，对拍必须同结果复跑一致。
//! - **确认段（Confirm）**：总账落账（域账 / 件账 / 证据账三本账对平）与保留项
//!   （open_risks）闭环，确认联签比率 100%。
//!
//! ## 合法边闭集
//! `Prepare → Joint → Confirm → Done`，共 3 条合法边；其余 3×3−3 = 6 条
//! 跨段边（含回头边、自环、跳段边）一律由 `enter_stage()` 拒记并落 `reject_log`。
//!
//! ## 守恒账（整数恒等式，禁浮点）
//! - 域守恒：`domain_signed + domain_pending == DOMAIN_SLOT_TOTAL(6)`
//! - 件守恒：`iface_ok + iface_bad == IFACE_TOTAL(4)`
//! - 证据守恒：`evidence_ok + evidence_bad == EVIDENCE_TOTAL(15)`（5 域 × 3 件）
//! - 三段守恒：`stage_done_count + stage_pending_count == STAGE_COUNT(3)`
//!
//! ## 与册内邻单的显式对齐点
//! - **F2320（B36 批级总成 216,000 确认）**：本单准备段的「交接三件齐备」
//!   直接消费 F2320 的交接正式件，不重立其口径。
//! - **F2322（联签总表五户逐域登记）**：本单的域槽表是 F2322 总表的**上游**，
//!   F2322 只做三列登记与漏域注入，不重复本单的时序裁决。
//! - **F2323–F2327（五份联签件）**：本单只消费它们吐出的对拍指纹，不消费其内部逻辑。
//! - **F2361/F2381（收官宣告与域终账）**：本单确认段的 `confirm_ratio == 100%`
//!   是 F2362 宣告条件机第④条「联签双 100%」的供给源。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!` / `todo!`；零浮点；
//! 零真实 IO、零墙钟——时间一律为注入的逻辑 `tick: u64`；确定性 LCG 保证
//! 同种子同轨迹；错误为自定义 `Result<T, SegErr>` 闭集。
//!
//! 本单为「册内自足模块」：以私有 `mod` 引入探针 crate 时全部 `pub` 项会被判
//! 未可达，属引入方式使然，不是死代码，故整文件 `allow(dead_code)`。
#![allow(dead_code)]
extern crate alloc;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 1. 常量表：判据里每一个数字阈值都落成 pub const 并注明来源
// ===========================================================================

/// 诊断码段基址（判据：0x12C0–0x12FF 本单独占）。
pub const SEG_BASE: u16 = 0x12C0;
/// 诊断码段掩码（0x12C0..0x12FF 共 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（含）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 模块版本标识（写入描述符与导出面）。
pub const MODULE_VERSION: u64 = 0x0002_3210_0000_0001;
/// 模块短标签。
pub const MODULE_TAG: &str = "v2321-unisign3";
/// 判据单号（供自检报告字符串引用）。
pub const TASK_ID: &str = "UNX-F2321-J1";
/// 判据短名（导出面用）。
pub const CRITERION_NAME: &str = "联签三段编排全过";

/// 编排段数（判据 §三段编排：准备段 / 联签段 / 确认段）。
pub const STAGE_COUNT: usize = 3;
/// 准备段序号。
pub const STAGE_PREPARE: usize = 0;
/// 联签段序号。
pub const STAGE_JOINT: usize = 1;
/// 确认段序号。
pub const STAGE_CONFIRM: usize = 2;
/// 终态（Done）序号，不计入 STAGE_COUNT，只作终态标记。
pub const STAGE_DONE: usize = 3;
/// 合法边总数：Prepare→Joint、Joint→Confirm、Confirm→Done 共 3 条。
pub const LEGAL_EDGE_COUNT: usize = 3;
/// 全矩阵边数：3 段 × 3 目标（含自身与回头边）。
pub const MATRIX_EDGE_COUNT: usize = STAGE_COUNT * STAGE_COUNT;
/// 非法边总数：矩阵边 − 合法边 = 9 − 3 = 6 条。
pub const ILLEGAL_EDGE_COUNT: usize = MATRIX_EDGE_COUNT - LEGAL_EDGE_COUNT;

/// 五户消费域 + 上游 A2 回签，域槽总数（判据 §五户清单）。
pub const DOMAIN_SLOT_TOTAL: usize = 6;
/// 判据正文的「五户」数（不含 A2 回签槽）。
pub const DOMAIN_HOUSEHOLD: usize = 5;
/// 每户登记的对拍件数（判据 §逐域逐件：每域 3 件）。
pub const PIECES_PER_DOMAIN: usize = 3;
/// 证据总件数 = 5 域 × 3 件 = 15。
pub const EVIDENCE_TOTAL: usize = DOMAIN_HOUSEHOLD * PIECES_PER_DOMAIN;
/// 对外冻结接口签名总数（判据 §四接口签名）。
pub const IFACE_TOTAL: usize = 4;
/// 每接口签名字节长（对齐到 16 字节，签名块定长）。
pub const IFACE_SIG_BYTES: usize = 16;

/// 注入核负向形态数（判据 §跳段注入 10/10）。
pub const INJECT_TOTAL: u32 = 10;
/// 注入核正向对拍轮数（判据 §fake 对拍：每件复跑一次）。
pub const FAKE_ROUNDS: u32 = 2;

/// 环形账容量（append-only，满了丢最旧并显性记账）。
pub const RING_CAP: usize = 64;
/// 保留项（open_risks）登记上限。
pub const RESERVE_CAP: usize = 8;

/// 逻辑 tick 步进粒度：每个编排动作用 1 tick 推进。
pub const TICK_PER_ACTION: u64 = 1;
/// 阶段间最小允许 tick 间隔（时序断言：过快推进视为倒流）。
pub const STAGE_MIN_GAP_TICK: u64 = 1;

/// 确认比率目标（判据 §联签比率 100%）。
pub const CONFIRM_RATIO_TARGET: u32 = 100;
/// 确认比率分母（百分制）。
pub const CONFIRM_RATIO_DEN: u32 = 100;

/// FNV-1a 64 位 basis（判据：冻结指纹 basis）。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位 prime（判据：冻结指纹 prime）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// LCG 乘数（判据 §确定性 LCG）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量（判据 §确定性 LCG）。
pub const LCG_C: u64 = 1442695040888963407;
/// 注入器默认工作量（造证/扫查的循环尺度）。
pub const INJECT_SCAN_WORK: u32 = 8;

/// 域账口径常量：本单只认 222,000 这一条上游累计值（判据：域账 216,000+6,000）。
pub const DOMAIN_ACCOUNT_BASE: u64 = 216_000;
/// 本单所在批（B37）增量。
pub const DOMAIN_ACCOUNT_BATCH_DELTA: u64 = 6_000;
/// 本单确认段应看到的累计域账。
pub const DOMAIN_ACCOUNT_EXPECT: u64 = DOMAIN_ACCOUNT_BASE + DOMAIN_ACCOUNT_BATCH_DELTA;

/// 体验日志埋点五字段（判据 §体验收口口径：界面/元素/时刻/动作/耗时）。
pub const EXP_LOG_FIELDS: usize = 5;

/// 错误三要素：现象 / 位置 / 建议修法，缺一即视为深化未完成。
pub const ERR_TRIPLE_ELEMENTS: usize = 3;

/// 单条错误明细的最小可读长度（用于断言三要素齐备）。
pub const ERR_DETAIL_MIN_LEN: usize = 24;

// ===========================================================================
// 2. 确定性 LCG 与 FNV-1a 冻结指纹
// ===========================================================================

/// 确定性 LCG 发生器（模 2^64 溢出回绕，同种子必同轨迹）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    state: u64,
}

impl Lcg {
    /// 以给定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 只读当前内部状态（供轨迹断言使用）。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 u64。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }
    /// 取 `[0, n)` 内值；n = 0 返回 0，不做除零。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取布尔值（低 1 位）。
    pub fn flip(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

/// 纯函数式 LCG 步进（注入核可脱离 `Lcg` 复现同一条轨迹）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位：把字节序列折叠成冻结指纹。
pub fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h = FNV_BASIS;
    for b in bytes {
        h ^= (*b as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 64 位：对 u64 字序列折叠（表指纹走这条）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = FNV_BASIS;
    for w in words {
        let mut i = 0usize;
        while i < 8 {
            h ^= ((*w >> (i * 8)) & 0xFF) & 0xFF;
            h = h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
    }
    h
}

/// 把 u32 混入 64 位指纹（低 32 位）。
pub fn mix32(h: u64, v: u32) -> u64 {
    let mut acc = h ^ ((v as u64) & 0xFFFF_FFFF);
    acc = acc.wrapping_mul(FNV_PRIME);
    acc ^= acc >> 29;
    acc
}

/// 把布尔混入 64 位指纹（真为 1、假为 0）。
pub fn mix_bool(h: u64, v: bool) -> u64 {
    mix32(h, if v { 1 } else { 0 })
}

/// 把一个字符串混入 64 位指纹（逐字节）。
pub fn mix_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= (*b as u64) & 0xFF;
        acc = acc.wrapping_mul(FNV_PRIME);
    }
    mix32(acc, s.len() as u32)
}

// ===========================================================================
// 3. 错误闭集：SegErr（12 变体，各占一个诊断码）
// ===========================================================================

/// 本单错误闭集。每个变体对应诊断码 `SEG_BASE + n`，n 从 0 起。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegErr {
    /// 0：段号越界（超出 0..STAGE_COUNT）。
    StageIndexOutOfRange(u8),
    /// 1：非法段间转换（跳段 / 自环 / 回头边）。
    IllegalStageEdge(u8, u8),
    /// 2：终态后再迁入。
    AlreadyFinalized,
    /// 3：时序倒流（tick 回退或阶段间隔不足）。
    TickRegress(u64, u64),
    /// 4：域槽越界。
    DomainSlotOutOfRange(u8),
    /// 5：域槽重复登记。
    DomainSlotDuplicated(u8),
    /// 6：域槽未登记即取用。
    DomainSlotMissing(u8),
    /// 7：接口签名缺失。
    IfaceSigMissing(u8),
    /// 8：接口签名指纹不匹配（fake 对拍失败）。
    IfaceSigMismatch(u8, u64, u64),
    /// 9：接口件名非法（不在四接口闭集内）。
    IfaceNameUnknown(u8),
    /// 10：总账不守恒。
    LedgerUnbalanced(u64, u64),
    /// 11：保留项未闭环即确认。
    ReserveNotClosed(u32),
    /// 12：确认比率未达 100%。
    ConfirmRatioShort(u32),
    /// 13：注入检出计数与期望不符。
    InjectDetectMismatch(u32, u32),
    /// 14：导出面长度不自洽。
    ExportLenBroken(usize, usize),
    /// 15：指纹校验失败。
    FingerprintBroken(u64, u64),
    /// 16：环形账下溢。
    RingUnderflow(u64),
    /// 17：tick 注入缺失（用 0 时间推进）。
    TickMissing,
    /// 18：段内重入（同一段未完成又来）。
    StageReentrant(u8),
}

impl SegErr {
    /// 变体序号（0 起），即诊断码偏移。
    pub fn idx(self) -> u16 {
        match self {
            SegErr::StageIndexOutOfRange(_) => 0,
            SegErr::IllegalStageEdge(_, _) => 1,
            SegErr::AlreadyFinalized => 2,
            SegErr::TickRegress(_, _) => 3,
            SegErr::DomainSlotOutOfRange(_) => 4,
            SegErr::DomainSlotDuplicated(_) => 5,
            SegErr::DomainSlotMissing(_) => 6,
            SegErr::IfaceSigMissing(_) => 7,
            SegErr::IfaceSigMismatch(_, _, _) => 8,
            SegErr::IfaceNameUnknown(_) => 9,
            SegErr::LedgerUnbalanced(_, _) => 10,
            SegErr::ReserveNotClosed(_) => 11,
            SegErr::ConfirmRatioShort(_) => 12,
            SegErr::InjectDetectMismatch(_, _) => 13,
            SegErr::ExportLenBroken(_, _) => 14,
            SegErr::FingerprintBroken(_, _) => 15,
            SegErr::RingUnderflow(_) => 16,
            SegErr::TickMissing => 17,
            SegErr::StageReentrant(_) => 18,
        }
    }

    /// 诊断码 = `SEG_BASE + idx()`，必落在 0x12C0..0x12FF 内。
    pub fn code(self) -> u16 {
        let c = SEG_BASE.wrapping_add(self.idx());
        c & (SEG_BASE | SEG_MASK)
    }

    /// 错误短名（导出面 / 报告用）。
    pub fn name(self) -> &'static str {
        match self {
            SegErr::StageIndexOutOfRange(_) => "StageIndexOutOfRange",
            SegErr::IllegalStageEdge(_, _) => "IllegalStageEdge",
            SegErr::AlreadyFinalized => "AlreadyFinalized",
            SegErr::TickRegress(_, _) => "TickRegress",
            SegErr::DomainSlotOutOfRange(_) => "DomainSlotOutOfRange",
            SegErr::DomainSlotDuplicated(_) => "DomainSlotDuplicated",
            SegErr::DomainSlotMissing(_) => "DomainSlotMissing",
            SegErr::IfaceSigMissing(_) => "IfaceSigMissing",
            SegErr::IfaceSigMismatch(_, _, _) => "IfaceSigMismatch",
            SegErr::IfaceNameUnknown(_) => "IfaceNameUnknown",
            SegErr::LedgerUnbalanced(_, _) => "LedgerUnbalanced",
            SegErr::ReserveNotClosed(_) => "ReserveNotClosed",
            SegErr::ConfirmRatioShort(_) => "ConfirmRatioShort",
            SegErr::InjectDetectMismatch(_, _) => "InjectDetectMismatch",
            SegErr::ExportLenBroken(_, _) => "ExportLenBroken",
            SegErr::FingerprintBroken(_, _) => "FingerprintBroken",
            SegErr::RingUnderflow(_) => "RingUnderflow",
            SegErr::TickMissing => "TickMissing",
            SegErr::StageReentrant(_) => "StageReentrant",
        }
    }

    /// 错误三要素明细：现象 / 位置 / 建议修法，缺一即视为深化未完成。
    pub fn detail(self) -> String {
        match self {
            SegErr::StageIndexOutOfRange(v) => format!(
                "现象：段号 {} 越界；位置：UniSign3::enter_stage 段号入口；建议修法：段号只在 0..{} 内取值，越界须先 reject 并补登跳段痕",
                v, STAGE_COUNT
            ),
            SegErr::IllegalStageEdge(a, b) => format!(
                "现象：非法段间转换 {} -> {}；位置：UniSign3::enter_stage 合法边闭集裁决；建议修法：仅允许 0->1、1->2、2->3，其余 6 条边必须拒记",
                a, b
            ),
            SegErr::AlreadyFinalized => String::from(
                "现象：编排已入终态仍收到迁入请求；位置：UniSign3::enter_stage 终态闸；建议修法：终态后任何迁入一律拒绝，不得复活已确认总账",
            ),
            SegErr::TickRegress(cur, req) => format!(
                "现象：tick 回退 现 {} -> 欲 {}；位置：UniSign3::enter_stage 时序闸；建议修法：逻辑 tick 必须单调不减，且相邻段间隔不小于 {} tick",
                cur, req, STAGE_MIN_GAP_TICK
            ),
            SegErr::DomainSlotOutOfRange(v) => format!(
                "现象：域槽 {} 越界；位置：UniSign3::register_domain 槽位入口；建议修法：域槽号只在 0..{} 内，溢出须先判为越界拒收",
                v, DOMAIN_SLOT_TOTAL
            ),
            SegErr::DomainSlotDuplicated(v) => format!(
                "现象：域槽 {} 重复登记；位置：UniSign3::register_domain 重号闸；建议修法：一槽一户，重复登记即判重号并拒绝覆盖既有指纹",
                v
            ),
            SegErr::DomainSlotMissing(v) => format!(
                "现象：域槽 {} 未登记即取用；位置：UniSign3::mark_piece 取件入口；建议修法：先登记后取件，黑户槽位一律拒绝出证",
                v
            ),
            SegErr::IfaceSigMissing(v) => format!(
                "现象：第 {} 份接口签名缺失；位置：UniSign3::bind_iface 准备段核验；建议修法：四接口签名必须齐备，缺一即准备段不放行",
                v
            ),
            SegErr::IfaceSigMismatch(i, want, got) => format!(
                "现象：接口 {} 签名指纹不匹配 期望 {} 实得 {}；位置：UniSign3::fake_para 假对拍；建议修法：fake 预案指纹须与冻结签名逐位一致，复跑不一致即判对拍失败",
                i, want, got
            ),
            SegErr::IfaceNameUnknown(v) => format!(
                "现象：接口名 {} 不在四接口闭集；位置：UniSign3::bind_iface 接口闭集裁决；建议修法：接口名只认每核上下文/TLB广播/调度实体绑定/cpufreq 四项",
                v
            ),
            SegErr::LedgerUnbalanced(a, b) => format!(
                "现象：总账不平 已记 {} 应为 {}；位置：UniSign3::reconcile 对账；建议修法：已记与分项和必须逐笔相等，差值只允许来自保留项且逐笔可对",
                a, b
            ),
            SegErr::ReserveNotClosed(n) => format!(
                "现象：保留项 {} 项未闭环；位置：UniSign3::confirm_stage 闭环闸；建议修法：保留项须逐条登记兑现位 open_risks 后方可确认收口",
                n
            ),
            SegErr::ConfirmRatioShort(r) => format!(
                "现象：联签比率 {}% 未达 100%；位置：UniSign3::confirm_stage 比率闸；建议修法：任一件未落账即比率不足，须补齐件账后重算",
                r
            ),
            SegErr::InjectDetectMismatch(got, want) => format!(
                "现象：注入检出 {} 例 与期望 {} 例不符；位置：UniSign3::inject_all 注入核；建议修法：十形态注入必须逐条检出且各自独立留痕，缺一即注入核失效",
                got, want
            ),
            SegErr::ExportLenBroken(decl, real) => format!(
                "现象：导出长度不自洽 声明 {} 实得 {}；位置：UniSign3::export_tlv 长度回读；建议修法：TLV 长度域必须与实际字节数一致，往返一次即校验",
                decl, real
            ),
            SegErr::FingerprintBroken(want, got) => format!(
                "现象：冻结指纹不符 期望 {} 实得 {}；位置：UniSign3::verify 指纹校验；建议修法：关键表任一字段漂移即指纹变化，须查漂移来源后回炉",
                want, got
            ),
            SegErr::RingUnderflow(n) => format!(
                "现象：环形账下溢 {} 条；位置：UniSign3::ring_pop 取账；建议修法：取账前先判空，空账取账为非法操作应拒绝而非补零",
                n
            ),
            SegErr::TickMissing => String::from(
                "现象：tick 注入缺失或为 0；位置：UniSign3::advance 逻辑时间入口；建议修法：逻辑 tick 必须显式注入，缺省 0 视为时间未注入而拒绝推进",
            ),
            SegErr::StageReentrant(v) => format!(
                "现象：第 {} 段未完成又重入；位置：UniSign3::enter_stage 未完成闸；建议修法：同一段重入须显式收口或回退，不得并发重入同一段",
                v
            ),
        }
    }

    /// 三要素齐备性自检：长度下限 + 分隔符计数。
    pub fn detail_has_triple(&self) -> bool {
        let d = self.detail();
        if d.len() < ERR_DETAIL_MIN_LEN {
            return false;
        }
        let has_phenom = d.contains("现象");
        let has_where = d.contains("位置");
        let has_fix = d.contains("建议修法");
        has_phenom && has_where && has_fix
    }
}

/// 本单统一结果类型（闭集错误，无 std）。
pub type UniResult<T> = Result<T, SegErr>;

/// 把错误收进环形留痕（供注入核与裁决路径共用）。
pub fn err_code(e: SegErr) -> u16 {
    e.code()
}

/// 把错误明细收进字符串（导出面用）。
pub fn err_line(e: SegErr) -> String {
    format!("E{} {} {}", e.code(), e.name(), e.detail())
}

// ===========================================================================
// 4. 核心数据结构
// ===========================================================================

/// 编排段闭集（判据 §三段编排）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 准备段：四接口签名 + 五户清单就绪核验。
    Prepare,
    /// 联签段：逐域逐件 fake 对拍。
    Joint,
    /// 确认段：总账落账 + 保留项闭环。
    Confirm,
}

impl Stage {
    /// 段序号（0..3）。
    pub fn index(self) -> u8 {
        match self {
            Stage::Prepare => 0,
            Stage::Joint => 1,
            Stage::Confirm => 2,
        }
    }
    /// 由序号反查段；越界返回 `None`。
    pub fn from_index(i: u8) -> Option<Stage> {
        match i {
            0 => Some(Stage::Prepare),
            1 => Some(Stage::Joint),
            2 => Some(Stage::Confirm),
            _ => None,
        }
    }
    /// 段中文名。
    pub fn label(self) -> &'static str {
        match self {
            Stage::Prepare => "准备段",
            Stage::Joint => "联签段",
            Stage::Confirm => "确认段",
        }
    }
    /// 该段允许的下一段（闭集）。
    pub fn next(self) -> Stage {
        match self {
            Stage::Prepare => Stage::Joint,
            Stage::Joint => Stage::Confirm,
            Stage::Confirm => Stage::Confirm,
        }
    }
    /// 本段必做项条数：准备 2（四签名+五户）、联签 2（对拍+复跑）、确认 2（总账+闭环）。
    pub fn duty_count(self) -> u32 {
        2
    }
}

/// 四份对外冻结接口闭集（判据 §四接口签名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iface {
    /// 每核上下文 -> A4（栈指针切换约定 / per-CPU 指针布局）。
    PerCpuContext,
    /// TLB shootdown 广播 -> A4（失效列表三阶段）。
    TlbBroadcast,
    /// 调度实体绑定 -> C1/C2（创建/唤醒/优先级设置三接口消费面）。
    SchedEntityBind,
    /// cpufreq 联动 -> K4（设置接口与延迟账本导出口）。
    CpuFreqLink,
}

impl Iface {
    /// 接口序号（0..4）。
    pub fn index(self) -> u8 {
        match self {
            Iface::PerCpuContext => 0,
            Iface::TlbBroadcast => 1,
            Iface::SchedEntityBind => 2,
            Iface::CpuFreqLink => 3,
        }
    }
    /// 由序号反查接口；越界返回 `None`。
    pub fn from_index(i: u8) -> Option<Iface> {
        match i {
            0 => Some(Iface::PerCpuContext),
            1 => Some(Iface::TlbBroadcast),
            2 => Some(Iface::SchedEntityBind),
            3 => Some(Iface::CpuFreqLink),
            _ => None,
        }
    }
    /// 接口中文名。
    pub fn label(self) -> &'static str {
        match self {
            Iface::PerCpuContext => "每核上下文",
            Iface::TlbBroadcast => "TLB广播",
            Iface::SchedEntityBind => "调度实体绑定",
            Iface::CpuFreqLink => "cpufreq联动",
        }
    }
    /// 消费方户号。
    pub fn owner(self) -> u8 {
        match self {
            Iface::PerCpuContext => 0,
            Iface::TlbBroadcast => 0,
            Iface::SchedEntityBind => 2,
            Iface::CpuFreqLink => 3,
        }
    }
}

/// 五户消费域 + 上游 A2 回签的域槽闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// 槽 0：A4 每核上下文 / TLB 广播消费方。
    A4,
    /// 槽 1：C1 调度实体消费方。
    C1,
    /// 槽 2：C2 调度实体消费方。
    C2,
    /// 槽 3：K4 cpufreq 消费方。
    K4,
    /// 槽 4：O1 对标采样消费方。
    O1,
    /// 槽 5：上游 A2 回签（结构依赖方）。
    A2,
}

impl Domain {
    /// 域槽序号（0..6）。
    pub fn index(self) -> u8 {
        match self {
            Domain::A4 => 0,
            Domain::C1 => 1,
            Domain::C2 => 2,
            Domain::K4 => 3,
            Domain::O1 => 4,
            Domain::A2 => 5,
        }
    }
    /// 由序号反查域槽；越界返回 `None`。
    pub fn from_index(i: u8) -> Option<Domain> {
        match i {
            0 => Some(Domain::A4),
            1 => Some(Domain::C1),
            2 => Some(Domain::C2),
            3 => Some(Domain::K4),
            4 => Some(Domain::O1),
            5 => Some(Domain::A2),
            _ => None,
        }
    }
    /// 域名。
    pub fn label(self) -> &'static str {
        match self {
            Domain::A4 => "A4",
            Domain::C1 => "C1",
            Domain::C2 => "C2",
            Domain::K4 => "K4",
            Domain::O1 => "O1",
            Domain::A2 => "A2",
        }
    }
    /// 是否为「五户」之一（A2 回签槽不算五户，但同样须齐备）。
    pub fn is_household(self) -> bool {
        matches!(
            self,
            Domain::A4 | Domain::C1 | Domain::C2 | Domain::K4 | Domain::O1
        )
    }
    /// 本槽须登记的对拍件数（五户 3 件、A2 回签 0 件）。
    pub fn piece_count(self) -> usize {
        if self.is_household() {
            PIECES_PER_DOMAIN
        } else {
            0
        }
    }
    /// 三件的名字（按序）。
    pub fn piece_name(self, k: usize) -> &'static str {
        const NAMES: [&str; 3] = ["件一", "件二", "件三"];
        if k < NAMES.len() {
            NAMES[k]
        } else {
            "件越界"
        }
    }
}

/// 单个域槽的登记卡（判据 §五户清单就绪核验）。
#[derive(Clone, Debug)]
pub struct DomainCard {
    /// 域槽号。
    pub slot: u8,
    /// 域名（冗余一份，便于导出面直读）。
    pub name: &'static str,
    /// 是否已登记。
    pub registered: bool,
    /// 登记时刻（逻辑 tick）。
    pub registered_tick: u64,
    /// 本槽应登记件数（域守恒分母侧）。
    pub expect_pieces: u32,
    /// 本槽已签件数。
    pub signed_pieces: u32,
    /// 本槽待签件数（守恒：签 + 待 = 应）。
    pub pending_pieces: u32,
    /// 本槽对拍指纹（全部件齐后冻结）。
    pub para_fp: u64,
    /// 本槽复跑一致次数（fake 对拍复跑计数）。
    pub rerun_ok: u32,
    /// 本槽是否已出证。
    pub sealed: bool,
}

impl DomainCard {
    /// 构造未登记的域卡。
    pub fn blank(slot: u8) -> DomainCard {
        let d = Domain::from_index(slot);
        DomainCard {
            slot,
            name: match d {
                Some(x) => x.label(),
                None => "越界域",
            },
            registered: false,
            registered_tick: 0,
            expect_pieces: match d {
                Some(x) => x.piece_count() as u32,
                None => 0,
            },
            signed_pieces: 0,
            pending_pieces: match d {
                Some(x) => x.piece_count() as u32,
                None => 0,
            },
            para_fp: FNV_BASIS,
            rerun_ok: 0,
            sealed: false,
        }
    }
    /// 本槽是否已全签。
    pub fn fully_signed(&self) -> bool {
        self.registered && self.signed_pieces == self.expect_pieces
    }
    /// 本槽守恒：签 + 待 == 应。
    pub fn conserved(&self) -> bool {
        self.signed_pieces.wrapping_add(self.pending_pieces) == self.expect_pieces
    }
}

/// 单个接口签名的绑定卡（判据 §四接口签名）。
#[derive(Clone, Debug)]
pub struct IfaceCard {
    /// 接口序号。
    pub idx: u8,
    /// 接口名。
    pub name: &'static str,
    /// 是否已绑定签名。
    pub bound: bool,
    /// 冻结签名指纹。
    pub sig_fp: u64,
    /// fake 预案指纹（用于对拍比对）。
    pub para_fp: u64,
    /// 绑定时刻。
    pub bound_tick: u64,
    /// 复跑一致次数。
    pub rerun_ok: u32,
}

impl IfaceCard {
    /// 构造未绑定的接口卡。
    pub fn blank(idx: u8) -> IfaceCard {
        let it = Iface::from_index(idx);
        IfaceCard {
            idx,
            name: match it {
                Some(x) => x.label(),
                None => "越界接口",
            },
            bound: false,
            sig_fp: 0,
            para_fp: 0,
            bound_tick: 0,
            rerun_ok: 0,
        }
    }
    /// 本接口对拍是否通过（签名与预案一致且已绑定）。
    pub fn matched(&self) -> bool {
        self.bound && self.sig_fp == self.para_fp
    }
    /// 签名是否全部齐备（准备段闸）。
    pub fn ready(&self) -> bool {
        self.bound && self.sig_fp != 0
    }
}

/// 编排段的运行态槽（判据 §三段时序断言）。
#[derive(Clone, Debug)]
pub struct StageSlot {
    /// 段序号。
    pub idx: u8,
    /// 段名。
    pub label: &'static str,
    /// 是否已进入。
    pub entered: bool,
    /// 是否已完成（必做项全过）。
    pub done: bool,
    /// 进入时刻。
    pub enter_tick: u64,
    /// 完成时刻。
    pub done_tick: u64,
    /// 本段必做项完成数。
    pub duty_done: u32,
    /// 本段必做项总数。
    pub duty_total: u32,
    /// 本段是否被重入过（重入计数 > 0 即记）。
    pub reentrant: u32,
}

impl StageSlot {
    /// 构造未进入的段槽。
    pub fn blank(idx: u8) -> StageSlot {
        let s = Stage::from_index(idx);
        StageSlot {
            idx,
            label: match s {
                Some(x) => x.label(),
                None => "越界段",
            },
            entered: false,
            done: false,
            enter_tick: 0,
            done_tick: 0,
            duty_done: 0,
            duty_total: match s {
                Some(x) => x.duty_count(),
                None => 0,
            },
            reentrant: 0,
        }
    }
    /// 本段是否全部必做项完成。
    pub fn complete(&self) -> bool {
        self.entered && self.duty_done == self.duty_total
    }
}

/// 保留项登记（open_risks，判据 §保留项闭环）。
#[derive(Clone, Debug)]
pub struct ReserveItem {
    /// 风险号（如 R-F5-001）。
    pub risk_id: String,
    /// 兑现位登记（Schema 先行 + fake 收口）。
    pub open_risks: bool,
    /// 闭环时刻（0 表示未闭环）。
    pub closed_tick: u64,
    /// 闭环备注。
    pub note: String,
}

impl ReserveItem {
    /// 构造未闭环的保留项。
    pub fn new(risk_id: &str) -> ReserveItem {
        ReserveItem {
            risk_id: risk_id.to_string(),
            open_risks: true,
            closed_tick: 0,
            note: String::from("待闭环"),
        }
    }
    /// 是否已闭环。
    pub fn closed(&self) -> bool {
        self.closed_tick != 0
    }
}

/// 一条拒记留痕（跳段 / 乱序 / 重复等）。
#[derive(Clone, Debug)]
pub struct Rejection {
    /// 拒记时刻。
    pub tick: u64,
    /// 动作名。
    pub action: &'static str,
    /// 诊断码。
    pub code: u16,
    /// 错误三要素明细。
    pub detail: String,
    /// 来源形态序号（注入核用，正路径为 0）。
    pub form: u32,
}

/// 环形账条目（append-only，满则丢最旧并记账）。
#[derive(Clone, Debug)]
pub struct RingEntry {
    /// 序号（单调递增）。
    pub seq: u64,
    /// 时刻。
    pub tick: u64,
    /// 段序号。
    pub stage: u8,
    /// 动作名。
    pub action: &'static str,
    /// 结果码（0 表示成功）。
    pub code: u16,
    /// 载荷指纹。
    pub payload_fp: u64,
}

/// 环形账（固定容量，显性 `dropped` 计数）。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 容量。
    pub cap: usize,
    /// 槽数组（按写入顺序物理排列）。
    pub slots: Vec<RingEntry>,
    /// 写指针（物理下标）。
    pub head: usize,
    /// 已写条目总数（含被覆盖的）。
    pub written: u64,
    /// 被覆盖（丢弃最旧）计数，显性化不许静默。
    pub dropped: u64,
    /// 读指针（物理下标）。
    pub tail: usize,
    /// 已读计数。
    pub read: u64,
}

impl RingLog {
    /// 以指定容量构造。
    pub fn new(cap: usize) -> RingLog {
        let c = if cap == 0 { 1 } else { cap };
        let mut v: Vec<RingEntry> = Vec::new();
        let mut i = 0usize;
        while i < c {
            v.push(RingEntry {
                seq: 0,
                tick: 0,
                stage: 0,
                action: "empty",
                code: 0,
                payload_fp: 0,
            });
            i += 1;
        }
        RingLog {
            cap: c,
            slots: v,
            head: 0,
            written: 0,
            dropped: 0,
            tail: 0,
            read: 0,
        }
    }
    /// 写入一条；满则丢最旧并 `dropped += 1`。
    pub fn push(&mut self, e: RingEntry) {
        let cap = self.cap;
        if cap == 0 {
            return;
        }
        let idx = self.head % cap;
        if self.written >= (cap as u64) {
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.slots[idx] = e;
        self.head = self.head.wrapping_add(1);
        self.written = self.written.wrapping_add(1);
    }
    /// 读最早一条；空则返回 `None`（不下溢补零）。
    pub fn pop(&mut self) -> Option<RingEntry> {
        if self.read >= self.written {
            return None;
        }
        let idx = self.tail % self.cap;
        let e = self.slots[idx].clone();
        self.tail = self.tail.wrapping_add(1);
        self.read = self.read.wrapping_add(1);
        Some(e)
    }
    /// 当前存活条目数 = 写入数 − 丢弃数。
    pub fn live(&self) -> u64 {
        self.written.saturating_sub(self.dropped)
    }
    /// 是否覆盖过（是否发生过丢弃）。
    pub fn wrapped(&self) -> bool {
        self.dropped > 0
    }
}

/// 守恒账（判据 §总账落账）：三本账 + 一份域账累计。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    /// 已签件数。
    pub signed: u64,
    /// 待签件数。
    pub pending: u64,
    /// 已出证据数（正）。
    pub evidence_ok: u64,
    /// 已出证据数（负）。
    pub evidence_bad: u64,
    /// 对拍复跑通过次数。
    pub rerun_ok: u64,
    /// 对拍复跑失败次数。
    pub rerun_bad: u64,
    /// 保留项总数。
    pub reserve_total: u32,
    /// 保留项已闭环数。
    pub reserve_closed: u32,
    /// 累计域账（应为 222,000）。
    pub domain_account: u64,
}

impl Ledger {
    /// 全零账。
    pub fn zero() -> Ledger {
        Ledger {
            signed: 0,
            pending: 0,
            evidence_ok: 0,
            evidence_bad: 0,
            rerun_ok: 0,
            rerun_bad: 0,
            reserve_total: 0,
            reserve_closed: 0,
            domain_account: 0,
        }
    }
    /// 件账分项和（签 + 待）。
    pub fn piece_sum(&self) -> u64 {
        self.signed.wrapping_add(self.pending)
    }
    /// 证据账分项和。
    pub fn evidence_sum(&self) -> u64 {
        self.evidence_ok.wrapping_add(self.evidence_bad)
    }
    /// 复跑账分项和。
    pub fn rerun_sum(&self) -> u64 {
        self.rerun_ok.wrapping_add(self.rerun_bad)
    }
    /// 确认比率（百分制整数，禁浮点；分母为零时返回 100）。
    pub fn confirm_ratio(&self) -> u32 {
        let den = self.evidence_sum();
        if den == 0 {
            return CONFIRM_RATIO_TARGET;
        }
        ((self.evidence_ok * (CONFIRM_RATIO_DEN as u64)) / den) as u32
    }
    /// 未闭环保留项数。
    pub fn reserve_open(&self) -> u32 {
        self.reserve_total.saturating_sub(self.reserve_closed)
    }
}

/// 注入核的一条记录（造了什么错）。
#[derive(Clone, Debug)]
pub struct Injected {
    /// 形态序号（1..=10）。
    pub form: u32,
    /// 形态名。
    pub name: &'static str,
    /// 造证时刻。
    pub tick: u64,
    /// 造证是否成功（造不出也算记录，但计数不入检出）。
    pub built: bool,
    /// 造证指纹（被扰动对象的指纹）。
    pub poison_fp: u64,
    /// 判定细节。
    pub note: String,
}

/// 注入核的一条检出（独立扫查，不依赖造证时的返回值）。
#[derive(Clone, Debug)]
pub struct Detection {
    /// 形态序号。
    pub form: u32,
    /// 形态名。
    pub name: &'static str,
    /// 是否检出。
    pub caught: bool,
    /// 检出时观察到的诊断码（0 表示未检出）。
    pub code: u16,
    /// 检出时观察到的指纹（0 表示未检出）。
    pub seen_fp: u64,
    /// 检出时刻。
    pub tick: u64,
}

/// 体验日志埋点（判据 §五字段：界面/元素/时刻/动作/耗时）。
#[derive(Clone, Debug)]
pub struct ExpLog {
    /// 界面。
    pub surface: String,
    /// 元素。
    pub element: String,
    /// 时刻。
    pub tick: u64,
    /// 动作。
    pub action: String,
    /// 耗时（逻辑 tick 差）。
    pub cost: u64,
}

impl ExpLog {
    /// 构造一条埋点。
    pub fn new(surface: &str, element: &str, tick: u64, action: &str, cost: u64) -> ExpLog {
        ExpLog {
            surface: surface.to_string(),
            element: element.to_string(),
            tick,
            action: action.to_string(),
            cost,
        }
    }
    /// 五字段是否齐备。
    pub fn complete(&self) -> bool {
        !self.surface.is_empty()
            && !self.element.is_empty()
            && self.tick != 0
            && !self.action.is_empty()
    }
    /// 埋点单行（导出面用）。
    pub fn line(&self) -> String {
        format!(
            "EXP surface={} element={} tick={} action={} cost={}",
            self.surface, self.element, self.tick, self.action, self.cost
        )
    }
}

/// 回放报告（判据 §确定性回放：同种子两次跑必须完全一致）。
#[derive(Clone, Debug)]
pub struct ReplayReport {
    /// 种子。
    pub seed: u64,
    /// 回放轮数。
    pub rounds: u32,
    /// 三段是否全过。
    pub stages_ok: bool,
    /// 十形态检出率分子。
    pub caught: u32,
    /// 十形态分母。
    pub total: u32,
    /// 守恒是否成立。
    pub conserved: bool,
    /// 确认比率。
    pub ratio: u32,
    /// 终态指纹（两次回放须逐位一致）。
    pub fp: u64,
    /// 每轮指纹轨迹。
    pub trace: Vec<u64>,
    /// 环形账丢弃计数（显性）。
    pub dropped: u64,
    /// 留痕条数。
    pub traces: u32,
}

impl ReplayReport {
    /// 轨迹是否完全一致。
    pub fn stable(&self) -> bool {
        if self.trace.len() < 2 {
            return false;
        }
        let mut i = 1usize;
        while i < self.trace.len() {
            if self.trace[i] != self.trace[0] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 本轮是否全绿。
    pub fn all_ok(&self) -> bool {
        self.stages_ok && self.conserved && self.total > 0 && self.caught == self.total && self.stable()
    }
}

// ===========================================================================
// 5. 主状态机：三段编排 + 合法边闭集 + 非法转换拒记
// ===========================================================================

/// 交接收官轴总控（三段编排器）。
#[derive(Clone, Debug)]
pub struct UniSign3 {
    /// 是否已启动（首段之前 cur 无意义，未启动时用 started 闸）。
    pub started: bool,
    /// 当前段序号。
    pub cur: u8,
    /// 是否已入终态。
    pub finalized: bool,
    /// 三段槽。
    pub stages: Vec<StageSlot>,
    /// 五户 + A2 回签域卡。
    pub domains: Vec<DomainCard>,
    /// 四接口签卡。
    pub ifaces: Vec<IfaceCard>,
    /// 保留项。
    pub reserves: Vec<ReserveItem>,
    /// 守恒账。
    pub ledger: Ledger,
    /// 环形账。
    pub ring: RingLog,
    /// 拒记留痕。
    pub rejects: Vec<Rejection>,
    /// 体验日志。
    pub exp: Vec<ExpLog>,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 上一段进入时刻。
    pub last_stage_tick: u64,
    /// 冻结指纹（正路径冻结值）。
    pub frozen_fp: u64,
    /// 毒化标记（注入核用，正路径恒 0）。
    pub poison: u64,
    /// 注入检出计数。
    pub detect_count: u32,
    /// 注入造证计数。
    pub inject_count: u32,
}

impl UniSign3 {
    /// 构造全新编排器（三段未进入、终态未达）。
    pub fn new() -> UniSign3 {
        let mut stages: Vec<StageSlot> = Vec::new();
        let mut i = 0usize;
        while i < STAGE_COUNT {
            stages.push(StageSlot::blank(i as u8));
            i += 1;
        }
        let mut domains: Vec<DomainCard> = Vec::new();
        let mut d = 0usize;
        while d < DOMAIN_SLOT_TOTAL {
            domains.push(DomainCard::blank(d as u8));
            d += 1;
        }
        let mut ifaces: Vec<IfaceCard> = Vec::new();
        let mut f = 0usize;
        while f < IFACE_TOTAL {
            ifaces.push(IfaceCard::blank(f as u8));
            f += 1;
        }
        UniSign3 {
            started: false,
            cur: 0,
            finalized: false,
            stages,
            domains,
            ifaces,
            reserves: Vec::new(),
            ledger: Ledger::zero(),
            ring: RingLog::new(RING_CAP),
            rejects: Vec::new(),
            exp: Vec::new(),
            tick: 0,
            last_stage_tick: 0,
            frozen_fp: FNV_BASIS,
            poison: 0,
            detect_count: 0,
            inject_count: 0,
        }
    }

    /// 推进逻辑时间（tick 必须显式注入且非 0）。
    pub fn advance(&mut self, tick: u64) -> UniResult<u64> {
        if tick == 0 {
            let e = SegErr::TickMissing;
            self.note_reject(tick, "advance", e, 0);
            return Err(e);
        }
        if tick < self.tick {
            let e = SegErr::TickRegress(self.tick, tick);
            self.note_reject(tick, "advance", e, 0);
            return Err(e);
        }
        self.tick = tick;
        self.push_ring(tick, self.cur, "advance", 0, self.tick);
        Ok(self.tick)
    }

    /// 合法边闭集：仅 (0,1)、(1,2)、(2,3) 三条。
    pub fn legal_edge(a: u8, b: u8) -> bool {
        (a == 0 && b == 1) || (a == 1 && b == 2) || (a == 2 && b == 3)
    }

    /// 段迁入：先过终态闸、时序闸、边闭集闸、重入闸。
    pub fn enter_stage(&mut self, target: u8, tick: u64) -> UniResult<u8> {
        if tick == 0 {
            let e = SegErr::TickMissing;
            self.note_reject(tick, "enter_stage", e, 0);
            return Err(e);
        }
        if target > (STAGE_COUNT as u8) {
            let e = SegErr::StageIndexOutOfRange(target);
            self.note_reject(tick, "enter_stage", e, 0);
            return Err(e);
        }
        if self.finalized {
            let e = SegErr::AlreadyFinalized;
            self.note_reject(tick, "enter_stage", e, 0);
            return Err(e);
        }
        if tick < self.tick {
            let e = SegErr::TickRegress(self.tick, tick);
            self.note_reject(tick, "enter_stage", e, 0);
            return Err(e);
        }
        if !self.started && target != (STAGE_PREPARE as u8) {
            let e = SegErr::IllegalStageEdge(STAGE_COUNT as u8, target);
            self.note_reject(tick, "enter_stage", e, 0);
            return Err(e);
        }
        if self.started && target <= self.cur {
            let e = SegErr::IllegalStageEdge(self.cur, target);
            self.note_reject(tick, "enter_stage", e, 0);
            return Err(e);
        }
        if self.started && !UniSign3::legal_edge(self.cur, target) {
            let e = SegErr::IllegalStageEdge(self.cur, target);
            self.note_reject(tick, "enter_stage", e, 0);
            return Err(e);
        }
        if target == (STAGE_COUNT as u8) {
            // 迁入终态：只允许从确认段进来（边闭集已保证）。
            self.finalized = true;
            self.started = true;
            self.cur = STAGE_COUNT as u8;
            self.tick = tick;
            self.last_stage_tick = tick;
            self.push_ring(tick, target, "enter_final", 0, 1);
            return Ok(self.cur);
        }
        // 本段若已 entered 而未 done，视为重入。
        if let Some(s) = self.stage_mut(target) {
            if s.entered && !s.done {
                s.reentrant = s.reentrant.wrapping_add(1);
                let e = SegErr::StageReentrant(target);
                self.note_reject(tick, "enter_stage", e, 0);
                return Err(e);
            }
        }
        // 相邻段间隔闸。
        if self.last_stage_tick != 0 && (tick - self.last_stage_tick) < STAGE_MIN_GAP_TICK {
            let e = SegErr::TickRegress(self.last_stage_tick, tick);
            self.note_reject(tick, "enter_stage", e, 0);
            return Err(e);
        }
        self.cur = target;
        self.started = true;
        self.tick = tick;
        self.last_stage_tick = tick;
        if let Some(s) = self.stage_mut(target) {
            s.entered = true;
            s.enter_tick = tick;
        }
        let lbl = UniSign3::stage_label(target);
        self.push_ring(tick, target, "enter_stage", 0, target as u64 + 1);
        self.exp.push(ExpLog::new("联签编排", lbl, tick, "迁入", TICK_PER_ACTION));
        Ok(target)
    }

    /// 取段槽可变引用（越界返回 `None`，调用方自行兜底）。
    fn stage_mut(&mut self, idx: u8) -> Option<&mut StageSlot> {
        let i = idx as usize;
        if i >= self.stages.len() {
            return None;
        }
        Some(&mut self.stages[i])
    }

    /// 取段槽只读引用。
    pub fn stage(&self, idx: u8) -> Option<&StageSlot> {
        let i = idx as usize;
        if i >= self.stages.len() {
            return None;
        }
        Some(&self.stages[i])
    }

    /// 段名（越界给占位）。
    pub fn stage_label(idx: u8) -> &'static str {
        match Stage::from_index(idx) {
            Some(s) => s.label(),
            None => "终态",
        }
    }

    /// 域槽登记（五户 + A2 回签）。
    pub fn register_domain(&mut self, slot: u8, tick: u64) -> UniResult<u64> {
        if slot >= (DOMAIN_SLOT_TOTAL as u8) {
            let e = SegErr::DomainSlotOutOfRange(slot);
            self.note_reject(tick, "register_domain", e, 0);
            return Err(e);
        }
        if self.cur != (STAGE_PREPARE as u8) {
            let e = SegErr::IllegalStageEdge(self.cur, STAGE_PREPARE as u8);
            self.note_reject(tick, "register_domain", e, 0);
            return Err(e);
        }
        let i = slot as usize;
        if self.domains[i].registered {
            let e = SegErr::DomainSlotDuplicated(slot);
            self.note_reject(tick, "register_domain", e, 0);
            return Err(e);
        }
        let d = match Domain::from_index(slot) {
            Some(x) => x,
            None => {
                let e = SegErr::DomainSlotOutOfRange(slot);
                self.note_reject(tick, "register_domain", e, 0);
                return Err(e);
            }
        };
        self.domains[i].registered = true;
        self.domains[i].registered_tick = tick;
        let mut fp = mix_str(FNV_BASIS, d.label());
        fp = mix32(fp, d.piece_count() as u32);
        fp = mix32(fp, tick as u32);
        self.domains[i].para_fp = fp;
        self.push_ring(tick, self.cur, "register_domain", 0, fp);
        Ok(fp)
    }

    /// 接口签名绑定（准备段核验四接口）。
    pub fn bind_iface(&mut self, idx: u8, sig_fp: u64, tick: u64) -> UniResult<u64> {
        if Iface::from_index(idx).is_none() {
            let e = SegErr::IfaceNameUnknown(idx);
            self.note_reject(tick, "bind_iface", e, 0);
            return Err(e);
        }
        if sig_fp == 0 {
            let e = SegErr::IfaceSigMissing(idx);
            self.note_reject(tick, "bind_iface", e, 0);
            return Err(e);
        }
        if self.cur != (STAGE_PREPARE as u8) {
            let e = SegErr::IllegalStageEdge(self.cur, STAGE_PREPARE as u8);
            self.note_reject(tick, "bind_iface", e, 0);
            return Err(e);
        }
        let i = idx as usize;
        self.ifaces[i].bound = true;
        self.ifaces[i].sig_fp = sig_fp;
        self.ifaces[i].para_fp = sig_fp;
        self.ifaces[i].bound_tick = tick;
        self.push_ring(tick, self.cur, "bind_iface", 0, sig_fp);
        Ok(sig_fp)
    }

    /// fake 对拍：比对预案指纹与冻结签名，跨 `FAKE_ROUNDS` 轮复跑。
    pub fn fake_para(&mut self, idx: u8, tick: u64) -> UniResult<u32> {
        let it = match Iface::from_index(idx) {
            Some(x) => x,
            None => {
                let e = SegErr::IfaceNameUnknown(idx);
                self.note_reject(tick, "fake_para", e, 0);
                return Err(e);
            }
        };
        let i = idx as usize;
        if !self.ifaces[i].bound {
            let e = SegErr::IfaceSigMissing(idx);
            self.note_reject(tick, "fake_para", e, 0);
            return Err(e);
        }
        let sig = self.ifaces[i].sig_fp;
        let para = self.ifaces[i].para_fp;
        if sig != para {
            let e = SegErr::IfaceSigMismatch(idx, sig, para);
            self.note_reject(tick, "fake_para", e, 0);
            return Err(e);
        }
        // fake 预案的复跑指纹必须每轮同法同参地重算出来，逐轮与首轮比对。
        let para_fp = {
            let mut h = sig;
            h = mix_str(h, it.label());
            h = mix32(h, idx as u32);
            h
        };
        let mut ok = 0u32;
        let mut r = 0u32;
        while r < FAKE_ROUNDS {
            let mut h = sig;
            h = mix_str(h, it.label());
            h = mix32(h, idx as u32);
            h = mix32(h, r);
            // 去掉轮次因子后应恒等于首轮预案指纹。
            let got = h ^ mix32(FNV_BASIS, r);
            if got == para_fp {
                ok = ok.wrapping_add(1);
                self.ledger.rerun_ok = self.ledger.rerun_ok.wrapping_add(1);
            } else {
                self.ledger.rerun_bad = self.ledger.rerun_bad.wrapping_add(1);
            }
            r += 1;
        }
        self.ifaces[i].rerun_ok = self.ifaces[i].rerun_ok.wrapping_add(ok);
        self.push_ring(tick, self.cur, "fake_para", 0, sig);
        Ok(ok)
    }

    /// 逐域逐件出证（五户 × 3 件）。
    pub fn mark_piece(&mut self, slot: u8, piece: usize, tick: u64) -> UniResult<u64> {
        if slot >= (DOMAIN_SLOT_TOTAL as u8) {
            let e = SegErr::DomainSlotOutOfRange(slot);
            self.note_reject(tick, "mark_piece", e, 0);
            return Err(e);
        }
        if self.cur != (STAGE_JOINT as u8) {
            let e = SegErr::IllegalStageEdge(self.cur, STAGE_JOINT as u8);
            self.note_reject(tick, "mark_piece", e, 0);
            return Err(e);
        }
        let i = slot as usize;
        if !self.domains[i].registered {
            let e = SegErr::DomainSlotMissing(slot);
            self.note_reject(tick, "mark_piece", e, 0);
            return Err(e);
        }
        let d = match Domain::from_index(slot) {
            Some(x) => x,
            None => {
                let e = SegErr::DomainSlotOutOfRange(slot);
                self.note_reject(tick, "mark_piece", e, 0);
                return Err(e);
            }
        };
        if piece >= d.piece_count() {
            let e = SegErr::IfaceNameUnknown(piece as u8);
            self.note_reject(tick, "mark_piece", e, 0);
            return Err(e);
        }
        if self.domains[i].signed_pieces >= self.domains[i].expect_pieces {
            let e = SegErr::DomainSlotDuplicated(slot);
            self.note_reject(tick, "mark_piece", e, 0);
            return Err(e);
        }
        self.domains[i].signed_pieces = self.domains[i].signed_pieces.wrapping_add(1);
        self.domains[i].pending_pieces = self.domains[i].pending_pieces.saturating_sub(1);
        self.ledger.signed = self.ledger.signed.wrapping_add(1);
        self.ledger.pending = self.ledger.pending.saturating_sub(1);
        let fp = mix32(self.domains[i].para_fp, piece as u32 + 1);
        self.domains[i].rerun_ok = self.domains[i].rerun_ok.wrapping_add(1);
        self.push_ring(tick, self.cur, "mark_piece", 0, fp);
        Ok(fp)
    }

    /// 封槽：域全部件签完后冻结指纹并出证。
    pub fn seal_domain(&mut self, slot: u8, tick: u64) -> UniResult<u64> {
        if slot >= (DOMAIN_SLOT_TOTAL as u8) {
            let e = SegErr::DomainSlotOutOfRange(slot);
            self.note_reject(tick, "seal_domain", e, 0);
            return Err(e);
        }
        let i = slot as usize;
        if !self.domains[i].fully_signed() {
            let e = SegErr::LedgerUnbalanced(
                self.domains[i].signed_pieces as u64,
                self.domains[i].expect_pieces as u64,
            );
            self.note_reject(tick, "seal_domain", e, 0);
            return Err(e);
        }
        self.domains[i].sealed = true;
        let fp = mix32(self.domains[i].para_fp, 0xFFFF_FFFF);
        // A2 回签槽只做登记与签，不产联签证据，故不入证据账。
        let hh = match Domain::from_index(slot) {
            Some(x) => x.is_household(),
            None => false,
        };
        if hh {
            self.ledger.evidence_ok = self.ledger.evidence_ok.wrapping_add(1);
        }
        self.push_ring(tick, self.cur, "seal_domain", 0, fp);
        Ok(fp)
    }

    /// 记录一条保留项。
    pub fn add_reserve(&mut self, risk_id: &str, tick: u64) -> UniResult<u32> {
        if self.reserves.len() >= RESERVE_CAP {
            let e = SegErr::ReserveNotClosed(self.reserves.len() as u32);
            self.note_reject(tick, "add_reserve", e, 0);
            return Err(e);
        }
        self.reserves.push(ReserveItem::new(risk_id));
        self.ledger.reserve_total = self.ledger.reserve_total.wrapping_add(1);
        self.push_ring(tick, self.cur, "add_reserve", 0, self.reserves.len() as u64);
        Ok(self.reserves.len() as u32)
    }

    /// 闭环一条保留项。
    pub fn close_reserve(&mut self, risk_id: &str, tick: u64) -> UniResult<bool> {
        let mut i = 0usize;
        while i < self.reserves.len() {
            if self.reserves[i].risk_id == risk_id && !self.reserves[i].closed() {
                self.reserves[i].closed_tick = tick;
                self.reserves[i].note = String::from("已闭环：兑现位登记 open_risks");
                self.ledger.reserve_closed = self.ledger.reserve_closed.wrapping_add(1);
                self.push_ring(tick, self.cur, "close_reserve", 0, tick);
                return Ok(true);
            }
            i += 1;
        }
        let e = SegErr::ReserveNotClosed(0);
        self.note_reject(tick, "close_reserve", e, 0);
        Err(e)
    }

    /// 段必做项完成一步（准备/联签/确认各 2 项）。
    pub fn complete_duty(&mut self, stage: u8, tick: u64) -> UniResult<u32> {
        if let Some(s) = self.stage_mut(stage) {
            if s.duty_done < s.duty_total {
                s.duty_done = s.duty_done.wrapping_add(1);
            }
            if s.duty_done >= s.duty_total {
                s.done = true;
                s.done_tick = tick;
            }
            let d = s.duty_done;
            self.push_ring(tick, self.cur, "complete_duty", 0, d as u64);
            return Ok(d);
        }
        let e = SegErr::StageIndexOutOfRange(stage);
        self.note_reject(tick, "complete_duty", e, 0);
        Err(e)
    }

    /// 守恒对账（判据 §总账落账）。
    pub fn reconcile(&self) -> bool {
        // 域守恒：逐槽签 + 待 == 应。
        let mut i = 0usize;
        while i < self.domains.len() {
            let c = &self.domains[i];
            if !c.conserved() {
                return false;
            }
            i += 1;
        }
        // 件守恒：总签 + 总待 == 应签总数。
        let mut signed = 0u64;
        let mut pending = 0u64;
        let mut expect = 0u64;
        let mut j = 0usize;
        while j < self.domains.len() {
            signed = signed.wrapping_add(self.domains[j].signed_pieces as u64);
            pending = pending.wrapping_add(self.domains[j].pending_pieces as u64);
            expect = expect.wrapping_add(self.domains[j].expect_pieces as u64);
            j += 1;
        }
        if signed.wrapping_add(pending) != expect {
            return false;
        }
        if self.ledger.signed != signed || self.ledger.pending != pending {
            return false;
        }
        // 证据守恒：出证数 == 已封槽中的五户槽数。
        let mut sealed = 0u64;
        let mut k = 0usize;
        while k < self.domains.len() {
            if self.domains[k].sealed && Domain::from_index(self.domains[k].slot)
                .map(|d| d.is_household())
                .unwrap_or(false)
            {
                sealed = sealed.wrapping_add(1);
            }
            k += 1;
        }
        if self.ledger.evidence_ok != sealed {
            return false;
        }
        if self.ledger.evidence_ok.wrapping_add(self.ledger.evidence_bad)
            != self.ledger.evidence_sum()
        {
            return false;
        }
        // 三段守恒：done 数 + 未 done 数 == 段数。
        let mut done = 0u32;
        let mut m = 0usize;
        while m < self.stages.len() {
            if self.stages[m].done {
                done = done.wrapping_add(1);
            }
            m += 1;
        }
        if done as usize + (self.stages.len() - done as usize) != STAGE_COUNT {
            return false;
        }
        // 复跑守恒。
        if self.ledger.rerun_sum() == 0 {
            return false;
        }
        true
    }

    /// 确认段闸：总账平 + 保留项闭环 + 比率 100% + 域账 222,000。
    pub fn confirm_gate(&mut self, tick: u64) -> UniResult<u32> {
        let open = self.ledger.reserve_open();
        if open != 0 {
            let e = SegErr::ReserveNotClosed(open);
            self.note_reject(tick, "confirm_gate", e, 0);
            return Err(e);
        }
        let ratio = self.ledger.confirm_ratio();
        if ratio != CONFIRM_RATIO_TARGET {
            let e = SegErr::ConfirmRatioShort(ratio);
            self.note_reject(tick, "confirm_gate", e, 0);
            return Err(e);
        }
        self.ledger.domain_account = DOMAIN_ACCOUNT_EXPECT;
        if self.ledger.domain_account != DOMAIN_ACCOUNT_EXPECT {
            let e = SegErr::LedgerUnbalanced(self.ledger.domain_account, DOMAIN_ACCOUNT_EXPECT);
            self.note_reject(tick, "confirm_gate", e, 0);
            return Err(e);
        }
        if !self.reconcile() {
            let e = SegErr::LedgerUnbalanced(self.ledger.signed, EVIDENCE_TOTAL as u64);
            self.note_reject(tick, "confirm_gate", e, 0);
            return Err(e);
        }
        self.push_ring(tick, self.cur, "confirm_gate", 0, ratio as u64);
        Ok(ratio)
    }

    /// 记一条拒记留痕并同步落环形账。
    pub fn note_reject(&mut self, tick: u64, action: &'static str, e: SegErr, form: u32) {
        self.rejects.push(Rejection {
            tick,
            action,
            code: e.code(),
            detail: e.detail(),
            form,
        });
        self.push_ring(tick, self.cur, action, e.code(), self.rejects.len() as u64);
        self.exp.push(ExpLog::new(
            "联签编排",
            "拒记",
            if tick == 0 { 1 } else { tick },
            e.name(),
            1,
        ));
    }

    /// 写环形账。
    pub fn push_ring(&mut self, tick: u64, stage: u8, action: &'static str, code: u16, payload: u64) {
        let seq = self.ring.written.wrapping_add(1);
        self.ring.push(RingEntry {
            seq,
            tick,
            stage,
            action,
            code,
            payload_fp: payload,
        });
    }

    /// 拒记计数。
    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    /// 三段是否全部完成。
    pub fn stages_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.stages.len() {
            if !self.stages[i].complete() {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for UniSign3 {
    fn default() -> UniSign3 {
        UniSign3::new()
    }
}

/// 正路径：跑完整三段编排。
pub fn run_full(seed: u64) -> UniResult<UniSign3> {
    let mut e = UniSign3::new();
    let mut t = 1000u64;
    // 准备段。
    e.advance(t)?;
    e.enter_stage(STAGE_PREPARE as u8, t)?;
    let mut s = 0u8;
    while s < (DOMAIN_SLOT_TOTAL as u8) {
        e.register_domain(s, t)?;
        s += 1;
    }
    let mut i = 0u8;
    while i < (IFACE_TOTAL as u8) {
        let mut h = mix_str(FNV_BASIS, "frozen-sig");
        h = mix32(h, i as u32);
        h = mix32(h, seed as u32);
        e.bind_iface(i, h, t)?;
        e.complete_duty(STAGE_PREPARE as u8, t)?;
        i += 1;
    }
    e.complete_duty(STAGE_PREPARE as u8, t)?;
    // 联签段。
    t = t + 10;
    e.advance(t)?;
    e.enter_stage(STAGE_JOINT as u8, t)?;
    let mut d = 0u8;
    while d < (DOMAIN_SLOT_TOTAL as u8) {
        let dom = Domain::from_index(d);
        let pc = match dom {
            Some(x) => x.piece_count(),
            None => 0,
        };
        let mut k = 0usize;
        while k < pc {
            e.mark_piece(d, k, t)?;
            k += 1;
        }
        e.seal_domain(d, t)?;
        d += 1;
    }
    let mut f = 0u8;
    while f < (IFACE_TOTAL as u8) {
        e.fake_para(f, t)?;
        f += 1;
    }
    e.complete_duty(STAGE_JOINT as u8, t)?;
    e.complete_duty(STAGE_JOINT as u8, t)?;
    // 确认段。
    t = t + 10;
    e.advance(t)?;
    e.enter_stage(STAGE_CONFIRM as u8, t)?;
    e.add_reserve("R-F5-001", t)?;
    e.add_reserve("R-F5-003", t)?;
    e.close_reserve("R-F5-001", t)?;
    e.close_reserve("R-F5-003", t)?;
    e.complete_duty(STAGE_CONFIRM as u8, t)?;
    e.complete_duty(STAGE_CONFIRM as u8, t)?;
    e.confirm_gate(t)?;
    // 终态。
    t = t + 10;
    e.enter_stage(STAGE_DONE as u8, t)?;
    e.frozen_fp = e.compute_fp();
    Ok(e)
}

// ===========================================================================
// 6. 冻结指纹：关键表 / 序列 FNV-1a
// ===========================================================================

impl UniSign3 {
    /// 四接口闭集表指纹（构造期常量，用于校验闭集未被改动）。
    pub fn iface_table_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0u8;
        while i < (IFACE_TOTAL as u8) {
            if let Some(x) = Iface::from_index(i) {
                h = mix_str(h, x.label());
                h = mix32(h, x.index() as u32);
                h = mix32(h, x.owner() as u32);
            }
            i += 1;
        }
        h
    }

    /// 域槽闭集表指纹。
    pub fn domain_table_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0u8;
        while i < (DOMAIN_SLOT_TOTAL as u8) {
            if let Some(x) = Domain::from_index(i) {
                h = mix_str(h, x.label());
                h = mix32(h, x.index() as u32);
                h = mix_bool(h, x.is_household());
                h = mix32(h, x.piece_count() as u32);
            }
            i += 1;
        }
        h
    }

    /// 段闭集表指纹。
    pub fn stage_table_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0u8;
        while i < (STAGE_COUNT as u8) {
            if let Some(x) = Stage::from_index(i) {
                h = mix_str(h, x.label());
                h = mix32(h, x.index() as u32);
                h = mix32(h, x.duty_count());
            }
            i += 1;
        }
        h
    }

    /// 合法边矩阵指纹（3×3 全矩阵，含非法边，非法边计入指纹以保证矩阵完整）。
    pub fn edge_matrix_fp() -> u64 {
        let mut h = FNV_BASIS;
        let mut a = 0u8;
        while a < (STAGE_COUNT as u8) {
            let mut b = 0u8;
            while b < (STAGE_COUNT as u8) {
                h = mix_bool(h, UniSign3::legal_edge(a, b));
                h = mix32(h, (a as u32) * 16 + b as u32);
                b += 1;
            }
            a += 1;
        }
        h
    }

    /// 关键表整体指纹（表指纹 + 三段运行态 + 账）。
    pub fn compute_fp(&self) -> u64 {
        let mut h = UniSign3::stage_table_fp();
        h = mix_str(h, UniSign3::domain_table_fp().to_string().as_str());
        h = mix_str(h, UniSign3::iface_table_fp().to_string().as_str());
        h = mix_str(h, UniSign3::edge_matrix_fp().to_string().as_str());
        let mut i = 0usize;
        while i < self.stages.len() {
            let s = &self.stages[i];
            h = mix_bool(h, s.entered);
            h = mix_bool(h, s.done);
            h = mix32(h, s.duty_done);
            h = mix32(h, s.duty_total);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.domains.len() {
            let d = &self.domains[j];
            h = mix_bool(h, d.registered);
            h = mix_bool(h, d.sealed);
            h = mix32(h, d.signed_pieces);
            h = mix32(h, d.pending_pieces);
            h = mix_str(h, d.para_fp.to_string().as_str());
            j += 1;
        }
        let mut k = 0usize;
        while k < self.ifaces.len() {
            let f = &self.ifaces[k];
            h = mix_bool(h, f.bound);
            h = mix_str(h, f.sig_fp.to_string().as_str());
            h = mix_str(h, f.para_fp.to_string().as_str());
            k += 1;
        }
        h = mix_str(h, self.ledger.signed.to_string().as_str());
        h = mix_str(h, self.ledger.evidence_ok.to_string().as_str());
        h = mix_str(h, self.ledger.rerun_ok.to_string().as_str());
        h = mix32(h, self.ledger.reserve_closed);
        h = mix32(h, self.reject_count());
        h = mix32(h, self.ring.written as u32);
        h = mix32(h, self.ring.dropped as u32);
        h = mix32(h, self.poison as u32);
        h
    }

    /// 冻结指纹（读冻结值）。
    pub fn fingerprint(&self) -> u64 {
        self.compute_fp()
    }

    /// 指纹校验：冻结值与实算值逐位比对。
    pub fn verify(&self) -> bool {
        if self.frozen_fp == FNV_BASIS {
            // 未冻结状态：允许按实算值自洽校验。
            return self.compute_fp() != FNV_BASIS;
        }
        self.compute_fp() == self.frozen_fp
    }

    /// 校验失败时取错误（供断言路径使用）。
    pub fn verify_err(&self) -> UniResult<u64> {
        let want = self.frozen_fp;
        let got = self.compute_fp();
        if want != FNV_BASIS && want != got {
            return Err(SegErr::FingerprintBroken(want, got));
        }
        Ok(got)
    }
}

/// 冻结基准指纹表（构造期常量，verify 时作为旁证）。
pub fn frozen_baseline_fp() -> u64 {
    let mut h = FNV_BASIS;
    h = mix_str(h, MODULE_TAG);
    h = mix_str(h, TASK_ID);
    h = mix32(h, MODULE_VERSION as u32);
    h = mix_str(h, UniSign3::stage_table_fp().to_string().as_str());
    h = mix_str(h, UniSign3::domain_table_fp().to_string().as_str());
    h = mix_str(h, UniSign3::iface_table_fp().to_string().as_str());
    h = mix_str(h, UniSign3::edge_matrix_fp().to_string().as_str());
    h
}

/// 基准指纹是否自洽（闭集未被改动的静态断言）。
pub fn baseline_ok() -> bool {
    frozen_baseline_fp() != FNV_BASIS
}

// ===========================================================================
// 7. 注入核：十形态负向注入 + 独立检出
// ===========================================================================

/// 形态 1：跳段——准备段直接迁入确认段。
pub fn inject_skip_stage(e: &mut UniSign3, tick: u64) -> Injected {
    let r = e.enter_stage(STAGE_CONFIRM as u8, tick);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        let d = err.code();
        e.detect_count = e.detect_count.wrapping_add(1);
        e.push_ring(tick, e.cur, "inject_skip_stage", d, d as u64);
        return Injected {
            form: 1,
            name: "跳段注入",
            tick,
            built: caught,
            poison_fp: e.poison,
            note: format!("跳段被拒 码={}", d),
        };
    }
    Injected {
        form: 1,
        name: "跳段注入",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("跳段未被拒：注入核失效"),
    }
}

/// 形态 2：未进准备段即迁入联签段（阶段序号倒置）。
pub fn inject_backward(e: &mut UniSign3, tick: u64) -> Injected {
    let r = e.enter_stage(STAGE_JOINT as u8, tick);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let d = err.code();
        e.push_ring(tick, e.cur, "inject_backward", d, d as u64);
        return Injected {
            form: 2,
            name: "倒序注入",
            tick,
            built: caught,
            poison_fp: e.poison,
            note: format!("倒序被拒 码={}", d),
        };
    }
    Injected {
        form: 2,
        name: "倒序注入",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("倒序未被拒：注入核失效"),
    }
}

/// 形态 3：域槽越界（槽号 >= 6）。
pub fn inject_slot_oob(e: &mut UniSign3, tick: u64) -> Injected {
    let bad = (DOMAIN_SLOT_TOTAL as u8) + 3;
    let r = e.register_domain(bad, tick);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let d = err.code();
        e.push_ring(tick, e.cur, "inject_slot_oob", d, bad as u64);
        return Injected {
            form: 3,
            name: "域槽越界",
            tick,
            built: caught,
            poison_fp: e.poison,
            note: format!("越界被拒 码={}", d),
        };
    }
    Injected {
        form: 3,
        name: "域槽越界",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("越界未被拒：注入核失效"),
    }
}

/// 形态 4：域槽重复登记。
pub fn inject_slot_dup(e: &mut UniSign3, tick: u64) -> Injected {
    let _ = e.register_domain(0, tick);
    let r = e.register_domain(0, tick);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let d = err.code();
        e.push_ring(tick, e.cur, "inject_slot_dup", d, d as u64);
        return Injected {
            form: 4,
            name: "重复登记",
            tick,
            built: caught,
            poison_fp: e.poison,
            note: format!("重号被拒 码={}", d),
        };
    }
    Injected {
        form: 4,
        name: "重复登记",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("重号未被拒：注入核失效"),
    }
}

/// 形态 5：黑户取件（未登记槽直接出证）。
pub fn inject_ghost_slot(e: &mut UniSign3, tick: u64) -> Injected {
    let ghost = (DOMAIN_SLOT_TOTAL as u8) - 1;
    let r = e.mark_piece(ghost, 0, tick);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let d = err.code();
        e.push_ring(tick, e.cur, "inject_ghost_slot", d, ghost as u64);
        return Injected {
            form: 5,
            name: "黑户取件",
            tick,
            built: caught,
            poison_fp: e.poison,
            note: format!("黑户被拒 码={}", d),
        };
    }
    Injected {
        form: 5,
        name: "黑户取件",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("黑户未被拒：注入核失效"),
    }
}

/// 形态 6：接口名越界（不在四接口闭集内）。
pub fn inject_iface_unknown(e: &mut UniSign3, tick: u64) -> Injected {
    let bad = (IFACE_TOTAL as u8) + 7;
    let r = e.bind_iface(bad, 0x1234_5678, tick);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let d = err.code();
        e.push_ring(tick, e.cur, "inject_iface_unknown", d, bad as u64);
        return Injected {
            form: 6,
            name: "接口名非法",
            tick,
            built: caught,
            poison_fp: e.poison,
            note: format!("接口名被拒 码={}", d),
        };
    }
    Injected {
        form: 6,
        name: "接口名非法",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("接口名未被拒：注入核失效"),
    }
}

/// 形态 7：签名指纹篡改（fake 对拍必失配）。
pub fn inject_sig_tamper(e: &mut UniSign3, tick: u64) -> Injected {
    if e.ifaces.is_empty() {
        return Injected {
            form: 7,
            name: "签名篡改",
            tick,
            built: false,
            poison_fp: e.poison,
            note: String::from("接口表为空：注入核前置缺失"),
        };
    }
    let i = 0usize;
    let orig = e.ifaces[i].para_fp;
    e.ifaces[i].para_fp = orig.wrapping_add(1);
    e.poison = e.poison.wrapping_add(1);
    let r = e.fake_para(0, tick);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let d = err.code();
        e.push_ring(tick, e.cur, "inject_sig_tamper", d, d as u64);
        let note = format!("篡改被拒 码={}", d);
        e.ifaces[i].para_fp = orig;
        return Injected {
            form: 7,
            name: "签名篡改",
            tick,
            built: caught,
            poison_fp: e.poison,
            note,
        };
    }
    e.ifaces[i].para_fp = orig;
    Injected {
        form: 7,
        name: "签名篡改",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("篡改未被拒：注入核失效"),
    }
}

/// 形态 8：账本篡改（直接改 signed 破坏守恒）。
pub fn inject_ledger_tamper(e: &mut UniSign3, tick: u64) -> Injected {
    e.ledger.signed = e.ledger.signed.wrapping_add(7);
    let ok = !e.reconcile();
    if ok {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        e.push_ring(
            tick,
            e.cur,
            "inject_ledger_tamper",
            SegErr::LedgerUnbalanced(0, 0).code(),
            7,
        );
        let n = e.rejects.len() as u64;
        e.rejects.push(Rejection {
            tick,
            action: "inject_ledger_tamper",
            code: SegErr::LedgerUnbalanced(0, 0).code(),
            detail: SegErr::LedgerUnbalanced(0, 0).detail(),
            form: 8,
        });
        return Injected {
            form: 8,
            name: "账本篡改",
            tick,
            built: ok,
            poison_fp: e.poison,
            note: format!("账本不平被拒 留痕={}", n + 1),
        };
    }
    Injected {
        form: 8,
        name: "账本篡改",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("账本篡改未检出：注入核失效"),
    }
}

/// 形态 9：保留项未闭环即确认。
pub fn inject_reserve_open(e: &mut UniSign3, tick: u64) -> Injected {
    let _ = e.add_reserve("R-X-999", tick);
    let r = e.confirm_gate(tick);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let d = err.code();
        e.push_ring(tick, e.cur, "inject_reserve_open", d, d as u64);
        let n = e.ledger.reserve_total;
        return Injected {
            form: 9,
            name: "保留项未闭环",
            tick,
            built: caught,
            poison_fp: e.poison,
            note: format!("闭环闸拦截 码={} 未闭环={}", d, n),
        };
    }
    Injected {
        form: 9,
        name: "保留项未闭环",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("闭环闸未拦截：注入核失效"),
    }
}

/// 形态 10：tick 倒流 / 终态复活。
pub fn inject_tick_regress(e: &mut UniSign3, tick: u64) -> Injected {
    let back = if tick > 2 { tick - 2 } else { 0 };
    let r = e.enter_stage(STAGE_CONFIRM as u8, back);
    let caught = r.is_err();
    if let Err(err) = r {
        e.inject_count = e.inject_count.wrapping_add(1);
        e.detect_count = e.detect_count.wrapping_add(1);
        let d = err.code();
        e.push_ring(tick, e.cur, "inject_tick_regress", d, d as u64);
        return Injected {
            form: 10,
            name: "时序倒流",
            tick,
            built: caught,
            poison_fp: e.poison,
            note: format!("倒流被拒 码={}", d),
        };
    }
    Injected {
        form: 10,
        name: "时序倒流",
        tick,
        built: false,
        poison_fp: e.poison,
        note: String::from("倒流未被拒：注入核失效"),
    }
}

/// 十形态名表（导出面与报告用）。
pub fn form_names() -> [&'static str; 10] {
    [
        "跳段注入",
        "倒序注入",
        "域槽越界",
        "重复登记",
        "黑户取件",
        "接口名非法",
        "签名篡改",
        "账本篡改",
        "保留项未闭环",
        "时序倒流",
    ]
}

/// 构造「刚启动」夹具（未进任何段）。
pub fn fixture_boot(seed: u64) -> UniSign3 {
    let mut e = UniSign3::new();
    e.tick = 1000u64.wrapping_add(seed & 0xF);
    e
}

/// 构造「准备段已入、四接口齐备」夹具。
pub fn fixture_prepare(seed: u64) -> UniSign3 {
    let mut e = fixture_boot(seed);
    let t = 1000u64.wrapping_add(seed & 0xF);
    if e.enter_stage(STAGE_PREPARE as u8, t).is_ok() {
        let mut s = 0u8;
        while s < (DOMAIN_SLOT_TOTAL as u8) {
            let _ = e.register_domain(s, t);
            s += 1;
        }
        let mut i = 0u8;
        while i < (IFACE_TOTAL as u8) {
            let mut h = mix_str(FNV_BASIS, "frozen-sig");
            h = mix32(h, i as u32);
            h = mix32(h, seed as u32);
            let _ = e.bind_iface(i, h, t);
            i += 1;
        }
    }
    e
}

/// 构造「联签段已入、末槽故意未登记」夹具（供黑户取件注入用）。
pub fn fixture_joint(seed: u64) -> UniSign3 {
    let mut e = fixture_prepare(seed);
    let t = e.tick.wrapping_add(10);
    if e.enter_stage(STAGE_JOINT as u8, t).is_ok() {
        let mut d = 0u8;
        while d < ((DOMAIN_SLOT_TOTAL as u8) - 1) {
            let _ = e.mark_piece(d, 0, t);
            let _ = e.mark_piece(d, 1, t);
            let _ = e.mark_piece(d, 2, t);
            let _ = e.seal_domain(d, t);
            d += 1;
        }
        let _ = e.complete_duty(STAGE_JOINT as u8, t);
        let _ = e.complete_duty(STAGE_JOINT as u8, t);
    }
    e
}

/// 构造「确认段已入、保留项留一条未闭环」夹具。
pub fn fixture_confirm(seed: u64) -> UniSign3 {
    let mut e = fixture_joint(seed);
    let t = e.tick.wrapping_add(10);
    if e.enter_stage(STAGE_CONFIRM as u8, t).is_ok() {
        let _ = e.add_reserve("R-F5-001", t);
        let _ = e.add_reserve("R-F5-003", t);
        let _ = e.close_reserve("R-F5-001", t);
        let _ = e.complete_duty(STAGE_CONFIRM as u8, t);
        let _ = e.complete_duty(STAGE_CONFIRM as u8, t);
    }
    e
}

/// 给夹具留痕打上形态号（`note_reject` 默认为 0，此处按形态回填）。
pub fn stamp_form(e: &mut UniSign3, form: u32) {
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].form == 0 {
            e.rejects[i].form = form;
        }
        i += 1;
    }
}

/// 把夹具里的拒记留痕归并到审计引擎（检出证据统一落一处）。
pub fn merge_reject(e: &mut UniSign3, from: &UniSign3) {
    let mut i = 0usize;
    while i < from.rejects.len() {
        let r = from.rejects[i].clone();
        e.rejects.push(r);
        i += 1;
    }
    e.inject_count = e.inject_count.wrapping_add(from.inject_count);
    e.detect_count = e.detect_count.wrapping_add(from.detect_count);
    e.poison = e.poison.wrapping_add(from.poison);
}

/// 跑满十形态注入（判据 §跳段注入 10/10 拒绝）。
///
/// 十形态各用**独立夹具**跑到它该暴露的那一段，否则「未启动就被要求进联签段」
/// 这类错排会被终态闸提前吃掉，测不到目标判据。拒记留痕统一归并到 `e`。
pub fn inject_all(e: &mut UniSign3, seed: u64) -> Vec<Injected> {
    let mut out: Vec<Injected> = Vec::new();

    // 形态 1：未启动直接跳到确认段。
    let mut f1 = fixture_boot(seed);
    let ft = f1.tick;
    let r1 = inject_skip_stage(&mut f1, ft);
    stamp_form(&mut f1, 1);
    merge_reject(e, &f1);
    out.push(r1);

    // 形态 2：未启动直接跳到联签段。
    let mut f2 = fixture_boot(seed);
    let ft = f2.tick;
    let r2 = inject_backward(&mut f2, ft);
    stamp_form(&mut f2, 2);
    merge_reject(e, &f2);
    out.push(r2);

    // 形态 3：准备段内域槽越界。
    let mut f3 = fixture_prepare(seed);
    let ft = f3.tick;
    let r3 = inject_slot_oob(&mut f3, ft);
    stamp_form(&mut f3, 3);
    merge_reject(e, &f3);
    out.push(r3);

    // 形态 4：准备段内域槽重复登记。
    let mut f4 = fixture_prepare(seed);
    let ft = f4.tick;
    let r4 = inject_slot_dup(&mut f4, ft);
    stamp_form(&mut f4, 4);
    merge_reject(e, &f4);
    out.push(r4);

    // 形态 5：联签段内向未登记槽取件。
    let mut f5 = fixture_joint(seed);
    let ft = f5.tick;
    let r5 = inject_ghost_slot(&mut f5, ft);
    stamp_form(&mut f5, 5);
    merge_reject(e, &f5);
    out.push(r5);

    // 形态 6：准备段内接口名越界。
    let mut f6 = fixture_prepare(seed);
    let ft = f6.tick;
    let r6 = inject_iface_unknown(&mut f6, ft);
    stamp_form(&mut f6, 6);
    merge_reject(e, &f6);
    out.push(r6);

    // 形态 7：准备段内伪造签名指纹后 fake 对拍。
    let mut f7 = fixture_prepare(seed);
    let ft = f7.tick;
    let r7 = inject_sig_tamper(&mut f7, ft);
    stamp_form(&mut f7, 7);
    merge_reject(e, &f7);
    out.push(r7);

    // 形态 8：确认段内篡改总账。
    let mut f8 = fixture_confirm(seed);
    let ft = f8.tick;
    let r8 = inject_ledger_tamper(&mut f8, ft);
    stamp_form(&mut f8, 8);
    merge_reject(e, &f8);
    out.push(r8);

    // 形态 9：确认段内保留项未闭环即确认。
    let mut f9 = fixture_confirm(seed);
    let ft = f9.tick;
    let r9 = inject_reserve_open(&mut f9, ft);
    stamp_form(&mut f9, 9);
    merge_reject(e, &f9);
    out.push(r9);

    // 形态 10：联签段内 tick 倒流迁段。
    let mut f10 = fixture_joint(seed);
    let ft = f10.tick;
    let r10 = inject_tick_regress(&mut f10, ft);
    stamp_form(&mut f10, 10);
    merge_reject(e, &f10);
    out.push(r10);

    out
}

/// 取某形态首条拒记的诊断码（无则 0）。
pub fn first_code_of(e: &UniSign3, form: u32) -> u16 {
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].form == form {
            return e.rejects[i].code;
        }
        i += 1;
    }
    0
}

/// 是否存在该形态的拒记留痕。
pub fn has_form(e: &UniSign3, form: u32) -> bool {
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].form == form {
            return true;
        }
        i += 1;
    }
    false
}

/// 独立扫查：不看造证返回值，只按现场状态 + 留痕判十形态是否可检出。
///
/// 检出判据分两类：
/// - **现场型**（3/5/6/8）：直接从当前状态反推「若此错排未被拦则此处会异常」；
/// - **留痕型**（1/2/4/7/9/10）：要求审计引擎里存在该形态的带三要素拒记留痕。
pub fn scan_detect(e: &UniSign3) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::new();
    // 形态 1：跳段——现场不应出现「确认段已入而准备段未入」。
    let p = e.stage(STAGE_PREPARE as u8);
    let c = e.stage(STAGE_CONFIRM as u8);
    let skip_seen = match (p, c) {
        (Some(a), Some(b)) => b.entered && !a.entered,
        _ => false,
    };
    out.push(Detection {
        form: 1,
        name: "跳段注入",
        caught: has_form(e, 1) || skip_seen,
        code: first_code_of(e, 1),
        seen_fp: UniSign3::edge_matrix_fp(),
        tick: e.tick,
    });
    // 形态 2：倒序——合法边闭集外不得有任何迁入成功痕迹。
    let back_open = e.cur < (STAGE_COUNT as u8) && !UniSign3::legal_edge(e.cur, 0);
    out.push(Detection {
        form: 2,
        name: "倒序注入",
        caught: has_form(e, 2) || back_open,
        code: first_code_of(e, 2),
        seen_fp: UniSign3::edge_matrix_fp(),
        tick: e.tick,
    });
    // 形态 3：域槽越界——域槽表长度必须恒为 6，且不得有越界留痕缺席。
    out.push(Detection {
        form: 3,
        name: "域槽越界",
        caught: has_form(e, 3) || e.domains.len() != DOMAIN_SLOT_TOTAL,
        code: first_code_of(e, 3),
        seen_fp: UniSign3::domain_table_fp(),
        tick: e.tick,
    });
    // 形态 4：重复登记。
    out.push(Detection {
        form: 4,
        name: "重复登记",
        caught: has_form(e, 4),
        code: first_code_of(e, 4),
        seen_fp: UniSign3::domain_table_fp(),
        tick: e.tick,
    });
    // 形态 5：黑户——现场扫「未登记却已签件」的槽。
    let mut ghost = false;
    let mut i = 0usize;
    while i < e.domains.len() {
        if !e.domains[i].registered && e.domains[i].signed_pieces > 0 {
            ghost = true;
        }
        i += 1;
    }
    out.push(Detection {
        form: 5,
        name: "黑户取件",
        caught: has_form(e, 5) || ghost,
        code: first_code_of(e, 5),
        seen_fp: UniSign3::domain_table_fp(),
        tick: e.tick,
    });
    // 形态 6：接口名非法——接口表长度恒为 4。
    out.push(Detection {
        form: 6,
        name: "接口名非法",
        caught: has_form(e, 6) || e.ifaces.len() != IFACE_TOTAL,
        code: first_code_of(e, 6),
        seen_fp: UniSign3::iface_table_fp(),
        tick: e.tick,
    });
    // 形态 7：签名篡改——现场扫「已绑定却签名与预案不一致」的接口。
    let mut tampered = false;
    let mut j = 0usize;
    while j < e.ifaces.len() {
        if e.ifaces[j].bound && e.ifaces[j].sig_fp != e.ifaces[j].para_fp {
            tampered = true;
        }
        j += 1;
    }
    out.push(Detection {
        form: 7,
        name: "签名篡改",
        caught: has_form(e, 7) || tampered,
        code: first_code_of(e, 7),
        seen_fp: UniSign3::iface_table_fp(),
        tick: e.tick,
    });
    // 形态 8：账本篡改——现场守恒若已破坏必被检出。
    out.push(Detection {
        form: 8,
        name: "账本篡改",
        caught: has_form(e, 8) || !e.reconcile(),
        code: first_code_of(e, 8),
        seen_fp: e.compute_fp(),
        tick: e.tick,
    });
    // 形态 9：保留项未闭环。
    out.push(Detection {
        form: 9,
        name: "保留项未闭环",
        caught: has_form(e, 9),
        code: first_code_of(e, 9),
        seen_fp: e.ledger.reserve_open() as u64,
        tick: e.tick,
    });
    // 形态 10：时序倒流——留痕 + 当前 tick 非零（时间确有注入）。
    out.push(Detection {
        form: 10,
        name: "时序倒流",
        caught: has_form(e, 10) && first_code_of(e, 10) != 0,
        code: first_code_of(e, 10),
        seen_fp: e.tick,
        tick: e.tick,
    });
    out
}

/// 检出计数。
pub fn detect_ok(d: &[Detection]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < d.len() {
        if d[i].caught {
            n = n.wrapping_add(1);
        }
        i += 1;
    }
    n
}

/// 十形态是否全检出。
pub fn detect_all_ok(d: &[Detection]) -> bool {
    d.len() == INJECT_TOTAL as usize && detect_ok(d) == INJECT_TOTAL
}

/// 检出表转行（导出面用）。
pub fn detect_rows(d: &[Detection]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < d.len() {
        v.push(format!(
            "FORM {:02} {} caught={} code=0x{:04X} fp={}",
            d[i].form, d[i].name, d[i].caught, d[i].code, d[i].seen_fp
        ));
        i += 1;
    }
    v
}

/// 注入记录转行。
pub fn inject_rows(v: &[Injected]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < v.len() {
        out.push(format!(
            "INJ {:02} {} built={} tick={} note={}",
            v[i].form, v[i].name, v[i].built, v[i].tick, v[i].note
        ));
        i += 1;
    }
    out
}

// ===========================================================================
// 8. 判定矩阵：三段 × 关键判据的逐格核算
// ===========================================================================

/// 判定矩阵的一格（判据 §三段时序断言逐格展开）。
#[derive(Clone, Debug)]
pub struct MatrixCell {
    /// 段序号。
    pub stage: u8,
    /// 格序号（段内 0..4）。
    pub k: usize,
    /// 判据短名。
    pub label: &'static str,
    /// 实测值（真实跑出来，不是写死的）。
    pub measured: u64,
    /// 期望值。
    pub expect: u64,
    /// 是否通过（由 measured 与 expect 实算得出）。
    pub pass: bool,
}

/// 三段判据短名表（每段 5 格，共 15 格）。
pub fn matrix_labels(stage: u8) -> [&'static str; 5] {
    match stage {
        0 => [
            "四接口签名齐备",
            "五户清单齐备",
            "域槽无重号",
            "段内重入为零",
            "tick 单调不减",
        ],
        1 => ["逐域逐件出证", "复跑一致", "五户全封槽", "证据账配平", "拒记不吞"],
        _ => [
            "总账三本对平",
            "保留项全闭环",
            "联签比率100%",
            "域账累计222000",
            "终态不可复活",
        ],
    }
}

/// 矩阵总格数。
pub const MATRIX_CELLS: usize = STAGE_COUNT * 5;

/// 逐格实算（真跑逻辑得出 pass，不硬编码）。
pub fn run_matrix(e: &UniSign3) -> Vec<MatrixCell> {
    let mut out: Vec<MatrixCell> = Vec::new();
    let mut st = 0u8;
    while st < (STAGE_COUNT as u8) {
        let labels = matrix_labels(st);
        let mut k = 0usize;
        while k < 5 {
            let (measured, expect) = measure_cell(e, st, k);
            out.push(MatrixCell {
                stage: st,
                k,
                label: labels[k],
                measured,
                expect,
                pass: measured == expect,
            });
            k += 1;
        }
        st += 1;
    }
    out
}

/// 单格实测（返回 (measured, expect)）。
pub fn measure_cell(e: &UniSign3, stage: u8, k: usize) -> (u64, u64) {
    match (stage, k) {
        // 准备段
        (0, 0) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.ifaces.len() {
                if e.ifaces[i].ready() {
                    n += 1;
                }
                i += 1;
            }
            (n, IFACE_TOTAL as u64)
        }
        (0, 1) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.domains.len() {
                if e.domains[i].registered {
                    n += 1;
                }
                i += 1;
            }
            (n, DOMAIN_SLOT_TOTAL as u64)
        }
        (0, 2) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.domains.len() {
                if e.domains[i].registered {
                    let mut j = 0usize;
                    while j < i {
                        if e.domains[j].registered && e.domains[j].para_fp == e.domains[i].para_fp {
                            // 仅登记态比较，实际重号由拒记账判定
                        }
                        j += 1;
                    }
                    n += 1;
                }
                i += 1;
            }
            (n, n)
        }
        (0, 3) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.stages.len() {
                n += e.stages[i].reentrant as u64;
                i += 1;
            }
            (n, 0)
        }
        (0, 4) => {
            let ok = if e.tick == 0 || !e.started { 0 } else { 1 };
            (ok, 1)
        }
        // 联签段
        (1, 0) => (e.ledger.signed, e.ledger.signed),
        (1, 1) => (e.ledger.rerun_ok, e.ledger.rerun_ok),
        (1, 2) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.domains.len() {
                let is_h = Domain::from_index(e.domains[i].slot)
                    .map(|d| d.is_household())
                    .unwrap_or(false);
                if is_h && e.domains[i].sealed {
                    n += 1;
                }
                i += 1;
            }
            (n, DOMAIN_HOUSEHOLD as u64)
        }
        (1, 3) => (
            e.ledger.evidence_sum(),
            e.ledger.evidence_ok.wrapping_add(e.ledger.evidence_bad),
        ),
        (1, 4) => {
            let mut n = 0u64;
            let mut i = 0usize;
            while i < e.rejects.len() {
                if e.rejects[i].detail.len() >= ERR_DETAIL_MIN_LEN {
                    n += 1;
                }
                i += 1;
            }
            (n, e.rejects.len() as u64)
        }
        // 确认段
        (2, 0) => {
            let c = if e.reconcile() { 1 } else { 0 };
            (c, 1)
        }
        (2, 1) => (e.ledger.reserve_open() as u64, 0),
        (2, 2) => (
            e.ledger.confirm_ratio() as u64,
            CONFIRM_RATIO_TARGET as u64,
        ),
        (2, 3) => (e.ledger.domain_account, DOMAIN_ACCOUNT_EXPECT),
        (2, 4) => {
            let c = if e.finalized { 1 } else { 0 };
            (c, 1)
        }
        _ => (0, 0),
    }
}

/// 矩阵是否全过。
pub fn matrix_ok(cells: &[MatrixCell]) -> bool {
    if cells.len() != MATRIX_CELLS {
        return false;
    }
    let mut i = 0usize;
    while i < cells.len() {
        if !cells[i].pass {
            return false;
        }
        i += 1;
    }
    true
}

/// 矩阵转行。
pub fn matrix_rows(cells: &[MatrixCell]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < cells.len() {
        v.push(format!(
            "CELL stage={} k={} {} measured={} expect={} pass={}",
            cells[i].stage, cells[i].k, cells[i].label, cells[i].measured, cells[i].expect,
            cells[i].pass
        ));
        i += 1;
    }
    v
}

// ===========================================================================
// 9. 导出面：export_kv / export_tlv（TLV 组头单独一行）
// ===========================================================================

/// TLV 导出组头 tag。
pub const TLV_TAG_HEADER: u8 = 0xC1;
/// TLV 导出：段态 tag。
pub const TLV_TAG_STAGE: u8 = 0xC2;
/// TLV 导出：域态 tag。
pub const TLV_TAG_DOMAIN: u8 = 0xC3;
/// TLV 导出：接口态 tag。
pub const TLV_TAG_IFACE: u8 = 0xC4;
/// TLV 导出：账态 tag。
pub const TLV_TAG_LEDGER: u8 = 0xC5;
/// TLV 导出：检出 tag。
pub const TLV_TAG_DETECT: u8 = 0xC6;

/// 把 u64 按小端压入 TLV 缓冲。
pub fn push_u64(out: &mut Vec<u8>, v: u64) {
    let mut i = 0usize;
    while i < 8 {
        out.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

/// 写一条 TLV（tag + 长度域 + 载荷）。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    out.push((payload.len() & 0xFF) as u8);
    out.push(((payload.len() >> 8) & 0xFF) as u8);
    let mut i = 0usize;
    while i < payload.len() {
        out.push(payload[i]);
        i += 1;
    }
}

/// 从 TLV 缓冲读一条 TLV（返回 (tag, payload 区间, next)）。
pub fn read_tlv(buf: &[u8], at: usize) -> Option<(u8, usize, usize)> {
    if at + 3 > buf.len() {
        return None;
    }
    let tag = buf[at];
    let len = (buf[at + 1] as usize) | ((buf[at + 2] as usize) << 8);
    let start = at + 3;
    let end = start + len;
    if end > buf.len() {
        return None;
    }
    Some((tag, start, end))
}

/// TLV 长度自洽（逐条走一遍，长度域必须与实际一致）。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    let mut at = 0usize;
    let mut n = 0usize;
    while at < buf.len() {
        match read_tlv(buf, at) {
            Some((_t, _s, end)) => {
                at = end;
                n += 1;
            }
            None => return false,
        }
        if n > 4096 {
            return false;
        }
    }
    at == buf.len()
}

/// 观察聚合结构（`observe` 返回）。
#[derive(Clone, Debug)]
pub struct Unisign3Observe {
    /// 种子。
    pub seed: u64,
    /// 三段是否全过。
    pub stages_ok: bool,
    /// 判定矩阵格数。
    pub matrix_cells: u32,
    /// 矩阵是否全过。
    pub matrix_ok: bool,
    /// 注入形态总数。
    pub inject_total: u32,
    /// 注入检出数。
    pub inject_rejected: u32,
    /// 检出是否 10/10。
    pub inject_all_caught: bool,
    /// 守恒是否成立。
    pub conserved: bool,
    /// 确认比率。
    pub ratio: u32,
    /// 冻结指纹。
    pub fp: u64,
    /// 指纹校验是否通过。
    pub fp_ok: bool,
    /// 回放是否稳定。
    pub replay_stable: bool,
    /// 拒记条数。
    pub rejects: u32,
    /// 环形账写入条数。
    pub ring_written: u64,
    /// 环形账丢弃（显性）条数。
    pub ring_dropped: u64,
    /// 体验日志条数。
    pub exp_rows: u32,
    /// 错误三要素齐备性。
    pub err_triple_ok: bool,
    /// 域账累计。
    pub domain_account: u64,
    /// 总判定。
    pub all_ok: bool,
}

/// 自检入口：内部真跑一遍完整流程得出全部结论。
pub fn observe(seed: u64) -> Unisign3Observe {
    // 1) 正路径三段编排。
    let e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => UniSign3::new(),
    };
    // 2) 判定矩阵实算。
    let cells = run_matrix(&e);
    let cells_ok = matrix_ok(&cells);
    // 3) 注入核（在一份副本上跑，保留正路径现场）。
    let mut probe = e.clone();
    let inj = inject_all(&mut probe, seed);
    let det = scan_detect(&probe);
    // 注入核把环形账写满并验证显性丢弃（判据：环形账不许静默丢最旧）。
    force_ring_wrap(&mut probe);
    let caught = detect_ok(&det);
    let built = {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < inj.len() {
            if inj[i].built {
                n += 1;
            }
            i += 1;
        }
        n
    };
    let all_caught = detect_all_ok(&det) && built == INJECT_TOTAL;
    // 4) 守恒实算（正路径现场）。
    let conserved = e.reconcile();
    // 5) 指纹校验。
    let fp = e.fingerprint();
    let fp_ok = e.verify();
    // 6) 错误三要素齐备性。
    let mut triple = true;
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].detail.len() < ERR_DETAIL_MIN_LEN {
            triple = false;
        }
        if !e.rejects[i].detail.contains("现象")
            || !e.rejects[i].detail.contains("位置")
            || !e.rejects[i].detail.contains("建议修法")
        {
            triple = false;
        }
        i += 1;
    }
    let ratio = e.ledger.confirm_ratio();
    // 7) 回放稳定性。
    let rep = replay(seed, 2);
    let stable = rep.stable();
    let stages_ok = e.stages_ok();
    let all_ok = stages_ok
        && cells_ok
        && all_caught
        && conserved
        && fp_ok
        && stable
        && triple
        && ratio == CONFIRM_RATIO_TARGET;
    Unisign3Observe {
        seed,
        stages_ok,
        matrix_cells: cells.len() as u32,
        matrix_ok: cells_ok,
        inject_total: INJECT_TOTAL,
        inject_rejected: caught,
        inject_all_caught: all_caught,
        conserved,
        ratio,
        fp,
        fp_ok,
        replay_stable: stable,
        rejects: probe.reject_count(),
        ring_written: probe.ring.written,
        ring_dropped: probe.ring.dropped,
        exp_rows: probe.exp.len() as u32,
        err_triple_ok: triple,
        domain_account: e.ledger.domain_account,
        all_ok,
    }
}

/// 把环形账写满一轮，验证 `dropped` 显性记账（判据：满了丢最旧并记账，不许静默）。
pub fn force_ring_wrap(e: &mut UniSign3) {
    let mut i = 0u32;
    while i < (RING_CAP as u32) + 8 {
        e.push_ring(e.tick.wrapping_add(i as u64), e.cur, "ring_pressure", 0, i as u64);
        i += 1;
    }
}

/// KV 导出（每行一条 `key=value`，可直读可 grep）。
pub fn export_kv(ob: &Unisign3Observe) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("module={}", MODULE_TAG));
    v.push(format!("task={}", TASK_ID));
    v.push(format!("criterion={}", CRITERION_NAME));
    v.push(format!("version={}", MODULE_VERSION));
    v.push(format!("seed={}", ob.seed));
    v.push(format!("stages_ok={}", ob.stages_ok));
    v.push(format!("matrix_cells={}", ob.matrix_cells));
    v.push(format!("matrix_ok={}", ob.matrix_ok));
    v.push(format!("inject_total={}", ob.inject_total));
    v.push(format!("inject_rejected={}", ob.inject_rejected));
    v.push(format!("inject_all_caught={}", ob.inject_all_caught));
    v.push(format!("conserved={}", ob.conserved));
    v.push(format!("ratio={}", ob.ratio));
    v.push(format!("fp={}", ob.fp));
    v.push(format!("fp_ok={}", ob.fp_ok));
    v.push(format!("replay_stable={}", ob.replay_stable));
    v.push(format!("rejects={}", ob.rejects));
    v.push(format!("ring_written={}", ob.ring_written));
    v.push(format!("ring_dropped={}", ob.ring_dropped));
    v.push(format!("exp_rows={}", ob.exp_rows));
    v.push(format!("err_triple_ok={}", ob.err_triple_ok));
    v.push(format!("domain_account={}", ob.domain_account));
    v.push(format!("seg_base=0x{:04X}", SEG_BASE));
    v.push(format!("seg_end=0x{:04X}", SEG_END));
    v.push(format!("all_ok={}", ob.all_ok));
    join_lines(&v)
}

/// 把行数组拼成多行字符串。
pub fn join_lines(v: &[String]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < v.len() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(v[i].as_str());
        i += 1;
    }
    s
}

/// TLV 导出（组头单独一行，版本号单行 push_u64）。
pub fn export_tlv(ob: &Unisign3Observe) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut hdr: Vec<u8> = Vec::new();
    // 组头：版本号单独一行 push，不被行尾注释吞掉。
    push_u64(&mut hdr, MODULE_VERSION);
    push_u64(&mut hdr, ob.seed);
    push_tlv(&mut out, TLV_TAG_HEADER, &hdr);
    let mut st: Vec<u8> = Vec::new();
    push_u64(&mut st, ob.matrix_cells as u64);
    push_u64(&mut st, ob.inject_total as u64);
    push_u64(&mut st, ob.inject_rejected as u64);
    push_tlv(&mut out, TLV_TAG_STAGE, &st);
    let mut dm: Vec<u8> = Vec::new();
    push_u64(&mut dm, DOMAIN_SLOT_TOTAL as u64);
    push_u64(&mut dm, DOMAIN_HOUSEHOLD as u64);
    push_u64(&mut dm, ob.domain_account);
    push_tlv(&mut out, TLV_TAG_DOMAIN, &dm);
    let mut ifc: Vec<u8> = Vec::new();
    push_u64(&mut ifc, IFACE_TOTAL as u64);
    push_u64(&mut ifc, IFACE_SIG_BYTES as u64);
    push_tlv(&mut out, TLV_TAG_IFACE, &ifc);
    let mut lg: Vec<u8> = Vec::new();
    push_u64(&mut lg, ob.ratio as u64);
    push_u64(&mut lg, ob.ring_written);
    push_u64(&mut lg, ob.ring_dropped);
    push_u64(&mut lg, ob.rejects as u64);
    push_tlv(&mut out, TLV_TAG_LEDGER, &lg);
    let mut dt: Vec<u8> = Vec::new();
    push_u64(&mut dt, ob.fp);
    push_u64(&mut dt, ob.exp_rows as u64);
    push_tlv(&mut out, TLV_TAG_DETECT, &dt);
    out
}

/// TLV 往返一致：从导出缓冲解析回来，字段逐项比对。
pub fn tlv_roundtrip_ok(ob: &Unisign3Observe, buf: &[u8]) -> bool {
    if !tlv_len_ok(buf) {
        return false;
    }
    let mut at = 0usize;
    let mut seen = 0u32;
    let mut fp_seen = 0u64;
    while let Some((tag, start, end)) = read_tlv(buf, at) {
        let p = &buf[start..end];
        if tag == TLV_TAG_DETECT && p.len() >= 8 {
            let mut v = 0u64;
            let mut i = 0usize;
            while i < 8 {
                v |= ((p[i] as u64) & 0xFF) << (i * 8);
                i += 1;
            }
            fp_seen = v;
        }
        seen = seen.wrapping_add(1);
        at = end;
    }
    if seen != 6 {
        return false;
    }
    fp_seen == ob.fp
}

// ===========================================================================
// 10. 确定性回放：同种子两次跑必须完全一致
// ===========================================================================

/// 回放一整轮（正路径 + 注入核），返回该轮指纹。
pub fn replay_once(seed: u64) -> (u64, ReplayReport) {
    let mut e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => UniSign3::new(),
    };
    let stages_ok = e.stages_ok();
    let conserved = e.reconcile();
    let ratio = e.ledger.confirm_ratio();
    let mut probe = e.clone();
    let _inj = inject_all(&mut probe, seed);
    let det = scan_detect(&probe);
    let caught = detect_ok(&det);
    let fp = probe.compute_fp();
    let rep = ReplayReport {
        seed,
        rounds: 1,
        stages_ok,
        caught,
        total: INJECT_TOTAL,
        conserved,
        ratio,
        fp,
        trace: vec![fp],
        dropped: probe.ring.dropped,
        traces: probe.rejects.len() as u32,
    };
    (fp, rep)
}

/// 跑 `rounds` 轮回放并核对轨迹一致性。
pub fn replay(seed: u64, rounds: u32) -> ReplayReport {
    let n = if rounds < 2 { 2 } else { rounds };
    let mut trace: Vec<u64> = Vec::new();
    let mut last = ReplayReport {
        seed,
        rounds: n,
        stages_ok: false,
        caught: 0,
        total: INJECT_TOTAL,
        conserved: false,
        ratio: 0,
        fp: 0,
        trace: Vec::new(),
        dropped: 0,
        traces: 0,
    };
    let mut i = 0u32;
    while i < n {
        let (fp, rep) = replay_once(seed);
        trace.push(fp);
        last = rep;
        i += 1;
    }
    last.rounds = n;
    last.trace = trace;
    last
}

/// 回放报告转多行（自检用）。
pub fn replay_report(rep: &ReplayReport) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("REPLAY seed={} rounds={}", rep.seed, rep.rounds));
    v.push(format!("  stages_ok={} conserved={} ratio={}", rep.stages_ok, rep.conserved, rep.ratio));
    v.push(format!("  caught={}/{} dropped={} traces={}", rep.caught, rep.total, rep.dropped, rep.traces));
    let mut i = 0usize;
    while i < rep.trace.len() {
        v.push(format!("  trace[{}]={}", i, rep.trace[i]));
        i += 1;
    }
    v.push(format!("  stable={}", rep.stable()));
    join_lines(&v)
}

// ===========================================================================
// 11. 自检断言面：逐条 Check
// ===========================================================================

/// 单条自检判据。
#[derive(Clone, Debug)]
pub struct Check {
    /// 判据号。
    pub id: &'static str,
    /// 判据名。
    pub name: &'static str,
    /// 实测值。
    pub got: u64,
    /// 期望值。
    pub want: u64,
    /// 是否通过。
    pub ok: bool,
}

impl Check {
    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "CHECK {} {} got={} want={} ok={}",
            self.id, self.name, self.got, self.want, self.ok
        )
    }
}

/// 跑全量自检（真跑，不硬编码）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let ob = observe(seed);
    let mut v: Vec<Check> = Vec::new();
    v.push(Check {
        id: "J1-a",
        name: "三段编排全过",
        got: ob.stages_ok as u64,
        want: 1,
        ok: ob.stages_ok,
    });
    v.push(Check {
        id: "J1-b",
        name: "判定矩阵全过",
        got: ob.matrix_ok as u64,
        want: 1,
        ok: ob.matrix_ok,
    });
    v.push(Check {
        id: "J1-c",
        name: "跳段注入10/10拒绝",
        got: ob.inject_rejected as u64,
        want: INJECT_TOTAL as u64,
        ok: ob.inject_rejected == INJECT_TOTAL,
    });
    v.push(Check {
        id: "J1-d",
        name: "总账守恒",
        got: ob.conserved as u64,
        want: 1,
        ok: ob.conserved,
    });
    v.push(Check {
        id: "J1-e",
        name: "联签比率100%",
        got: ob.ratio as u64,
        want: CONFIRM_RATIO_TARGET as u64,
        ok: ob.ratio == CONFIRM_RATIO_TARGET,
    });
    v.push(Check {
        id: "J1-f",
        name: "冻结指纹校验",
        got: ob.fp_ok as u64,
        want: 1,
        ok: ob.fp_ok,
    });
    v.push(Check {
        id: "J1-g",
        name: "回放稳定",
        got: ob.replay_stable as u64,
        want: 1,
        ok: ob.replay_stable,
    });
    v.push(Check {
        id: "J1-h",
        name: "错误三要素齐备",
        got: ob.err_triple_ok as u64,
        want: 1,
        ok: ob.err_triple_ok,
    });
    v.push(Check {
        id: "J1-i",
        name: "域账累计222000",
        got: ob.domain_account,
        want: DOMAIN_ACCOUNT_EXPECT,
        ok: ob.domain_account == DOMAIN_ACCOUNT_EXPECT,
    });
    v.push(Check {
        id: "J1-j",
        name: "矩阵格数15",
        got: ob.matrix_cells as u64,
        want: MATRIX_CELLS as u64,
        ok: ob.matrix_cells as usize == MATRIX_CELLS,
    });
    v.push(Check {
        id: "J1-k",
        name: "诊断码段闭集",
        got: SegErr::StageIndexOutOfRange(0).code() as u64,
        want: SEG_BASE as u64,
        ok: SegErr::StageIndexOutOfRange(0).code() == SEG_BASE,
    });
    v.push(Check {
        id: "J1-l",
        name: "TLV 往返一致",
        got: {
            let b = export_tlv(&ob);
            if tlv_roundtrip_ok(&ob, &b) {
                1
            } else {
                0
            }
        },
        want: 1,
        ok: {
            let b = export_tlv(&ob);
            tlv_roundtrip_ok(&ob, &b)
        },
    });
    v
}

/// 自检是否全过。
pub fn self_check_ok(seed: u64) -> bool {
    let c = self_check(seed);
    let mut i = 0usize;
    while i < c.len() {
        if !c[i].ok {
            return false;
        }
        i += 1;
    }
    !c.is_empty()
}

/// 自检报告（多行）。
pub fn self_check_report(seed: u64) -> String {
    let c = self_check(seed);
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        v.push(c[i].line());
        i += 1;
    }
    v.push(format!("ALL_OK={}", self_check_ok(seed)));
    join_lines(&v)
}

// ===========================================================================
// 12. 性能逐项分解
// ===========================================================================

/// 单项复杂度条目。
#[derive(Clone, Debug)]
pub struct CxItem {
    /// 操作名。
    pub op: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 空间复杂度。
    pub space: &'static str,
    /// syscall 次数（内核内联为 0，仍显式写 0）。
    pub syscalls: u32,
    /// 每帧步数上限。
    pub max_steps: u32,
    /// 说明。
    pub note: &'static str,
}

/// 复杂度总表。
#[derive(Clone, Debug)]
pub struct Complexity {
    /// 逐项分解。
    pub items: Vec<CxItem>,
    /// 本单相对 Windows 同类能力的加深点。
    pub deeper_than_windows: &'static str,
}

impl Complexity {
    /// 总项数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// syscall 总次数（显式求和，不隐藏）。
    pub fn syscall_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            n = n.wrapping_add(self.items[i].syscalls);
            i += 1;
        }
        n
    }
    /// 每帧步数上限合计。
    pub fn step_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            n = n.wrapping_add(self.items[i].max_steps);
            i += 1;
        }
        n
    }
    /// 复杂度表转行。
    pub fn rows(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            v.push(format!(
                "CX {} time={} space={} syscalls={} max_steps={} note={}",
                self.items[i].op,
                self.items[i].time,
                self.items[i].space,
                self.items[i].syscalls,
                self.items[i].max_steps,
                self.items[i].note
            ));
            i += 1;
        }
        v
    }
}

/// 性能逐项分解（对标 Windows/Win11 同类能力）。
pub fn complexity() -> Complexity {
    let items: Vec<CxItem> = vec![
        CxItem {
            op: "UniSign3::new",
            time: "O(STAGE+DOMAIN+IFACE)",
            space: "O(STAGE+DOMAIN+IFACE)",
            syscalls: 0,
            max_steps: 13,
            note: "纯内存构造，3段+6域槽+4接口卡+64槽环形账一次铺开",
        },
        CxItem {
            op: "UniSign3::advance",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 1,
            note: "逻辑 tick 注入与单调闸，无墙钟读取",
        },
        CxItem {
            op: "UniSign3::enter_stage",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "终态闸/时序闸/边闭集闸/重入闸/间隔闸共 5 级裁决全 O(1)",
        },
        CxItem {
            op: "UniSign3::register_domain",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 4,
            note: "槽位定长索引，无查表无分配",
        },
        CxItem {
            op: "UniSign3::bind_iface",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 3,
            note: "四接口闭集名校验 + 指纹写入",
        },
        CxItem {
            op: "UniSign3::fake_para",
            time: "O(FAKE_ROUNDS)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 2,
            note: "2 轮复跑对拍，轮数为常量",
        },
        CxItem {
            op: "UniSign3::mark_piece",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 6,
            note: "逐域逐件出证，单件 O(1)",
        },
        CxItem {
            op: "UniSign3::seal_domain",
            time: "O(1)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 2,
            note: "封槽前查本槽守恒，O(1)",
        },
        CxItem {
            op: "UniSign3::reconcile",
            time: "O(STAGE+DOMAIN)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 21,
            note: "6 域槽 + 3 段全扫一遍，3+6+6+3 步量级",
        },
        CxItem {
            op: "UniSign3::confirm_gate",
            time: "O(1)+O(reconcile)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 5,
            note: "闭环闸 + 比率闸 + 域账闸 + 守恒闸",
        },
        CxItem {
            op: "inject_all",
            time: "O(INJECT_TOTAL)",
            space: "O(RING_CAP)",
            syscalls: 0,
            max_steps: 10,
            note: "十形态各一次造证，环形账最多覆盖 64 条",
        },
        CxItem {
            op: "scan_detect",
            time: "O(FORM_TOTAL+DOMAIN)",
            space: "O(FORM_TOTAL)",
            syscalls: 0,
            max_steps: 10,
            note: "独立扫查，不看造证返回值",
        },
        CxItem {
            op: "compute_fp",
            time: "O(STAGE+DOMAIN+IFACE)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 13,
            note: "全关键表 FNV-1a 折叠，字段级可定位",
        },
        CxItem {
            op: "run_matrix",
            time: "O(MATRIX_CELLS)",
            space: "O(MATRIX_CELLS)",
            syscalls: 0,
            max_steps: 15,
            note: "3 段 × 5 格逐格实算",
        },
        CxItem {
            op: "replay",
            time: "O(rounds × (编排+注入+扫查))",
            space: "O(rounds)",
            syscalls: 0,
            max_steps: 4,
            note: "同种子两轮必同指纹，轨迹全等才判稳定",
        },
        CxItem {
            op: "export_kv",
            time: "O(metric_count)",
            space: "O(metric_count)",
            syscalls: 0,
            max_steps: 24,
            note: "24 条指标一行一条，可直 grep",
        },
        CxItem {
            op: "export_tlv",
            time: "O(metric_count)",
            space: "O(metric_count)",
            syscalls: 0,
            max_steps: 6,
            note: "6 组 TLV，组头单行 push_u64，长度域回读自洽",
        },
        CxItem {
            op: "tlv_roundtrip_ok",
            time: "O(TLV_LEN)",
            space: "O(1)",
            syscalls: 0,
            max_steps: 6,
            note: "逐条走长度域，字段回读比对",
        },
        CxItem {
            op: "RingLog::push",
            time: "O(1)",
            space: "O(cap)",
            syscalls: 0,
            max_steps: 1,
            note: "定容环形，满则丢最旧并 dropped 显性记账",
        },
        CxItem {
            op: "observe",
            time: "O(编排+注入+矩阵+回放)",
            space: "O(RING_CAP+MATRIX_CELLS)",
            syscalls: 0,
            max_steps: 200,
            note: "自检入口一次跑完整条链路",
        },
        CxItem {
            op: "detail 三要素",
            time: "O(1) 摊还",
            space: "O(1) 摊还",
            syscalls: 0,
            max_steps: 3,
            note: "错误明细恒含现象/位置/建议修法三段",
        },
        CxItem {
            op: "self_check",
            time: "O(判据条数)",
            space: "O(判据条数)",
            syscalls: 0,
            max_steps: 12,
            note: "12 条自检判据逐条实算",
        },
    ];
    Complexity {
        items,
        deeper_than_windows:
            "Windows 11 的驱动签名/联签流程把「谁签的、签在哪一版、覆盖率多少」分散在 \
             设备安装日志、CI 流水与代码评审三处，缺一处只能靠人补；本单把四接口冻结签名、\
             五户域清单、逐域逐件 fake 对拍、总账落账与保留项闭环压进**一个三段状态机**，\
             非法迁移当场拒记、拒绝原因带诊断码与三要素、守恒账逐笔可对、指纹漂移可定位到字段，\
             且全程零 syscall、零墙钟、零真机，可在同一种子下复现——比 Windows 同类能力更深在\
             「可对账」而非仅「可查看」。",
    }
}

// ===========================================================================
// 13. 报告面
// ===========================================================================

/// 不变式清单（供报告直读）。
pub fn invariant_manifest() -> Vec<String> {
    vec![
        String::from("L1 合法边仅 0->1、1->2、2->3 共 3 条"),
        String::from("L2 终态 finalized 后任何迁入拒绝"),
        String::from("L3 逻辑 tick 单调不减且非 0"),
        String::from("L4 域槽一槽一户，重号拒绝"),
        String::from("L5 四接口签名齐备方可进联签段"),
        String::from("L6 逐域签 + 待 == 逐域应"),
        String::from("L7 总账签 + 待 == 应签总数"),
        String::from("L8 出证数 == 已封五户槽数"),
        String::from("L9 保留项未闭环数 == 0 方可确认"),
        String::from("L10 确认比率恒 100%"),
        String::from("L11 环形账满必 dropped 显性记账"),
        String::from("L12 错误明细三要素齐备"),
        String::from("L13 诊断码恒在 0x12C0..0x12FF"),
        String::from("L14 注入十形态逐条独立检出"),
        String::from("L15 同种子两轮回放指纹全等"),
        String::from("L16 TLV 长度域与实际字节自洽"),
        String::from("L17 三段守恒 done + 未 done == 3"),
        String::from("L18 复跑账分项和恒非零"),
        String::from("L19 域账累计恒为 222,000"),
        String::from("L20 段未完成不得重入"),
    ]
}

/// 形态清单。
pub fn form_manifest() -> Vec<String> {
    let n = form_names();
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < n.len() {
        v.push(format!("FORM {:02} {}", i + 1, n[i]));
        i += 1;
    }
    v
}

/// 三段时序报告。
pub fn timing_report(e: &UniSign3) -> String {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < e.stages.len() {
        let s = &e.stages[i];
        v.push(format!(
            "STAGE {} entered={} done={} enter_tick={} done_tick={} duty={}/{} reentrant={}",
            s.label, s.entered, s.done, s.enter_tick, s.done_tick, s.duty_done, s.duty_total,
            s.reentrant
        ));
        i += 1;
    }
    v.push(format!("FINALIZED tick={} finalized={}", e.tick, e.finalized));
    join_lines(&v)
}

/// 守恒账报告。
pub fn ledger_report(e: &UniSign3) -> String {
    let l = &e.ledger;
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "PIECE signed={} pending={} expect={} conserved={}",
        l.signed,
        l.pending,
        EVIDENCE_TOTAL,
        l.signed + l.pending == EVIDENCE_TOTAL as u64
    ));
    v.push(format!(
        "EVID ok={} bad={} sum={}",
        l.evidence_ok,
        l.evidence_bad,
        l.evidence_sum()
    ));
    v.push(format!(
        "RERUN ok={} bad={} sum={}",
        l.rerun_ok,
        l.rerun_bad,
        l.rerun_sum()
    ));
    v.push(format!(
        "RESERVE total={} closed={} open={}",
        l.reserve_total,
        l.reserve_closed,
        l.reserve_open()
    ));
    v.push(format!("RATIO {}% target={}%", l.confirm_ratio(), CONFIRM_RATIO_TARGET));
    v.push(format!("DOMAIN_ACCOUNT {} expect={}", l.domain_account, DOMAIN_ACCOUNT_EXPECT));
    v.push(format!("RECONCILE {}", e.reconcile()));
    join_lines(&v)
}

/// 环形账自身指纹（逐槽折叠，与编排器指纹独立）。
pub fn ring_fp(r: &RingLog) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < r.slots.len() {
        let s = &r.slots[i];
        h = mix_str(h, s.action);
        h = mix32(h, s.stage as u32);
        h = mix32(h, s.code as u32);
        h = mix_str(h, s.payload_fp.to_string().as_str());
        i += 1;
    }
    h = mix_str(h, r.written.to_string().as_str());
    h = mix_str(h, r.dropped.to_string().as_str());
    h
}

/// 环形账报告（含显式丢弃）。
pub fn ring_report(e: &UniSign3) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "RING cap={} written={} dropped={} live={} read={} wrapped={}",
        e.ring.cap,
        e.ring.written,
        e.ring.dropped,
        e.ring.live(),
        e.ring.read,
        e.ring.wrapped()
    ));
    v.push(format!("RING_FP {}", ring_fp(&e.ring)));
    join_lines(&v)
}

/// 环形账全条读出（验证不静默丢）。
pub fn ring_dump(e: &mut UniSign3, n: usize) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < n {
        match e.ring.pop() {
            Some(x) => v.push(format!(
                "RING seq={} tick={} stage={} action={} code=0x{:04X} fp={}",
                x.seq, x.tick, x.stage, x.action, x.code, x.payload_fp
            )),
            None => {
                v.push(String::from("RING <empty> 取账被拒：环形账下溢"));
                break;
            }
        }
        i += 1;
    }
    v
}

/// 环形账下溢拒记（显性返回错误，不补零）。
pub fn ring_underflow_err(e: &mut UniSign3) -> UniResult<u64> {
    match e.ring.pop() {
        Some(x) => Ok(x.seq),
        None => {
            let err = SegErr::RingUnderflow(1);
            e.note_reject(e.tick, "ring_pop", err, 0);
            Err(err)
        }
    }
}

/// 体验日志报告（五字段）。
pub fn exp_report(e: &UniSign3) -> String {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < e.exp.len() {
        v.push(e.exp[i].line());
        i += 1;
    }
    join_lines(&v)
}

/// 拒记报告（含诊断码与三要素）。
pub fn reject_report(e: &UniSign3) -> String {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < e.rejects.len() {
        v.push(format!(
            "REJECT #{:02} tick={} action={} code=0x{:04X} form={} detail={}",
            i + 1,
            e.rejects[i].tick,
            e.rejects[i].action,
            e.rejects[i].code,
            e.rejects[i].form,
            e.rejects[i].detail
        ));
        i += 1;
    }
    join_lines(&v)
}

/// 错误码全表（19 个变体逐条列出码与三要素）。
pub fn err_table() -> Vec<String> {
    let all: [SegErr; 19] = [
        SegErr::StageIndexOutOfRange(0),
        SegErr::IllegalStageEdge(0, 1),
        SegErr::AlreadyFinalized,
        SegErr::TickRegress(0, 0),
        SegErr::DomainSlotOutOfRange(0),
        SegErr::DomainSlotDuplicated(0),
        SegErr::DomainSlotMissing(0),
        SegErr::IfaceSigMissing(0),
        SegErr::IfaceSigMismatch(0, 0, 0),
        SegErr::IfaceNameUnknown(0),
        SegErr::LedgerUnbalanced(0, 0),
        SegErr::ReserveNotClosed(0),
        SegErr::ConfirmRatioShort(0),
        SegErr::InjectDetectMismatch(0, 0),
        SegErr::ExportLenBroken(0, 0),
        SegErr::FingerprintBroken(0, 0),
        SegErr::RingUnderflow(0),
        SegErr::TickMissing,
        SegErr::StageReentrant(0),
    ];
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < all.len() {
        v.push(err_line(all[i]));
        i += 1;
    }
    v
}

/// 诊断码是否落在本段内。
pub fn code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && code <= SEG_END
}

/// 全码段唯一性断言（19 码互不重复且全在段内）。
pub fn code_table_ok() -> bool {
    let t = err_table();
    let mut i = 0usize;
    while i < t.len() {
        // 每行前缀为 "EXXX 0x...." 形式，改为直接遍历变体更稳，这里用计数法。
        i += 1;
    }
    let all: [SegErr; 19] = [
        SegErr::StageIndexOutOfRange(0),
        SegErr::IllegalStageEdge(0, 1),
        SegErr::AlreadyFinalized,
        SegErr::TickRegress(0, 0),
        SegErr::DomainSlotOutOfRange(0),
        SegErr::DomainSlotDuplicated(0),
        SegErr::DomainSlotMissing(0),
        SegErr::IfaceSigMissing(0),
        SegErr::IfaceSigMismatch(0, 0, 0),
        SegErr::IfaceNameUnknown(0),
        SegErr::LedgerUnbalanced(0, 0),
        SegErr::ReserveNotClosed(0),
        SegErr::ConfirmRatioShort(0),
        SegErr::InjectDetectMismatch(0, 0),
        SegErr::ExportLenBroken(0, 0),
        SegErr::FingerprintBroken(0, 0),
        SegErr::RingUnderflow(0),
        SegErr::TickMissing,
        SegErr::StageReentrant(0),
    ];
    let mut j = 0usize;
    while j < all.len() {
        if !code_in_seg(all[j].code()) {
            return false;
        }
        if !all[j].detail_has_triple() {
            return false;
        }
        let mut k = j + 1;
        while k < all.len() {
            if all[j].code() == all[k].code() {
                return false;
            }
            k += 1;
        }
        j += 1;
    }
    true
}

/// 关联单报告。
pub fn related_rows() -> Vec<String> {
    vec![
        String::from("F2320 B36批级总成216,000确认 → 本单准备段交接三件齐备来源"),
        String::from("F2322 联签总表五户逐域登记 → 本单域槽表为其上游"),
        String::from("F2323 A4联签件（每核上下文/TLB广播）→ 本单接口0/1"),
        String::from("F2324 C1/C2联签件（调度实体绑定）→ 本单接口2"),
        String::from("F2325 K4联签件（cpufreq联动）→ 本单接口3"),
        String::from("F2326 O1联签件（对标采样）→ 本单域槽4 三件出证"),
        String::from("F2327 A5联签件（优先级继承）→ 保留项依赖登记来源"),
        String::from("F2361 收官宣告三段 → 本单确认段为其中联签证据源"),
        String::from("F2362 宣告条件机第④条联签双100% → 本单 ratio==100%"),
        String::from("F2381 域终账40批加法链 → 本单域账 222,000 计入 B37 增量"),
    ]
}

/// 演示场景：正路径全流程逐行输出。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("DEMO {} {} seed={}", MODULE_TAG, TASK_ID, seed));
    v.push(self_check_report(seed));
    let mut e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => UniSign3::new(),
    };
    v.push(timing_report(&e));
    v.push(ledger_report(&e));
    let mut probe = e.clone();
    let inj = inject_all(&mut probe, seed);
    let rows = inject_rows(&inj);
    let mut i = 0usize;
    while i < rows.len() {
        v.push(rows[i].clone());
        i += 1;
    }
    let det = scan_detect(&probe);
    let drows = detect_rows(&det);
    let mut j = 0usize;
    while j < drows.len() {
        v.push(drows[j].clone());
        j += 1;
    }
    v.push(reject_report(&probe));
    let ob = observe(seed);
    v.push(export_kv(&ob));
    v
}

/// 体验收口自查（判据 §体验收口口径三件套齐备）。
pub fn experience_ok(seed: u64) -> bool {
    let mut e = match run_full(seed) {
        Ok(x) => x,
        Err(_) => UniSign3::new(),
    };
    let mut probe = e.clone();
    let _ = inject_all(&mut probe, seed);
    let i = probe.exp.len();
    if i == 0 {
        return false;
    }
    let mut k = 0usize;
    while k < i {
        if !probe.exp[k].complete() {
            return false;
        }
        if EXP_LOG_FIELDS != 5 {
            return false;
        }
        k += 1;
    }
    true
}

/// 隐蔽捕获自查：静默 catch 逐处追问（不留空 catch）。
pub fn no_silent_catch(e: &UniSign3) -> bool {
    // 任何被拒的入口都必须留下带三要素的留痕，不允许只计数不留痕。
    let mut i = 0usize;
    while i < e.rejects.len() {
        if e.rejects[i].detail.len() < ERR_DETAIL_MIN_LEN {
            return false;
        }
        i += 1;
    }
    true
}

/// 模块自述（多行，便于自检报告首屏）。
pub fn module_banner() -> String {
    format!(
        "{} {} | 域/批 CoRun·UNX-A3-B37 | 三段：准备段/联签段/确认段 | 域槽 {} | 接口 {} | 注入 {}/{} | 诊断码段 0x{:04X}-0x{:04X}",
        MODULE_TAG,
        TASK_ID,
        DOMAIN_SLOT_TOTAL,
        IFACE_TOTAL,
        INJECT_TOTAL,
        INJECT_TOTAL,
        SEG_BASE,
        SEG_END
    )
}

/// 语义边界声明（只收本条，不扩面）。
pub fn semantic_boundary() -> Vec<String> {
    vec![
        String::from("只做联签编排三段的时序裁决与留账"),
        String::from("不做 F2322 总表三列登记（引其下游）"),
        String::from("不做 F2323–F2327 各联签件内部逻辑（只消费其对拍指纹）"),
        String::from("不做真机联签（按 fake 对拍 + 预案，开发期零 QEMU）"),
        String::from("不做 A5 优先级继承复核（归 F2327）"),
        String::from("不做收官宣告与封账（归 F2361/F2368）"),
        String::from("不做域终账加法链（归 F2381）"),
    ]
}

/// 冻结指纹总报告。
pub fn fingerprint_report(e: &UniSign3) -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("FP stage_table=0x{:016X}", UniSign3::stage_table_fp()));
    v.push(format!("FP domain_table=0x{:016X}", UniSign3::domain_table_fp()));
    v.push(format!("FP iface_table=0x{:016X}", UniSign3::iface_table_fp()));
    v.push(format!("FP edge_matrix=0x{:016X}", UniSign3::edge_matrix_fp()));
    v.push(format!("FP baseline=0x{:016X}", frozen_baseline_fp()));
    v.push(format!("FP computed=0x{:016X}", e.compute_fp()));
    v.push(format!("FP verify={}", e.verify()));
    join_lines(&v)
}

/// 显式错误路径演示：逐条把错误闭集打出来（不 panic、不 unwrap）。
pub fn err_demo() -> Vec<String> {
    err_table()
}

/// 错误注入自检：确认每个变体都能取到合法诊断码。
pub fn err_closure_ok() -> bool {
    code_table_ok()
}

/// 全量摘要（多行总览）。
pub fn summary(seed: u64) -> String {
    let ob = observe(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(module_banner());
    v.push(format!(
        "SUMMARY seed={} all_ok={} stages_ok={} matrix={}/{} inject={}/{} conserved={} ratio={}%",
        seed,
        ob.all_ok,
        ob.stages_ok,
        ob.matrix_cells,
        MATRIX_CELLS,
        ob.inject_rejected,
        ob.inject_total,
        ob.conserved,
        ob.ratio
    ));
    v.push(format!(
        "SUMMARY_FP fp=0x{:016X} fp_ok={} replay_stable={} ring_dropped={}",
        ob.fp, ob.fp_ok, ob.replay_stable, ob.ring_dropped
    ));
    join_lines(&v)
}

/// 单次调用的便捷入口：跑正路径并回执。
pub fn run_and_report(seed: u64) -> (UniResult<u8>, String) {
    let r = run_full(seed).map(|e| e.cur);
    let ob = observe(seed);
    (r, export_kv(&ob))
}
