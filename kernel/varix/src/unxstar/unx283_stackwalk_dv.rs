//! unx283_stackwalk_dv —— UNX-F0283 栈回溯帧步行器（rBP 链 + 保守扫描混合）· 深化版。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B15）：UNX-F0283 · 栈回溯帧步行器（rBP 链 + 保守扫描混合） · 目标 340 行 · 已深化
//! 判据原文：UNX-F0283-J1 链式回溯与保守扫描混合策略在栈损坏场景仍出有效帧，误帧率 0
//! 组件：栈窗读取器（StackWindow）+ 文本区间表（TextRangeTable）+ 链式步行器（walk_chain）
//!      + 保守扫描器（scan_conservative）+ 混合裁决（walk）+ 损坏账（CorruptionEvent 追加式）
//!      + 报告指纹（FNV-1a 全量渲染字节）。
//!      （与并行速写版 unx283_stackwalk 双实现并存，模块名独立，深化版为准收单留痕。）
//! 职责定位：内核 panic/审计路径的栈回溯底座——rBP 链快但易被栈损坏打断，
//!   保守扫描慢但抗损坏；本器以「链断即扫、扫而必证」混合策略保证：
//!   报出的每一帧要么链上实证（Verified），要么扫描证据分达阈值（Plausible），
//!   证据不足的候选只进统计不进帧表——误帧率恒 0 由构造保证并由独立复核函数验证。
//! 性能逐项分解：链式 O(帧数)（每帧 2 次 8B 读 + 6 项守卫）；扫描 O(窗字数)
//!   （每字 1 次读 + 文本区间二分 O(log R)）；去重 O(已收帧数)（帧上限 128 封顶）；
//!   指纹 O(渲染字节数)；全部整数算术，零浮点零墙钟，同输入恒同输出。
//! 诊断码段：0x96（0x9601~0x960C，全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟），
//!   逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（来源：施工书 UNX-F0283 深化正文 + 本仓步行器纪律；写死并注释）
// ---------------------------------------------------------------------------

/// 链式步行单次最大跳数：防死循环硬上限（栈深 128 已覆盖内核最深调用链）。
pub const MAX_CHAIN_HOPS: usize = 128;
/// 保守扫描最多接受的帧数（扫描是补救通道，不喧宾夺主）。
pub const MAX_SCAN_CANDIDATES: usize = 64;
/// 报告帧表硬上限（链+扫合计）。
pub const MAX_REPORTED_FRAMES: usize = 128;
/// 扫描候选入选证据分下限：低于此分只计拒绝统计，绝不入帧表（误帧率 0 的第一道闸）。
pub const MIN_ACCEPT_SCORE: u32 = 70;
/// 栈指针对齐粒度：x86_64 ABI 要求 rBP 8 字节对齐（SysV ABI §3.2.2）。
pub const STACK_ALIGN: u64 = 8;
/// 返回地址最小对齐粒度：指令边界按 4 字节起步（宽于 1 字节可显著压误帧）。
pub const RET_MIN_ALIGN: u64 = 4;
/// 宽对齐加分粒度：16 字节对齐的候选更可能是真返回地址（编译器栈帧对齐惯例）。
pub const RET_WIDE_ALIGN: u64 = 16;
/// 扫描步长：按 8 字节字扫描（u64 栈槽）。
pub const SCAN_STEP: u64 = 8;
/// 证据分：文本区间命中底分。
pub const SCORE_TEXT_HIT: u32 = 50;
/// 证据分：紧邻 call 指令前缀实证（ret-5 处 0xE8 = call rel32）。
pub const SCORE_CALL_PREFIX: u32 = 30;
/// 证据分：16 字节宽对齐。
pub const SCORE_WIDE_ALIGN: u32 = 10;
/// 证据分：位于扫描窗外的合理代码区（栈上执行不可信，反向扣分）。
pub const SCORE_IN_STACK: u32 = 10;
/// 去重账容量：链上已见 rBP 环检测账（128 全量封顶）。
pub const CYCLE_LEDGER_CAP: usize = 128;

// ---------------------------------------------------------------------------
// 诊断码段 0x96（0x9601~0x960C 闭集；域内独占，先入库者占段）
// ---------------------------------------------------------------------------

/// 0x9601 链上 rBP 为 0：链自然到底或被清零——链通道正常终止信号之一。
pub const C_RBP_NULL: u16 = 0x9601;
/// 0x9602 rBP 非规范地址（高 16 位既非全 0 也非全 1）：链损坏。
pub const C_RBP_NONCANON: u16 = 0x9602;
/// 0x9603 rBP 或其帧数据越出栈窗：链损坏或窗给小了。
pub const C_RBP_OUTSIDE: u16 = 0x9603;
/// 0x9604 rBP 非严格单调上升：帧链倒退/平走，链损坏。
pub const C_RBP_NONMONO: u16 = 0x9604;
/// 0x9605 返回地址不在任何已登记文本区间：不可证实为代码指针。
pub const C_RET_NOT_TEXT: u16 = 0x9605;
/// 0x9606 返回地址违反最小指令对齐：不可证实为指令边界。
pub const C_RET_MISALIGNED: u16 = 0x9606;
/// 0x9607 链环检测命中：saved_rBP 与近期已见值重复。
pub const C_CHAIN_CYCLE: u16 = 0x9607;
/// 0x9608 扫描候选低于入选证据分：只计拒绝，不入帧表。
pub const C_SCAN_WEAK: u16 = 0x9608;
/// 0x9609 扫描/链预算耗尽：候选数达上限。
pub const C_SCAN_BUDGET: u16 = 0x9609;
/// 0x960A 栈窗为空：无任何可回溯材料。
pub const C_WINDOW_EMPTY: u16 = 0x960A;
/// 0x960B rBP 违反 8 字节栈对齐。
pub const C_RBP_MISALIGNED: u16 = 0x960B;
/// 0x960C 报告帧表达上限：后续帧不再收集（截断显性，不静默）。
pub const C_FRAME_CAP: u16 = 0x960C;

/// 段守卫：全部诊断码必须落 0x96 段（防码段漂移/撞段）。
pub const fn seg_guard() -> bool {
    (C_RBP_NULL >> 8) == 0x96
        && (C_RBP_NONCANON >> 8) == 0x96
        && (C_RBP_OUTSIDE >> 8) == 0x96
        && (C_RBP_NONMONO >> 8) == 0x96
        && (C_RET_NOT_TEXT >> 8) == 0x96
        && (C_RET_MISALIGNED >> 8) == 0x96
        && (C_CHAIN_CYCLE >> 8) == 0x96
        && (C_SCAN_WEAK >> 8) == 0x96
        && (C_SCAN_BUDGET >> 8) == 0x96
        && (C_WINDOW_EMPTY >> 8) == 0x96
        && (C_RBP_MISALIGNED >> 8) == 0x96
        && (C_FRAME_CAP >> 8) == 0x96
}

/// 诊断码 → 人话（可操作建议一体：先说发生了什么，再说下一步怎么办）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_RBP_NULL => "rBP 为 0：链到底；若预期还有帧则改走扫描通道复核",
        C_RBP_NONCANON => "rBP 非规范地址：链损坏；改保守扫描",
        C_RBP_OUTSIDE => "帧数据越出栈窗：链损坏或窗过小；改保守扫描并核对窗界",
        C_RBP_NONMONO => "rBP 倒退/平走：帧链损坏；改保守扫描",
        C_RET_NOT_TEXT => "返回地址不在文本区间：不可证实；候选降级为拒绝统计",
        C_RET_MISALIGNED => "返回地址违反指令对齐：不可证实；候选降级为拒绝统计",
        C_CHAIN_CYCLE => "链环命中：saved_rBP 重复；改保守扫描",
        C_SCAN_WEAK => "扫描候选证据不足：不入帧表（误帧率 0 红线）",
        C_SCAN_BUDGET => "扫描预算耗尽：候选封顶；如需更多帧先扩窗",
        C_WINDOW_EMPTY => "栈窗为空：核对窗基址与长度后重试",
        C_RBP_MISALIGNED => "rBP 违反 8 字节对齐：链损坏；改保守扫描",
        C_FRAME_CAP => "帧表达上限：截断显性入账；如需更深回溯调大上限",
        _ => "未知诊断码：核对 0x96 段闭集",
    }
}

// ---------------------------------------------------------------------------
// 文本区间表：返回地址合法性论证的唯一依据
// ---------------------------------------------------------------------------

/// 单条文本区间：[lo, hi) 半开区间 + 域标签 id（0 = 未命名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub lo: u64,
    pub hi: u64,
    pub tag: u32,
}

impl TextRange {
    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.lo && addr < self.hi
    }
}

/// 文本区间表：登记制（只增），区间有序按 lo 升序，二分查找。
/// 错误路径：lo>=hi 拒登、与既有区间重叠拒登（重叠会让「命中」论证失去唯一出处）；
/// 每次拒绝计数入账（rejected），守门行为可审计。
#[derive(Clone, Debug, Default)]
pub struct TextRangeTable {
    ranges: Vec<TextRange>,
    rejected: usize,
}

impl TextRangeTable {
    pub fn new() -> Self {
        Self { ranges: Vec::new(), rejected: 0 }
    }

    /// 登记一条区间。Ok(插入序) / Err(原因码：1=lo>=hi 2=与前驱重叠 3=与后继重叠)。
    pub fn add(&mut self, lo: u64, hi: u64, tag: u32) -> Result<usize, u8> {
        if lo >= hi {
            self.rejected += 1;
            return Err(1);
        }
        // 有序表插入点：第一个 lo 大于待插 lo 的位置。
        let mut idx = self.ranges.len();
        for (i, r) in self.ranges.iter().enumerate() {
            if r.lo > lo {
                idx = i;
                break;
            }
        }
        if idx > 0 && self.ranges[idx - 1].hi > lo {
            self.rejected += 1;
            return Err(2);
        }
        if idx < self.ranges.len() && self.ranges[idx].lo < hi {
            self.rejected += 1;
            return Err(3);
        }
        self.ranges.insert(idx, TextRange { lo, hi, tag });
        Ok(idx)
    }

    /// 命中查询：二分找最后一个 lo <= addr 的区间再验 hi（区间不重叠 ⇒ 至多一条）。
    pub fn lookup(&self, addr: u64) -> Option<TextRange> {
        let mut lo = 0usize;
        let mut hi = self.ranges.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.ranges[mid].lo <= addr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let r = self.ranges[lo - 1];
        if r.contains(addr) {
            Some(r)
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn rejected_count(&self) -> usize {
        self.rejected
    }
}

// ---------------------------------------------------------------------------
// 栈窗：调用方给定的确定性栈镜像（模型内存，零真实读）
// ---------------------------------------------------------------------------

/// 栈窗 = [base, base+len) 的字节镜像。read_u64 只读窗内完整 8 字节，
/// 越界/残字一律 None——绝不返回半截数据（异常零静默的读取侧形态）。
#[derive(Clone, Debug)]
pub struct StackWindow {
    base: u64,
    bytes: Vec<u8>,
}

impl StackWindow {
    pub fn new(base: u64, bytes: Vec<u8>) -> Self {
        Self { base, bytes }
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.len()
    }

    /// 窗内读 8 字节（小端）。越界或残字返回 None。
    pub fn read_u64(&self, addr: u64) -> Option<u64> {
        if addr < self.base || addr + 8 > self.base + self.len() {
            return None;
        }
        let off = (addr - self.base) as usize;
        let mut v: u64 = 0;
        for i in 0..8 {
            v |= (self.bytes[off + i] as u64) << (i * 8);
        }
        Some(v)
    }

    /// 窗内读单字节（call 前缀取证用）。越界返回 None。
    pub fn read_u8(&self, addr: u64) -> Option<u8> {
        if addr < self.base || addr >= self.base + self.len() {
            return None;
        }
        Some(self.bytes[(addr - self.base) as usize])
    }
}

// ---------------------------------------------------------------------------
// 帧模型：来源 / 裁决 / 帧 / 损坏账
// ---------------------------------------------------------------------------

/// 帧来源：链上实证（Chain）或保守扫描补救（Conservative）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOrigin {
    Chain,
    Conservative,
}

impl FrameOrigin {
    pub const fn tag(&self) -> &'static str {
        match self {
            FrameOrigin::Chain => "chain",
            FrameOrigin::Conservative => "scan",
        }
    }
}

/// 帧裁决：Verified = 链上实证；Plausible = 扫描证据分达阈值；
/// Rejected 只进统计绝不进帧表（本模块以构造保证帧表内无 Rejected）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameVerdict {
    Verified,
    Plausible,
}

impl FrameVerdict {
    pub const fn tag(&self) -> &'static str {
        match self {
            FrameVerdict::Verified => "verified",
            FrameVerdict::Plausible => "plausible",
        }
    }
}

/// 单个回溯帧：返回地址 + 所在 rBP + 来源 + 裁决 + 证据分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkedFrame {
    pub depth: usize,
    pub ret_addr: u64,
    pub rbp: u64,
    pub origin: FrameOrigin,
    pub verdict: FrameVerdict,
    pub score: u32,
}

/// 损坏事件：追加式账（append-only），code + 发生深度 + 人话细节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorruptionEvent {
    pub code: u16,
    pub depth: usize,
    pub detail: String,
}

impl CorruptionEvent {
    fn new(code: u16, depth: usize, detail: &str) -> Self {
        Self { code, depth, detail: detail.to_string() }
    }
}

/// 停机原因闭集：为什么不再继续收帧（每一态都可解释、可对拍）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// 链自然走完（saved_rBP = 0 哨兵）。
    ChainSentinel,
    /// 链断，已转保守扫描收尾。
    ScanFallback,
    /// 扫描预算耗尽。
    ScanBudget,
    /// 栈窗为空，无材料。
    WindowEmpty,
    /// 帧表达上限。
    FrameCap,
    /// 链与扫描双通道皆无产出。
    NothingFound,
}

impl StopReason {
    pub const fn tag(&self) -> &'static str {
        match self {
            StopReason::ChainSentinel => "chain-sentinel",
            StopReason::ScanFallback => "scan-fallback",
            StopReason::ScanBudget => "scan-budget",
            StopReason::WindowEmpty => "window-empty",
            StopReason::FrameCap => "frame-cap",
            StopReason::NothingFound => "nothing-found",
        }
    }
}

/// 统计账：双通道全程计数，供报告与判据对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WalkStats {
    pub chain_hops: usize,
    pub chain_frames: usize,
    pub scan_words: usize,
    pub scan_candidates: usize,
    pub scan_accepted: usize,
    pub scan_rejected: usize,
    pub corruption_count: usize,
}

impl WalkStats {
    /// 守恒式：扫描候选 ≡ 收下 + 拒收（缺一项即账破）。
    pub const fn scan_conserves(&self) -> bool {
        self.scan_candidates == self.scan_accepted + self.scan_rejected
    }

    /// 守恒式：帧表 ≡ 链帧 + 扫帧（来源二分，账实一致）。
    pub fn frames_conserves(&self, frames: usize) -> bool {
        self.chain_frames + self.scan_accepted == frames
    }
}

/// 步行配置：全部显性可调（当前策略值，可调）。
#[derive(Clone, Copy, Debug)]
pub struct WalkConfig {
    pub max_hops: usize,
    pub max_scan_candidates: usize,
    pub max_frames: usize,
    pub min_accept_score: u32,
}

impl WalkConfig {
    pub const fn defaults() -> Self {
        Self {
            max_hops: MAX_CHAIN_HOPS,
            max_scan_candidates: MAX_SCAN_CANDIDATES,
            max_frames: MAX_REPORTED_FRAMES,
            min_accept_score: MIN_ACCEPT_SCORE,
        }
    }
}

/// 步行报告：帧表 + 损坏账 + 统计 + 停机原因 + 截断旗。
#[derive(Clone, Debug)]
pub struct WalkReport {
    pub frames: Vec<WalkedFrame>,
    pub events: Vec<CorruptionEvent>,
    pub stats: WalkStats,
    pub stop: StopReason,
    pub truncated: bool,
}

impl WalkReport {
    /// 误帧率复核（独立于构造路径的第二双眼睛）：
    /// 帧表内不得出现不可论证帧——Verified 必须文本命中，
    /// Plausible 必须证据分达阈值且文本命中；链帧必须 Verified。
    /// 任一违反 = 误帧率非 0 = 本器失守。
    pub fn verify_no_misframe(&self, text: &TextRangeTable, min_score: u32) -> bool {
        for f in &self.frames {
            match f.verdict {
                FrameVerdict::Verified => {
                    if text.lookup(f.ret_addr).is_none() {
                        return false;
                    }
                }
                FrameVerdict::Plausible => {
                    if f.score < min_score || text.lookup(f.ret_addr).is_none() {
                        return false;
                    }
                }
            }
            if f.origin == FrameOrigin::Chain && f.verdict != FrameVerdict::Verified {
                return false;
            }
            if f.origin == FrameOrigin::Conservative && f.verdict != FrameVerdict::Plausible {
                return false;
            }
        }
        true
    }

    /// FNV-1a 指纹：覆盖帧表 + 事件码 + 统计 + 停机原因 + 截断旗。
    /// 同输入恒同指纹；损坏一处必变指纹（对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for f in &self.frames {
            for b in f.ret_addr.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            for b in f.rbp.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h ^= match f.origin {
                FrameOrigin::Chain => 1,
                FrameOrigin::Conservative => 2,
            };
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= match f.verdict {
                FrameVerdict::Verified => 3,
                FrameVerdict::Plausible => 4,
            };
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            for b in f.score.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        for e in &self.events {
            for b in e.code.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            for b in (e.depth as u64).to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        for b in self.stats.chain_hops.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for b in self.stats.scan_words.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for b in self.stats.scan_candidates.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= match self.stop {
            StopReason::ChainSentinel => 10,
            StopReason::ScanFallback => 11,
            StopReason::ScanBudget => 12,
            StopReason::WindowEmpty => 13,
            StopReason::FrameCap => 14,
            StopReason::NothingFound => 15,
        };
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.truncated as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 确定性渲染：一行一帧/一事件，行首键控可解析，供转储与对拍。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("walk stop=");
        s.push_str(self.stop.tag());
        s.push_str(" truncated=");
        s.push_str(if self.truncated { "1" } else { "0" });
        s.push_str(" fp=");
        s.push_str(&alloc::format!("{:016x}", self.fingerprint()));
        s.push('\n');
        s.push_str(&alloc::format!(
            "stats hops={} cframes={} words={} cands={} acc={} rej={} corr={}\n",
            self.stats.chain_hops,
            self.stats.chain_frames,
            self.stats.scan_words,
            self.stats.scan_candidates,
            self.stats.scan_accepted,
            self.stats.scan_rejected,
            self.stats.corruption_count,
        ));
        for f in &self.frames {
            s.push_str(&alloc::format!(
                "frame d={} ret={:#x} rbp={:#x} src={} v={} score={}\n",
                f.depth,
                f.ret_addr,
                f.rbp,
                f.origin.tag(),
                f.verdict.tag(),
                f.score,
            ));
        }
        for e in &self.events {
            s.push_str(&alloc::format!(
                "corr code={:#06x} d={} {}\n",
                e.code, e.depth, e.detail
            ));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 混合步行器本体
// ---------------------------------------------------------------------------

/// 步行器：持配置 + 文本区间表。全部方法 &self（无内部可变态），可复用。
#[derive(Clone, Debug)]
pub struct StackWalker {
    cfg: WalkConfig,
    text: TextRangeTable,
}

impl StackWalker {
    pub fn new(text: TextRangeTable) -> Self {
        Self { cfg: WalkConfig::defaults(), text }
    }

    pub fn with_config(mut self, cfg: WalkConfig) -> Self {
        self.cfg = cfg;
        self
    }

    pub fn text(&self) -> &TextRangeTable {
        &self.text
    }

    pub fn config(&self) -> &WalkConfig {
        &self.cfg
    }

    /// 混合入口：链式先行，链断转扫描，扫描证据裁决收尾。
    /// 不变量（由构造保证）：
    ///   ① 帧表内无不可论证帧（verify_no_misframe 恒真）；
    ///   ② 扫描账守恒（候选 ≡ 收 + 拒）；
    ///   ③ 帧表守恒（链帧 + 扫帧 ≡ 总帧）；
    ///   ④ 损坏必入账（零静默）。
    pub fn walk(&self, mem: &StackWindow, rbp0: u64) -> WalkReport {
        let mut frames: Vec<WalkedFrame> = Vec::new();
        let mut events: Vec<CorruptionEvent> = Vec::new();
        let mut stats = WalkStats::default();
        let mut cycle_ledger: Vec<u64> = Vec::new();

        if mem.is_empty() {
            events.push(CorruptionEvent::new(C_WINDOW_EMPTY, 0, code_text(C_WINDOW_EMPTY)));
            stats.corruption_count = 1;
            return WalkReport {
                frames,
                events,
                stats,
                stop: StopReason::WindowEmpty,
                truncated: false,
            };
        }

        // —— 阶段一：链式回溯 ——
        let chain_stop =
            self.walk_chain(mem, rbp0, &mut frames, &mut events, &mut stats, &mut cycle_ledger);

        // —— 阶段二：链断（或预算耗尽）转保守扫描 ——
        let stop = match chain_stop {
            ChainStop::Sentinel => {
                // 链完好走完：无需扫描（扫描是补救通道，链实证优先）。
                StopReason::ChainSentinel
            }
            ChainStop::Broke | ChainStop::Budget => {
                self.scan_conservative(mem, &mut frames, &mut events, &mut stats);
                if stats.scan_candidates >= self.cfg.max_scan_candidates {
                    StopReason::ScanBudget
                } else if frames.is_empty() {
                    StopReason::NothingFound
                } else {
                    StopReason::ScanFallback
                }
            }
        };

        let truncated = frames.len() >= self.cfg.max_frames;
        if truncated {
            events.push(CorruptionEvent::new(C_FRAME_CAP, frames.len(), code_text(C_FRAME_CAP)));
        }
        WalkReport { frames, events, stats, stop, truncated }
    }

    /// 链式回溯：rbp0 起沿 saved_rBP 链上行，每帧六项守卫全过才收帧。
    /// 收帧只认全证据：saved_rBP 与返回地址双读成功 + 返回地址文本命中 + 对齐，
    /// 差一项即链断转扫描（不收半证据帧——链帧必须是 Verified）。
    fn walk_chain(
        &self,
        mem: &StackWindow,
        rbp0: u64,
        frames: &mut Vec<WalkedFrame>,
        events: &mut Vec<CorruptionEvent>,
        stats: &mut WalkStats,
        cycle_ledger: &mut Vec<u64>,
    ) -> ChainStop {
        let mut rbp = rbp0;
        let mut prev_rbp: Option<u64> = None;
        let mut hops = 0usize;

        loop {
            hops += 1;
            if hops > self.cfg.max_hops {
                stats.chain_hops = hops - 1;
                events.push(CorruptionEvent::new(
                    C_SCAN_BUDGET,
                    frames.len(),
                    "链跳数预算耗尽：转扫描补底（预算显性，不静默截断）",
                ));
                return ChainStop::Budget;
            }

            // 守卫①：空指针哨兵 = 链自然到底。
            if rbp == 0 {
                stats.chain_hops = hops - 1;
                events.push(CorruptionEvent::new(C_RBP_NULL, frames.len(), code_text(C_RBP_NULL)));
                return ChainStop::Sentinel;
            }

            // 守卫②：规范地址（x86_64 四级页表 48 位规范形）。
            let top = rbp >> 47;
            if top != 0 && top != 0x1FFFF {
                stats.chain_hops = hops - 1;
                events.push(CorruptionEvent::new(
                    C_RBP_NONCANON,
                    frames.len(),
                    code_text(C_RBP_NONCANON),
                ));
                return ChainStop::Broke;
            }

            // 守卫③：窗界。
            if !mem.contains(rbp) {
                stats.chain_hops = hops - 1;
                events.push(CorruptionEvent::new(
                    C_RBP_OUTSIDE,
                    frames.len(),
                    code_text(C_RBP_OUTSIDE),
                ));
                return ChainStop::Broke;
            }

            // 守卫④：8 字节栈对齐。
            if rbp % STACK_ALIGN != 0 {
                stats.chain_hops = hops - 1;
                events.push(CorruptionEvent::new(
                    C_RBP_MISALIGNED,
                    frames.len(),
                    code_text(C_RBP_MISALIGNED),
                ));
                return ChainStop::Broke;
            }

            // 守卫⑤：单调严格上升（栈向下长，帧链向上走）。
            if let Some(prev) = prev_rbp {
                if rbp <= prev {
                    stats.chain_hops = hops - 1;
                    events.push(CorruptionEvent::new(
                        C_RBP_NONMONO,
                        frames.len(),
                        code_text(C_RBP_NONMONO),
                    ));
                    return ChainStop::Broke;
                }
            }

            // 守卫⑥：环检测（saved_rBP 已见即环，防 A→B→A 死循环）。
            if cycle_ledger.contains(&rbp) {
                stats.chain_hops = hops - 1;
                events.push(CorruptionEvent::new(
                    C_CHAIN_CYCLE,
                    frames.len(),
                    code_text(C_CHAIN_CYCLE),
                ));
                return ChainStop::Broke;
            }
            if cycle_ledger.len() < CYCLE_LEDGER_CAP {
                cycle_ledger.push(rbp);
            }

            // 读帧：[rbp] = saved_rBP，[rbp+8] = 返回地址。残读即链断。
            let saved_rbp = match mem.read_u64(rbp) {
                Some(v) => v,
                None => {
                    stats.chain_hops = hops - 1;
                    events.push(CorruptionEvent::new(
                        C_RBP_OUTSIDE,
                        frames.len(),
                        "帧数据残缺（saved_rBP 越窗）：链断",
                    ));
                    return ChainStop::Broke;
                }
            };
            let ret = match mem.read_u64(rbp + 8) {
                Some(v) => v,
                None => {
                    stats.chain_hops = hops - 1;
                    events.push(CorruptionEvent::new(
                        C_RBP_OUTSIDE,
                        frames.len(),
                        "帧数据残缺（返回地址越窗）：链断",
                    ));
                    return ChainStop::Broke;
                }
            };

            // 帧数据双验证：返回地址必须文本命中且最小对齐。
            if self.text.lookup(ret).is_none() {
                stats.chain_hops = hops - 1;
                events.push(CorruptionEvent::new(
                    C_RET_NOT_TEXT,
                    frames.len(),
                    code_text(C_RET_NOT_TEXT),
                ));
                return ChainStop::Broke;
            }
            if ret % RET_MIN_ALIGN != 0 {
                stats.chain_hops = hops - 1;
                events.push(CorruptionEvent::new(
                    C_RET_MISALIGNED,
                    frames.len(),
                    code_text(C_RET_MISALIGNED),
                ));
                return ChainStop::Broke;
            }

            // 全守卫通过：收链帧（链帧证据分恒满分：文本+call 前缀+宽对齐等效）。
            stats.chain_frames += 1;
            let depth = frames.len();
            frames.push(WalkedFrame {
                depth,
                ret_addr: ret,
                rbp,
                origin: FrameOrigin::Chain,
                verdict: FrameVerdict::Verified,
                score: SCORE_TEXT_HIT + SCORE_CALL_PREFIX + SCORE_WIDE_ALIGN + SCORE_IN_STACK,
            });
            if frames.len() >= self.cfg.max_frames {
                stats.chain_hops = hops;
                return ChainStop::Sentinel;
            }

            prev_rbp = Some(rbp);
            rbp = saved_rbp;
        }
    }

    /// 保守扫描：整窗按 8 字节步进找「像返回地址」的字，
    /// 证据分四项累加，达阈值才入帧表（误帧率 0 的主闸）；
    /// 证据不足者计入拒绝账并出 0x9608，绝不入帧表。
    fn scan_conservative(
        &self,
        mem: &StackWindow,
        frames: &mut Vec<WalkedFrame>,
        events: &mut Vec<CorruptionEvent>,
        stats: &mut WalkStats,
    ) {
        // 已收返回地址集合（链帧 + 扫帧去重）。
        let mut seen: Vec<u64> = Vec::new();
        for f in frames.iter() {
            seen.push(f.ret_addr);
        }

        let mut candidates: Vec<WalkedFrame> = Vec::new();
        let base = mem.base();
        let words = mem.len() / SCAN_STEP;
        let mut w = 0u64;
        while w < words {
            stats.scan_words += 1;
            let addr = base + w * SCAN_STEP;
            let value = match mem.read_u64(addr) {
                Some(v) => v,
                None => {
                    w += 1;
                    continue;
                }
            };
            w += 1;

            // 候选预筛：非零、规范地址才继续取证。
            if value == 0 {
                continue;
            }
            let top = value >> 47;
            if top != 0 && top != 0x1FFFF {
                continue;
            }

            if stats.scan_candidates >= self.cfg.max_scan_candidates {
                events.push(CorruptionEvent::new(
                    C_SCAN_BUDGET,
                    frames.len(),
                    code_text(C_SCAN_BUDGET),
                ));
                break;
            }
            stats.scan_candidates += 1;

            // —— 证据取证（四项）——
            let mut score: u32 = 0;
            // ① 文本命中底分。
            if self.text.lookup(value).is_some() {
                score += SCORE_TEXT_HIT;
            }
            // ② call 前缀实证：ret-5 处 0xE8（call rel32）或 ret-2 处 0xFF。
            let prefix_e8 = mem.read_u8(value.wrapping_sub(5)) == Some(0xE8);
            let prefix_ff = mem.read_u8(value.wrapping_sub(2)) == Some(0xFF);
            if prefix_e8 || prefix_ff {
                score += SCORE_CALL_PREFIX;
            }
            // ③ 对齐：16 字节宽对齐满分，4 字节半分。
            if value % RET_WIDE_ALIGN == 0 {
                score += SCORE_WIDE_ALIGN;
            } else if value % RET_MIN_ALIGN == 0 {
                score += SCORE_WIDE_ALIGN / 2;
            }
            // ④ 栈上执行不可信：落在栈窗内的候选反向扣分；窗外合理区加分。
            if mem.contains(value) || mem.contains(value.wrapping_sub(1)) {
                score = score.saturating_sub(SCORE_IN_STACK * 2);
            } else {
                score += SCORE_IN_STACK;
            }

            // 去重：与已收帧/同轮候选重复的字不重复计帧。
            if seen.contains(&value) || candidates.iter().any(|c| c.ret_addr == value) {
                stats.scan_rejected += 1;
                events.push(CorruptionEvent::new(
                    C_SCAN_WEAK,
                    frames.len(),
                    "扫描候选与已收帧重复：不重复计帧",
                ));
                continue;
            }

            // 裁决：证据分达阈值才收（Rejected 永不入帧表——误帧率 0 主闸）。
            if score < self.cfg.min_accept_score {
                stats.scan_rejected += 1;
                events.push(CorruptionEvent::new(
                    C_SCAN_WEAK,
                    frames.len(),
                    &alloc::format!(
                        "{}（score={} 阈值={}）",
                        code_text(C_SCAN_WEAK),
                        score,
                        self.cfg.min_accept_score
                    ),
                ));
                continue;
            }

            stats.scan_accepted += 1;
            candidates.push(WalkedFrame {
                depth: 0,
                ret_addr: value,
                rbp: addr.wrapping_sub(8), // 扫描帧无可靠 rBP，记候选槽址-8 供参考
                origin: FrameOrigin::Conservative,
                verdict: FrameVerdict::Plausible,
                score,
            });
            seen.push(value);
        }

        // 稳定排序：分高在前；同分按返回地址升序（确定性，双跑一致）。
        candidates.sort_by(|a, b| b.score.cmp(&a.score).then(a.ret_addr.cmp(&b.ret_addr)));
        for mut c in candidates {
            c.depth = frames.len();
            frames.push(c);
            if frames.len() >= self.cfg.max_frames {
                break;
            }
        }
    }
}

/// 链通道终态（内部）。
enum ChainStop {
    Sentinel,
    Broke,
    Budget,
}

// ---------------------------------------------------------------------------
// 合成栈镜像构造器：判据自检用（也是调用方集成测试的样板）
// ---------------------------------------------------------------------------

/// 在确定性字节镜像上搭帧链：push_frame 写 [rbp]=prev、[rbp+8]=ret，
/// 返回新 rbp（帧链向上：新帧 rbp 大于旧帧）。损坏注入用 write_u64/poison。
pub struct StackImageBuilder {
    base: u64,
    bytes: Vec<u8>,
}

impl StackImageBuilder {
    pub fn new(base: u64, size: usize) -> Self {
        Self { base, bytes: alloc::vec![0u8; size] }
    }

    /// 建一帧：分配 16 字节槽，写 saved_rBP 与返回地址，返回新 rbp。
    pub fn push_frame(&mut self, prev_rbp: u64, ret: u64) -> u64 {
        let rbp = self.alloc_slot(16);
        self.write_u64(rbp, prev_rbp);
        self.write_u64(rbp + 8, ret);
        rbp
    }

    fn alloc_slot(&mut self, n: u64) -> u64 {
        let off = self.bytes.len() as u64;
        for _ in 0..n {
            self.bytes.push(0);
        }
        self.base + off
    }

    /// 直接写一个 u64（损坏注入用：可把任意槽改成任意值）。
    pub fn write_u64(&mut self, addr: u64, v: u64) {
        let off = (addr - self.base) as usize;
        for i in 0..8 {
            self.bytes[off + i] = (v >> (i * 8)) as u8;
        }
    }

    /// 直接写单字节（call 前缀取证构造用）。
    pub fn write_u8(&mut self, addr: u64, v: u8) {
        let off = (addr - self.base) as usize;
        self.bytes[off] = v;
    }

    /// 毒化区：填充 0xCC（模拟栈损坏面）。
    pub fn poison(&mut self, addr: u64, n: u64) {
        let off = (addr - self.base) as usize;
        for i in 0..n as usize {
            if off + i < self.bytes.len() {
                self.bytes[off + i] = 0xCC;
            }
        }
    }

    /// 暴露镜像长度（对账用）。
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    pub fn finish(self) -> StackWindow {
        StackWindow::new(self.base, self.bytes)
    }
}

// ---------------------------------------------------------------------------
// 文本区间样板：内核态 + 模块态两段（判据自检与调用方共用）
// ---------------------------------------------------------------------------

/// 样板文本表：ktext [0x401000,0x4FF000) + mod [0x600000,0x610000)。
pub fn sample_text_table() -> TextRangeTable {
    let mut t = TextRangeTable::new();
    let _ = t.add(0x0040_1000, 0x004F_F000, 1); // ktext
    let _ = t.add(0x0060_0000, 0x0061_0000, 2); // module
    t
}

// ---------------------------------------------------------------------------
// 自检（逐检查带 detail；域聚合在 mod.rs）
// ---------------------------------------------------------------------------

/// C1：健康链全链回收——5 帧链式全 Verified，哨兵到底，账守恒，误帧 0。
fn check_c1_healthy_chain() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
    let mut rbp = 0u64;
    let mut rets: Vec<u64> = Vec::new();
    for i in 0..5u64 {
        let ret = 0x0040_1000 + i * 0x40;
        rets.push(ret);
        rbp = img.push_frame(rbp, ret);
    }
    let mem = img.finish();
    let report = walker.walk(&mem, rbp);
    let ok_frames = report.frames.len() == 5
        && report
            .frames
            .iter()
            .all(|f| f.origin == FrameOrigin::Chain && f.verdict == FrameVerdict::Verified && rets.contains(&f.ret_addr));
    let ok_stop = report.stop == StopReason::ChainSentinel && !report.truncated;
    let ok_stats = report.stats.chain_frames == 5
        && report.stats.scan_candidates == 0
        && report.stats.corruption_count >= 1 // C_RBP_NULL 哨兵入账（零静默）
        && report.stats.scan_conserves()
        && report.stats.frames_conserves(report.frames.len());
    let ok_misframe = report.verify_no_misframe(&sample_text_table(), MIN_ACCEPT_SCORE);
    ok_frames && ok_stop && ok_stats && ok_misframe
}

/// C2：链断转扫描——深度 2 处 saved_rBP 被改写为非规范值，
/// 链收 2 帧即断（0x9602 入账），扫描补回收残余有效返回地址，全程误帧 0。
fn check_c2_corrupt_fallback() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
    let mut rbps: Vec<u64> = Vec::new();
    let mut rbp = 0u64;
    for i in 0..6u64 {
        let ret = 0x0040_1000 + i * 0x40;
        rbp = img.push_frame(rbp, ret);
        rbps.push(rbp);
    }
    // 毒化下标 2 帧的 saved_rBP 槽：写非规范值（>>47 = 0x7FFFE）。
    let bad = 0xFFFF_8000_0000_0000u64;
    img.write_u64(rbps[2], bad);
    let mem = img.finish();
    let report = walker.walk(&mem, rbps[5]);
    let chain_ok = report.stats.chain_frames >= 2
        && report.frames.iter().take(2).all(|f| f.origin == FrameOrigin::Chain);
    let scan_ok = report.stats.scan_accepted >= 1
        && report
            .frames
            .iter()
            .any(|f| f.origin == FrameOrigin::Conservative && f.verdict == FrameVerdict::Plausible);
    let event_ok = report.events.iter().any(|e| e.code == C_RBP_NONCANON);
    let stop_ok = report.stop == StopReason::ScanFallback;
    let misframe_ok = report.verify_no_misframe(&sample_text_table(), MIN_ACCEPT_SCORE);
    chain_ok && scan_ok && event_ok && stop_ok && misframe_ok
}

/// C3：链环检测——saved_rBP 成环（A↔B），0x9607 入账后转扫描，有界步数内终止。
fn check_c3_cycle_detected() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
    let r1 = img.push_frame(0, 0x0040_1000); // 帧 A
    let r2 = img.push_frame(r1, 0x0040_1040); // 帧 B（B.saved=A）
    // 成环：A.saved 改指 B（B.saved 又指 A）。
    img.write_u64(r1, r2);
    let mem = img.finish();
    let report = walker.walk(&mem, r2);
    let cycle_ok = report.events.iter().any(|e| e.code == C_CHAIN_CYCLE);
    let bounded_ok = report.stats.chain_hops <= MAX_CHAIN_HOPS + 1;
    let misframe_ok = report.verify_no_misframe(&sample_text_table(), MIN_ACCEPT_SCORE);
    cycle_ok && bounded_ok && misframe_ok
}

/// C4：空窗显性——零材料时 0x960A 入账、零帧报告、不谎报帧。
fn check_c4_empty_window() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    let mem = StackWindow::new(0x7F00_0000, Vec::new());
    let report = walker.walk(&mem, 0x7F00_0100);
    report.stop == StopReason::WindowEmpty
        && report.frames.is_empty()
        && report.events.iter().any(|e| e.code == C_WINDOW_EMPTY)
        && report.verify_no_misframe(&sample_text_table(), MIN_ACCEPT_SCORE)
}

/// C5：返回地址不可证——链上某帧返回地址不在文本区间，
/// 链断（0x9605 入账）且该地址绝不入帧表（误帧率 0 红线正向验证）。
fn check_c5_ret_not_text() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
    let r1 = img.push_frame(0, 0x0040_1000);
    let r2 = img.push_frame(r1, 0x0055_5000); // 不在任何文本区间
    let mem = img.finish();
    let report = walker.walk(&mem, r2);
    let event_ok = report.events.iter().any(|e| e.code == C_RET_NOT_TEXT);
    let not_reported = !report.frames.iter().any(|f| f.ret_addr == 0x0055_5000);
    event_ok && not_reported
}

/// C6：指纹确定性——同镜像双走指纹恒等；破坏一个返回地址指纹必变。
fn check_c6_fingerprint_determinism() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    let build = || {
        let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
        let mut rbp = 0u64;
        for i in 0..4u64 {
            rbp = img.push_frame(rbp, 0x0040_1000 + i * 0x40);
        }
        (img, rbp)
    };
    let (img1, rbp1) = build();
    let mem1 = img1.finish();
    let rep_a = walker.walk(&mem1, rbp1);
    let (mut img2, rbp2) = build();
    img2.write_u64(rbp2 + 8, 0x0060_0100); // 改最深帧返回地址为模块段
    let mem2 = img2.finish();
    let rep_b = walker.walk(&mem2, rbp2);
    let (img3, rbp3) = build();
    let mem3 = img3.finish();
    let rep_c = walker.walk(&mem3, rbp3);
    rep_a.fingerprint() == rep_c.fingerprint() && rep_a.fingerprint() != rep_b.fingerprint()
}

/// C7：渲染可解析——渲染行数 = 头 + 统计 + 帧 + 事件，逐行键控可对拍。
fn check_c7_render_parseable() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
    let mut rbp = 0u64;
    for i in 0..3u64 {
        rbp = img.push_frame(rbp, 0x0040_1000 + i * 0x40);
    }
    let mem = img.finish();
    let report = walker.walk(&mem, rbp);
    let s = report.render();
    let mut lines = s.lines();
    let head_ok = lines.next().map_or(false, |l| l.starts_with("walk stop="));
    let stats_ok = lines.next().map_or(false, |l| l.starts_with("stats hops="));
    let mut frame_lines = 0usize;
    let mut corr_lines = 0usize;
    for l in lines {
        if l.starts_with("frame d=") {
            frame_lines += 1;
        } else if l.starts_with("corr code=") {
            corr_lines += 1;
        }
    }
    frame_lines == report.frames.len()
        && corr_lines == report.events.len()
        && head_ok
        && stats_ok
}

/// C8：扫描证据闸——无 call 前缀、非宽对齐的纯文本命中候选
/// 证据分 50+5+10=65 < 70 阈值，必须被拒收（不计帧、计拒绝账出 0x9608）。
fn check_c8_scan_gate() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    // 空链窗（rbp0=0 直接哨兵），窗内埋一个纯文本命中值：
    // 0x0040_1234（4 对齐但非 16 对齐、无 call 前缀）。
    let mut img = StackImageBuilder::new(0x7F00_0000, 0x200);
    img.write_u64(0x7F00_0100, 0x0040_1234);
    let mem = img.finish();
    let report = walker.walk(&mem, 0);
    let rejected_ok = report.stats.scan_rejected >= 1;
    let not_reported = !report.frames.iter().any(|f| f.ret_addr == 0x0040_1234);
    let event_ok = report.events.iter().any(|e| e.code == C_SCAN_WEAK);
    rejected_ok && not_reported && event_ok
}

/// C9：扫描强证入选——ret-5 处 0xE8 call 前缀 + 16 对齐 + 文本命中
/// = 50+30+10+10 = 100 分，必收（正向验证闸不误杀），账守恒。
fn check_c9_scan_strong_accept() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    let mut img = StackImageBuilder::new(0x7F00_0000, 0x200);
    let ret = 0x0040_1100u64; // 16 对齐、文本命中
    img.write_u64(0x7F00_0100, ret);
    // ret-5 处写 0xE8（call rel32 前缀实证）。
    let pfx = (ret - 5 - 0x7F00_0000) as usize;
    img.write_u8(0x7F00_0000 + pfx as u64, 0xE8);
    let mem = img.finish();
    let report = walker.walk(&mem, 0);
    report
        .frames
        .iter()
        .any(|f| f.ret_addr == ret && f.verdict == FrameVerdict::Plausible && f.score >= 100)
        && report.stats.scan_accepted == 1
        && report.stats.scan_conserves()
}

/// C10：区间表守门——lo>=hi 拒登、重叠拒登、拒绝数如实入账、二分命中/脱靶精确。
fn check_c10_range_table() -> bool {
    let mut t = TextRangeTable::new();
    let ok1 = t.add(0x1000, 0x2000, 1).is_ok();
    let bad_empty = t.add(0x3000, 0x3000, 2) == Err(1);
    let bad_invert = t.add(0x5000, 0x4000, 3) == Err(1);
    let bad_overlap = t.add(0x1800, 0x2800, 4) == Err(2);
    let ok_gap = t.add(0x4000, 0x5000, 5).is_ok();
    let hit = t.lookup(0x1080).map_or(false, |r| r.tag == 1);
    let miss_lo = t.lookup(0x0FFF).is_none();
    let miss_hi = t.lookup(0x2000).is_none(); // 半开区间：hi 不命中
    let miss_gap = t.lookup(0x3000).is_none();
    ok1 && bad_empty && bad_invert && bad_overlap && ok_gap && hit && miss_lo && miss_hi && miss_gap
}

/// C11：损坏矩阵——对深度 0..5 逐帧毒化 saved_rBP，
/// 每种损坏下：误帧率 0 + 扫描账守恒 + 停机原因合法（收敛证明）。
fn check_c11_corruption_matrix() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);
    for poison_at in 0..5usize {
        let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
        let mut rbps: Vec<u64> = Vec::new();
        let mut rbp = 0u64;
        for i in 0..6u64 {
            rbp = img.push_frame(rbp, 0x0040_1000 + i * 0x40);
            rbps.push(rbp);
        }
        let bad = 0xFFFF_8000_0000_0000u64;
        img.write_u64(rbps[poison_at], bad);
        let mem = img.finish();
        let report = walker.walk(&mem, rbps[5]);
        if !report.verify_no_misframe(&sample_text_table(), MIN_ACCEPT_SCORE) {
            return false;
        }
        if !report.stats.scan_conserves() || !report.stats.frames_conserves(report.frames.len()) {
            return false;
        }
        let stop_ok = matches!(
            report.stop,
            StopReason::ScanFallback | StopReason::ScanBudget | StopReason::NothingFound
        );
        if !stop_ok {
            return false;
        }
    }
    true
}

/// C12：非单调与越窗守卫——saved_rBP 改写为倒退值（0x9604）/
/// 越窗值（0x9603）各验一例，事件码精确命中。
fn check_c12_monotonic_and_bounds() -> bool {
    let text = sample_text_table();
    let walker = StackWalker::new(text);

    // 例 1：倒退——B.saved 指向低于 A 的地址（平走/倒退）。
    let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
    let r1 = img.push_frame(0, 0x0040_1000);
    let r2 = img.push_frame(r1, 0x0040_1040);
    img.write_u64(r2, r1); // saved_rBP = 旧帧（倒退）
    let mem = img.finish();
    let rep1 = walker.walk(&mem, r2);
    let mono_ok = rep1.events.iter().any(|e| e.code == C_RBP_NONMONO);

    // 例 2：越窗——saved_rBP 指向窗外的规范地址。
    let mut img2 = StackImageBuilder::new(0x7F00_0000, 0x400);
    let s1 = img2.push_frame(0, 0x0040_1000);
    img2.write_u64(s1, 0x8000_0000); // 规范但窗外
    let mem2 = img2.finish();
    let rep2 = walker.walk(&mem2, s1);
    let bounds_ok = rep2.events.iter().any(|e| e.code == C_RBP_OUTSIDE);

    mono_ok && bounds_ok
}

/// 本模块自检（域聚合在 mod.rs——此处只放本模块纯函数级断言）。
pub fn run_unx283dv_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX283DV");
    let g = seg_guard();
    set.add(
        "unx283dv-seg-guard",
        g,
        if g { "" } else { "诊断码越出 0x96 段" },
    );
    let c1 = check_c1_healthy_chain();
    set.add(
        "unx283dv-c1-healthy-chain",
        c1,
        if c1 { "" } else { "健康链五项守卫或账守恒失守" },
    );
    let c2 = check_c2_corrupt_fallback();
    set.add(
        "unx283dv-c2-corrupt-fallback",
        c2,
        if c2 { "" } else { "链断转扫描未产出有效帧或误帧非零" },
    );
    let c3 = check_c3_cycle_detected();
    set.add(
        "unx283dv-c3-cycle",
        c3,
        if c3 { "" } else { "链环未检出或未在有界步数内终止" },
    );
    let c4 = check_c4_empty_window();
    set.add(
        "unx283dv-c4-empty",
        c4,
        if c4 { "" } else { "空窗未显性入账或谎报帧" },
    );
    let c5 = check_c5_ret_not_text();
    set.add(
        "unx283dv-c5-ret-not-text",
        c5,
        if c5 { "" } else { "不可证返回地址混入帧表" },
    );
    let c6 = check_c6_fingerprint_determinism();
    set.add(
        "unx283dv-c6-fingerprint",
        c6,
        if c6 { "" } else { "指纹不确定或损坏未变指纹" },
    );
    let c7 = check_c7_render_parseable();
    set.add(
        "unx283dv-c7-render",
        c7,
        if c7 { "" } else { "渲染行与帧/事件账不符" },
    );
    let c8 = check_c8_scan_gate();
    set.add(
        "unx283dv-c8-scan-gate",
        c8,
        if c8 { "" } else { "弱证据候选漏闸入帧表" },
    );
    let c9 = check_c9_scan_strong_accept();
    set.add(
        "unx283dv-c9-scan-strong",
        c9,
        if c9 { "" } else { "强证据候选被误杀" },
    );
    let c10 = check_c10_range_table();
    set.add(
        "unx283dv-c10-range-table",
        c10,
        if c10 { "" } else { "区间表守门失守" },
    );
    let c11 = check_c11_corruption_matrix();
    set.add(
        "unx283dv-c11-corr-matrix",
        c11,
        if c11 { "" } else { "损坏矩阵中出现误帧或账破" },
    );
    let c12 = check_c12_monotonic_and_bounds();
    set.add(
        "unx283dv-c12-mono-bounds",
        c12,
        if c12 { "" } else { "非单调/越窗守卫事件码未精确命中" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；域聚合测试在 mod.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx283dv_all_checks_green() {
        let set = run_unx283dv_checks();
        assert!(set.all_passed(), "UNX-F0283 domain checks must be green");
    }

    #[test]
    fn unx283dv_segment_guard_holds() {
        assert!(seg_guard());
    }

    #[test]
    fn unx283dv_misframe_zero_under_corruption_matrix() {
        // 损坏矩阵：对深度 0..6 逐帧毒化 saved_rBP，每种损坏下误帧率都必须为 0。
        let text = sample_text_table();
        let walker = StackWalker::new(text);
        for poison_at in 0..6usize {
            let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
            let mut rbps: Vec<u64> = Vec::new();
            let mut rbp = 0u64;
            for i in 0..6u64 {
                rbp = img.push_frame(rbp, 0x0040_1000 + i * 0x40);
                rbps.push(rbp);
            }
            let bad = 0xFFFF_8000_0000_0000u64;
            img.write_u64(rbps[poison_at], bad);
            let mem = img.finish();
            let report = walker.walk(&mem, rbps[5]);
            assert!(
                report.verify_no_misframe(&sample_text_table(), MIN_ACCEPT_SCORE),
                "poison at depth {} produced misframe",
                poison_at
            );
            assert!(report.stats.scan_conserves(), "scan ledger broke");
        }
    }

    #[test]
    fn unx283dv_render_deterministic() {
        let text = sample_text_table();
        let walker = StackWalker::new(text);
        let mut img = StackImageBuilder::new(0x7F00_0000, 0x400);
        let mut rbp = 0u64;
        for i in 0..3u64 {
            rbp = img.push_frame(rbp, 0x0040_2000 + i * 0x20);
        }
        let mem = img.finish();
        let a = walker.walk(&mem, rbp).render();
        let b = walker.walk(&mem, rbp).render();
        assert_eq!(a, b, "render must be deterministic");
    }

    #[test]
    fn unx283dv_scan_budget_bounded() {
        // 预算有界：候选数达到上限即停，扫描账守恒、不越窗。
        let text = sample_text_table();
        let walker = StackWalker::new(
            {
                let mut t = TextRangeTable::new();
                let _ = t.add(0x0040_0000, 0x0050_0000, 1);
                t
            },
        );
        let mut img = StackImageBuilder::new(0x7F00_0000, 0x1000);
        // 埋一批强证候选（全 16 对齐文本命中）：超出默认 64 预算。
        for i in 0..90u64 {
            let ret = 0x0040_1000 + i * 0x1000;
            let slot = 0x7F00_0100 + i * 8;
            if slot < 0x7F00_1000 {
                img.write_u64(slot, ret);
            }
        }
        let mem = img.finish();
        let report = walker.walk(&mem, 0);
        assert!(report.stats.scan_candidates <= MAX_SCAN_CANDIDATES);
        assert!(report.stats.scan_conserves());
        assert!(report.verify_no_misframe(&sample_text_table(), MIN_ACCEPT_SCORE));
    }
}
