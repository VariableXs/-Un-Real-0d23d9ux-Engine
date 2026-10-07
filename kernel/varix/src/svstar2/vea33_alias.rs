//! VE-F0033 · 别名与堆复用仲裁（VE-A 域 · 显存别名安全域分析 + 复用收益风险取舍 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0033`
//!
//! **判据（锚点原文）**：显存别名的仲裁（别名安全域分析：两个资源不许同时活跃），
//! 复用收益与风险的取舍表；含别名仲裁的收益实测（省了多少显存可查）。
//! 判据：**别名仲裁、安全域分析、同时活跃阻断、收益表、判据**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 同时活跃 → **阻断**（两个资源共用一块显存却**同时活跃** = 内容互相踩踏，
//!   画面是随机错乱；这必须阻断，不能降级）
//! - 收益为负 → **不别名**（别名有成本——多一层间接与转换点；
//!   省下的显存不够抵时不该别名，那是为了别名而别名）
//! - 仲裁失误 → **审计**（误判为「不同时活跃」而实际重叠 = 显存踩踏；
//!   故仲裁结论必须可复核，安全域分析过程留痕）
//!
//! **数据结构**：仲裁器（[`AliasArbiter`]）；安全域分析（[`safety_domain`]）；
//! 取舍表（[`GainTable`]）。
//!
//! **性能逐项分解**：O(别名对)——[`AliasArbiter::arbitrate`] 对每个候选对做
//! 一次区间重叠判定（O(1)，区间是半开区间故端点相接不算重叠）。
//!
//! **跨批对接点**：A05 显存预算联动——[`AliasArbiter::saved_bytes`] 给出
//! 别名实际省下的字节数，供显存预算决策；本条只算账不管分配。
//!
//! **无障碍与隐私**：仲裁记录读屏可达（[`AliasArbiter::a11y_lines`]）——报
//! 「候选对数/别名成功数/阻断数/省下字节」，中英双语逐行。面板**只报聚合计数**，
//! **不报单个资源的尺寸与内容哈希**（那是资产布局信息）。
//!
//! ## 设计要点
//!
//! - **活跃区间用半开区间 `[begin, end)`**（[`Span`]）：两个区间**端点相接**
//!   （`a.end == b.begin`）不算重叠——那正是「上一帧最后一个活跃帧」与
//!   「下一帧第一个活跃帧」，它们在时间上不重叠。若用闭区间，端点相接会被
//!   判为重叠，于是所有「每帧连续绘制」的相邻对全被阻断，别名功能整体失效。
//! - **同时活跃必须阻断，且不降级**（[`Verdict::Blocked`]）：这不是「性能不好」
//!   而是「内容互相踩踏」。故 [`AliasArbiter::arbitrate`] 对重叠对返回
//!   [`Verdict::Blocked`]，**不**提供「强制别名」的口子。
//! - **别名键必须含对齐要求**（[`AliasReq::key`]）：两个资源能共用一块显存，
//!   要求**对齐量与尺寸都不超过对方**。只看尺寸会漏对齐——把 8 字节对齐的
//!   资源别名到 256 字节对齐的块上，逻辑上能跑但每次访问都可能未对齐命中。
//! - **收益为负不别名**（[`GainTable`]）：别名省显存但要多一层间接。
//!   收益 = 省下的字节 − 别名开销；**净收益 ≤ 0 即不别名**（同 F0030 的阈纪律）。
//! - **安全域分析留痕可复核**（[`SafetyVerdict`] 带 [`Span`] 证据）：
//!   仲裁失误（把重叠判成不重叠）= 显存踩踏，最难查。故判据要求
//!   「结论必须附区间证据」，不能只给一个 bool。
//! - **夹逼对钉死界位置**（[`MIN_SAVED`] / [`MAX_ALIAS_PAIRS`]）：一律用
//!   「界前合法 / 界上 / 界后一位」三点。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0026（`vea26_desc_heap`）管描述符堆分配**，本条管「哪两个资源可以共用
//!   一块显存」。堆问「还有多少空位」，本条问「这两者能不能叠」——同一时刻
//!   可分配 ≠ 可别名。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个模块
//! 的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，与真实缺陷
//! 长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 活跃区间半开区间的空端点约定（`end <= begin` 即空区间）。
pub const EMPTY_END: u32 = 0;

/// 单帧别名候选对上限（超出 → 停止登记并登记 [`Verdict::PairBudgetExceeded`]）。
pub const MAX_ALIAS_PAIRS: usize = 128;

/// 别名的最小净收益（字节；**严格大于**才别名，≤ 0 则不别名）。
pub const MIN_SAVED: i64 = 0;

/// 每次别名的间接开销（字节当量）：别名要多一层指针与转换点。
pub const ALIAS_OVERHEAD: i64 = 16;

/// 仲裁结论种数。
pub const VERDICT_COUNT: usize = 3;

/// 收益表行数上限。
pub const MAX_GAIN_ROWS: usize = MAX_ALIAS_PAIRS;

// ---------------------------------------------------------------------------
// 二、活跃区间与安全域
// ---------------------------------------------------------------------------

/// 活跃区间（**半开** `[begin, end)`）。
///
/// 半开的理由：端点相接的两个区间在时间上不重叠（上一帧最后一个活跃帧的
/// `end` 就是下一帧第一个活跃帧的 `begin`）。用闭区间会把它们判成重叠，
/// 于是所有逐帧连续绘制的相邻对全被阻断，别名功能整体失效。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 起始帧（含）。
    pub begin: u32,
    /// 结束帧（**不含**）。
    pub end: u32,
}

impl Span {
    /// 构造半开区间。`end <= begin` 视为**空区间**（永不活跃）。
    pub const fn new(begin: u32, end: u32) -> Span {
        Span { begin, end }
    }

    /// 是否为空区间（`end <= begin`）。
    pub const fn is_empty(&self) -> bool {
        self.end <= self.begin
    }

    /// 两区间是否**重叠**（半开口径：端点相接不重叠）。
    pub const fn overlaps(&self, other: &Span) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        // 半开区间相交：a.begin < b.end 且 b.begin < a.end
        self.begin < other.end && other.begin < self.end
    }

    /// 帧数（空区间为 0）。
    pub const fn frames(&self) -> u32 {
        if self.is_empty() {
            0
        } else {
            self.end - self.begin
        }
    }

    /// 中文摘要（审计用）。
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return String::from("空区间");
        }
        format!("[{}, {}) 共 {} 帧", self.begin, self.end, self.frames())
    }
}

/// 别名请求（两个资源想共用一块显存）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AliasReq {
    /// 左侧资源下标。
    pub lhs: usize,
    /// 右侧资源下标。
    pub rhs: usize,
    /// 左侧活跃区间。
    pub lhs_span: Span,
    /// 右侧活跃区间。
    pub rhs_span: Span,
    /// 左侧字节数。
    pub lhs_bytes: u64,
    /// 右侧字节数。
    pub rhs_bytes: u64,
    /// 所需对齐字节（两者都要满足）。
    pub align: u32,
}

impl AliasReq {
    /// 构造一个别名请求。
    pub const fn new(
        lhs: usize,
        rhs: usize,
        lhs_span: Span,
        rhs_span: Span,
        lhs_bytes: u64,
        rhs_bytes: u64,
        align: u32,
    ) -> AliasReq {
        AliasReq { lhs, rhs, lhs_span, rhs_span, lhs_bytes, rhs_bytes, align }
    }

    /// 别名键：把「谁与谁、什么对齐」打包成可比较的指纹。
    ///
    /// 键里**必须含对齐量**：两个尺寸都合的块，若对齐要求不同则**不能**共用——
    /// 只看尺寸会漏对齐，把 8 字节对齐的资源别名到 256 字节对齐的块上，
    /// 逻辑上跑得通但每次访问都可能未对齐命中。
    ///
    /// **位段布局（三段互不重叠，故不同三元组必得不同键）**：
    /// `[0,32)` 较小下标 / `[32,64)` 较大下标 / `[64,96)` 对齐字节。
    ///
    /// 旧布局 `a | (b << 32) | (align << 8)` 让**对齐段压在较小下标段之上**，
    /// 于是下标一旦越过 8 位边界，对齐量就被下标吃掉——
    /// `lhs=256, rhs=257, align=1` 与 `lhs=0, rhs=257, align=1` 算出**同一个键**。
    /// 那是真实碰撞（指纹失去区分力，仲裁失误无从发现），已修：改用
    /// 128 位键让三段各占 32 位，下标取 `u32::MAX` 以内、对齐取 `u32`
    /// 全域，三元组到键是**单射**，无需掩码（掩码会把越界输入静默折叠回同一键）。
    pub fn key(&self) -> u128 {
        let (a, b) = if self.lhs <= self.rhs {
            (self.lhs, self.rhs)
        } else {
            (self.rhs, self.lhs)
        };
        // 下标窄于 u32 时高位截断无可避免，但必须**在文档里写明**而不是静默：
        // 资源数不会到 4 294 967 296，而截断发生意味着调用方已在别处越界。
        let a32 = (a as u64 & 0xFFFF_FFFF) as u128;
        let b32 = (b as u64 & 0xFFFF_FFFF) as u128;
        let al = self.align as u128;
        a32 | (b32 << 32) | (al << 64)
    }

    /// 下标窄于 u32 时高位截断无可避免，但必须**在文档里写明**而不是静默：
    /// 资源数不会到 4 294 967 296，而截断发生意味着调用方已在别处越界。
    pub const KEY_ALIGN_SHIFT: u32 = 64;
}

/// 仲裁结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 可别名：区间不重叠且净收益为正。
    Aliased,
    /// 阻断：两资源**同时活跃**（区间重叠），内容会互相踩踏。
    Blocked,
    /// 不别名：可别名但净收益为负（为了别名而别名）。
    NotWorthIt,
    /// 候选对数超预算（停止登记）。
    PairBudgetExceeded,
}

impl Verdict {
    /// 全集，顺序稳定（判据按此下标推导，不靠字面量）。
    pub const ALL: [Verdict; VERDICT_COUNT] =
        [Verdict::Aliased, Verdict::Blocked, Verdict::NotWorthIt];

    /// 判别下标（[`Verdict::PairBudgetExceeded`] 是**非常规**结论，
    /// 不进常规三类，故下标为 `VERDICT_COUNT`，越界但可辨）。
    pub const fn ordinal(self) -> usize {
        match self {
            Verdict::Aliased => 0,
            Verdict::Blocked => 1,
            Verdict::NotWorthIt => 2,
            Verdict::PairBudgetExceeded => VERDICT_COUNT,
        }
    }

    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            Verdict::Aliased => "可别名",
            Verdict::Blocked => "同时活跃已阻断",
            Verdict::NotWorthIt => "收益为负不别名",
            Verdict::PairBudgetExceeded => "候选对超预算",
        }
    }

    /// 英文标签。
    pub const fn tag(self) -> &'static str {
        match self {
            Verdict::Aliased => "aliased",
            Verdict::Blocked => "blocked",
            Verdict::NotWorthIt => "not worth it",
            Verdict::PairBudgetExceeded => "pair budget exceeded",
        }
    }
}

/// 安全域分析结论（**带区间证据**，可复核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SafetyVerdict {
    /// 结论。
    pub verdict: Verdict,
    /// 左侧区间（证据）。
    pub lhs_span: Span,
    /// 右侧区间（证据）。
    pub rhs_span: Span,
}

impl SafetyVerdict {
    /// 是否为「同时活跃」阻断。
    pub const fn is_blocked(&self) -> bool {
        matches!(self.verdict, Verdict::Blocked)
    }

    /// 审计一行：结论 + 两侧区间证据。
    ///
    /// **必须带区间**——只给一个 bool 的结论无法复核，而仲裁失误
    /// （把重叠判成不重叠）等于显存踩踏，是最难查的一类。
    pub fn audit_line(&self) -> String {
        format!(
            "{}：左 {}，右 {}",
            self.verdict.zh(),
            self.lhs_span.summary(),
            self.rhs_span.summary()
        )
    }
}

/// 安全域分析（纯函数）：两区间是否可共存。
///
/// **只看活跃区间**，不看尺寸——尺寸与对齐的考量为 [`net_gain`] 与别名键负责。
pub fn safety_domain(a: Span, b: Span) -> bool {
    // 不重叠 ⇒ 可共存
    !a.overlaps(&b)
}

// ---------------------------------------------------------------------------
// 三、收益表与仲裁器
// ---------------------------------------------------------------------------

/// 一行收益账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GainRow {
    /// 别名键（128 位三段布局，见 [`AliasReq::key`]）。
    pub key: u128,
    /// 共用块的字节数（取两侧较大者——块要装得下大的那个）。
    pub block_bytes: u64,
    /// 不别名时两者合计占用。
    pub separate_bytes: u64,
    /// 别名后占用。
    pub aliased_bytes: u64,
    /// 别名开销。
    pub overhead: i64,
    /// 净收益（`separate - aliased - overhead`，有符号，可为负）。
    pub net_gain: i64,
}

impl GainRow {
    /// 按实算一行收益账（**不接受外部传参**，全部由尺寸与对齐推出）。
    pub fn compute(req: &AliasReq) -> GainRow {
        let block = if req.lhs_bytes > req.rhs_bytes {
            req.lhs_bytes
        } else {
            req.rhs_bytes
        };
        let separate = req.lhs_bytes.saturating_add(req.rhs_bytes);
        let overhead = ALIAS_OVERHEAD;
        let net = i64::try_from(separate)
            .unwrap_or(i64::MAX)
            .saturating_sub(i64::try_from(block).unwrap_or(i64::MAX))
            .saturating_sub(overhead);
        GainRow {
            key: req.key(),
            block_bytes: block,
            separate_bytes: separate,
            aliased_bytes: block,
            overhead,
            net_gain: net,
        }
    }

    /// 是否值得别名（净收益**严格大于** [`MIN_SAVED`]）。
    pub const fn worth(&self) -> bool {
        self.net_gain > MIN_SAVED
    }

    /// 中文一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "分开 {} 字节，别名后 {} 字节，开销 {}，净收益 {}",
            self.separate_bytes, self.aliased_bytes, self.overhead, self.net_gain
        )
    }
}

/// 收益表（逐对一行，可查「省了多少显存」）。
#[derive(Clone, Debug, Default)]
pub struct GainTable {
    /// 逐行收益账。
    pub rows: Vec<GainRow>,
}

impl GainTable {
    /// 新建空表。
    pub fn new() -> GainTable {
        GainTable { rows: Vec::new() }
    }

    /// 追加一行。
    pub fn push(&mut self, row: GainRow) {
        self.rows.push(row);
    }

    /// 总净收益（有符号，**绝对值口径逐行累加**，不用净值差）。
    pub fn total_net(&self) -> i64 {
        let mut acc = 0i64;
        for r in self.rows.iter() {
            acc = acc.saturating_add(r.net_gain);
        }
        acc
    }

    /// 实际省下的字节数（**只累加正收益行**：负收益行不省显存）。
    ///
    /// 「省了多少显存」不能对负行也减——那会把「别名反而更占」算成节省。
    pub fn saved_bytes(&self) -> u64 {
        let mut acc = 0u64;
        for r in self.rows.iter() {
            if r.net_gain > 0 {
                let by = (r.separate_bytes - r.aliased_bytes) as u64;
                acc = acc.saturating_add(by.saturating_sub(r.overhead as u64));
            }
        }
        acc
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 净收益（纯函数，便于判据独立重算）。
pub fn net_gain(req: &AliasReq) -> i64 {
    GainRow::compute(req).net_gain
}

/// 别名仲裁器。
#[derive(Clone, Debug, Default)]
pub struct AliasArbiter {
    /// 已登记的候选对数。
    pairs: usize,
    /// 各结论计数（按 [`Verdict::ALL`] 下标，绝对值口径）。
    counts: [u32; VERDICT_COUNT],
    /// 收益表。
    pub gains: GainTable,
    /// 审计留痕（每条结论一行，带区间证据）。
    pub audit: Vec<String>,
}

impl AliasArbiter {
    /// 新建仲裁器。
    pub fn new() -> AliasArbiter {
        AliasArbiter {
            pairs: 0,
            counts: [0u32; VERDICT_COUNT],
            gains: GainTable::new(),
            audit: Vec::new(),
        }
    }

    /// 仲裁一对。
    ///
    /// 次序刻意为 **先安全后收益**：先判同时活跃（阻断），再判收益。
    /// 反过来写会先因收益为正而放行，再在安全检查处阻断——顺序不同但结论
    /// 可能相同，可一旦 `safety_domain` 出错，**先收益**的实现会把钱算进去。
    pub fn arbitrate(&mut self, req: &AliasReq) -> SafetyVerdict {
        // 候选对预算：超出即停并登记（不静默丢弃）。
        if self.pairs >= MAX_ALIAS_PAIRS {
            let v = Verdict::PairBudgetExceeded;
            self.note(v);
            let sv =
                SafetyVerdict { verdict: v, lhs_span: req.lhs_span, rhs_span: req.rhs_span };
            // **超限也必须留痕**。此处原先直接 `return` 掉，`audit.push` 在
            // 后面——注释写着「不静默丢弃」，实际却把超限这一类**静默丢掉**
            // 了留痕：预算耗尽正是最容易查不出「漏了几对」的地方，而锚点把
            // 「仲裁失误→审计」列为错误路径。改走 `note_verdict` 统一出口。
            self.push_verdict(&sv);
            return sv;
        }
        self.pairs += 1;

        // 1) 安全域：同时活跃 ⇒ 阻断（不降级）。
        let safe = safety_domain(req.lhs_span, req.rhs_span);
        let verdict = if !safe {
            Verdict::Blocked
        } else {
            // 2) 收益：净收益为正才别名。
            let row = GainRow::compute(req);
            if row.worth() {
                self.gains.push(row);
                Verdict::Aliased
            } else {
                Verdict::NotWorthIt
            }
        };
        self.note(verdict);
        let sv = SafetyVerdict { verdict, lhs_span: req.lhs_span, rhs_span: req.rhs_span };
        self.push_verdict(&sv);
        sv
    }

    /// 记一条结论的审计留痕（**所有**结论都走这里，含超限）。
    ///
    /// 留痕与计数刻意分开两个出口：`note` 只动计数、`push_verdict` 只动审计。
    /// 原先把两者写在同一个函数体里，导致「超限」分支提前 `return` 时
    /// 绕过了留痕——计数记了、审计没记，恰好在最难查的那类上留了空。
    fn push_verdict(&mut self, sv: &SafetyVerdict) {
        self.audit.push(sv.audit_line());
    }

    /// 记一条结论（绝对值口径；非常规结论不入常规三类计数）。
    fn note(&mut self, v: Verdict) {
        let i = v.ordinal();
        if i < VERDICT_COUNT {
            self.counts[i] = self.counts[i].saturating_add(1);
        }
    }

    /// 各结论计数（按 [`Verdict::ALL`] 下标）。
    pub fn verdict_counts(&self) -> [u32; VERDICT_COUNT] {
        self.counts
    }

    /// 已登记候选对数。
    pub fn pairs(&self) -> usize {
        self.pairs
    }

    /// 实际省下的字节数（转调收益表）。
    pub fn saved_bytes(&self) -> u64 {
        self.gains.saved_bytes()
    }

    /// 审计留痕（只读）。
    pub fn audit_lines(&self) -> &[String] {
        &self.audit
    }

    /// 读屏面板：中英双语逐行，**只报聚合计数**，不报单个资源尺寸与哈希。
    pub fn a11y_lines(&self) -> [String; 6] {
        [
            format!("候选对数 / pairs: {}", self.pairs),
            format!("别名成功 / aliased: {}", self.counts[Verdict::Aliased.ordinal()]),
            format!("同时活跃阻断 / blocked: {}", self.counts[Verdict::Blocked.ordinal()]),
            format!(
                "收益为负未别名 / not worth it: {}",
                self.counts[Verdict::NotWorthIt.ordinal()]
            ),
            format!("省下字节 / saved bytes: {}", self.saved_bytes()),
            format!("收益表行数 / gain rows: {}", self.gains.len()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// VE-F0033 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_vea33_checks() -> CheckSet {
    let mut s = CheckSet::new("vea33_alias");

    // --- 判据 1：半开区间语义——端点相接**不**算重叠 ------------------------
    s.add(
        "A33-安全域-半开区间端点相接不重叠",
        {
            let a = Span::new(0, 10);
            let b = Span::new(10, 20); // 端点相接
            let c = Span::new(9, 20); // 真正重叠
            let d = Span::new(0, 0); // 空区间（begin == end）
            let e = Span::new(7, 3); // 空区间（end < begin，畸形）
            !a.overlaps(&b)
                && a.overlaps(&c)
                && !a.overlaps(&d)
                // 端点相接的逐帧相邻对可共存（用闭区间会把它们全判重叠，别名整体失效）
                && safety_domain(a, b)
                && !safety_domain(a, c)
                && a.frames() == 10
                && d.is_empty()
                && d.frames() == 0
                // **空区间 × 空区间**：两种空区间（begin==end 与 end<begin）之间
                // 也必须不重叠。原判据只测了「非空 × 空」一路，于是把守卫从
                // `||` 改成 `&&`（空×空判重叠）时**判据全绿**——这是实测
                // 抓到的真实弱门禁，不是假想。语义上：空区间 = 永不活跃，
                // 两个永不活跃的东西当然不同时活跃，故可共存。
                && !d.overlaps(&d)
                && !e.overlaps(&d)
                && !d.overlaps(&e)
                && !e.overlaps(&e)
                && safety_domain(d, e)
                && e.is_empty()
                && e.frames() == 0
                // 空区间与真重叠区间不重叠（空的那侧不活跃）
                && !d.overlaps(&c)
                && !c.overlaps(&d)
        },
        "半开区间 [begin,end)：端点相接不重叠、真重叠判重叠；空区间（两种形态）对任何区间都不重叠",
    );

    // --- 判据 2：同时活跃必须阻断且**不留强制别名口子** ----------------------
    {
        let mut a = AliasArbiter::new();
        let req = AliasReq::new(0, 1, Span::new(0, 10), Span::new(5, 15), 4096, 4096, 256);
        let v = a.arbitrate(&req);
        let c = a.verdict_counts();
        s.add(
            "A33-仲裁-同时活跃阻断且计入阻断计数",
            v.is_blocked()
                && v.verdict == Verdict::Blocked
                && c[Verdict::Blocked.ordinal()] == 1
                && c[Verdict::Aliased.ordinal()] == 0
                // 阻断的 pair **不进收益表**（不能既踩踏又算钱）
                && a.gains.is_empty(),
            "区间重叠的一对被判阻断，不计入别名成功且不进收益表",
        );
    }

    // --- 判据 3：阻断结论**带区间证据**可复核（仲裁失误审计）----------------
    {
        let mut a = AliasArbiter::new();
        let req = AliasReq::new(0, 1, Span::new(3, 9), Span::new(6, 12), 1024, 1024, 64);
        let v = a.arbitrate(&req);
        let line = v.audit_line();
        s.add(
            "A33-审计-阻断结论附区间证据可复核",
            v.is_blocked()
                && v.lhs_span == Span::new(3, 9)
                && v.rhs_span == Span::new(6, 12)
                && line.contains("[3, 9)")
                && line.contains("[6, 12)")
                && a.audit_lines().len() == 1,
            "仲裁结论携带两侧区间摘要，审计留痕可复核（只给bool 的结论无法复核）",
        );
    }

    // --- 判据 4：净收益为正才别名、别名键含对齐量 ----------------------------
    {
        // 4096 + 4096 分开；别名后块取大者 4096；开销 16 ⇒ 净 4080 > 0 ⇒ 别名
        let ok_req = AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), 4096, 4096, 256);
        // 8 + 8：分开 16，块 8，开销 16 ⇒ 净 -8 ≤ 0 ⇒ 不别名
        let bad_req = AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), 8, 8, 4);
        let mut a = AliasArbiter::new();
        let v1 = a.arbitrate(&ok_req);
        let v2 = a.arbitrate(&bad_req);
        let c = a.verdict_counts();
        // 键含对齐：同两资源不同对齐 ⇒ 键不同
        let k256 = ok_req.key();
        let k4 = AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), 4096, 4096, 4).key();
        s.add(
            "A33-收益-净收益为正才别名且别名键含对齐",
            v1.verdict == Verdict::Aliased
                && v2.verdict == Verdict::NotWorthIt
                && c[Verdict::Aliased.ordinal()] == 1
                && c[Verdict::NotWorthIt.ordinal()] == 1
                && net_gain(&ok_req) == 4080
                && net_gain(&bad_req) == -8
                // 同两资源、不同对齐 ⇒ 键必不同（只看尺寸会漏对齐）
                && k256 != k4
                // 键与顺序无关（(0,1) 与 (1,0) 同键）
                && ok_req.key()
                    == AliasReq::new(1, 0, ok_req.lhs_span, ok_req.rhs_span, 4096, 4096, 256)
                        .key(),
            "净收益正才别名；别名键含对齐量故不同对齐不同键，且键与左右顺序无关",
        );
    }

    // --- 判据 5：收益为负**不别名**（为了别名而别名是反模式）----------------
    {
        let mut a = AliasArbiter::new();
        // 一串小资源：每个净收益都为负
        let mut not_worth = 0u32;
        let mut i = 0u32;
        while i < 5 {
            let req = AliasReq::new(
                i as usize * 2,
                i as usize * 2 + 1,
                Span::new(0, 1),
                Span::new(1, 2),
                4,
                4,
                4,
            );
            if a.arbitrate(&req).verdict == Verdict::NotWorthIt {
                not_worth += 1;
            }
            i += 1;
        }
        s.add(
            "A33-收益-收益为负一律不别名",
            not_worth == 5
                && a.gains.is_empty()
                && a.verdict_counts()[Verdict::Aliased.ordinal()] == 0,
            "五个净收益为负的候选对全部不别名，收益表为空（不为别名而别名）",
        );
    }

    // --- 判据 6：夹逼对钉死收益阈位置（净 0 不别名 / 净 1 才别名）----------
    {
        // 净 = (lhs + rhs) - max(lhs, rhs) - 16 = min(lhs,rhs) - 16
        // 取 lhs = rhs = m ⇒ 净 = m - 16。净恰 0 ⇒ m = 16；净恰 1 ⇒ m = 17。
        let zero_req = AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), 16, 16, 4);
        let one_req = AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), 17, 17, 4);
        let rz = GainRow::compute(&zero_req);
        let ro = GainRow::compute(&one_req);
        s.add(
            "A33-收益-夹逼对钉死阈位置（0不别名1才别名）",
            rz.net_gain == 0
                && !rz.worth()
                && ro.net_gain == 1
                && ro.worth(),
            "净收益恰为 0 的对不合、恰为 1 的对合——阈值写成 >= 会被此判据抓住",
        );
    }

    // --- 判据 7：共用块取两侧较大者（块要装得下大的）------------------------
    {
        let req = AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), 1024, 8192, 256);
        let row = GainRow::compute(&req);
        s.add(
            "A33-收益-共用块取两侧较大者",
            row.block_bytes == 8192
                && row.aliased_bytes == 8192
                && row.separate_bytes == 9216
                && row.net_gain == 1024 - ALIAS_OVERHEAD,
            "块取大者 8192（装得下大的），分开 9216，净收益为差值减开销",
        );
    }

    // --- 判据 8：省下字节只累加**正**收益行（负行不省显存）------------------
    {
        let mut t = GainTable::new();
        // 正收益行：分开 8192、别名后 4096 ⇒ 省 4096 - 16
        t.push(GainRow::compute(&AliasReq::new(
            0,
            1,
            Span::new(0, 1),
            Span::new(1, 2),
            4096,
            4096,
            256,
        )));
        // 负收益行：**非对称尺寸且收益严格为负**（8 与 24：分开 32、别名后块取大者 24，
        // `separate - aliased = 8` 小于 overhead 16 ⇒ 净收益 8 - 16 = **-8**）。
        //
        // 这行是**口径鉴别子**：净收益 ≤ 0 的行绝不该贡献「省下的字节」。
        // **这里必须取严格负值而不是 0**：若第二行净收益恰为 0，则
        // 「只累加正收益行」与「累加全部行」两种口径在 `total_net()` 上
        // 结果**完全相同**，`total_net() == 4080 + row2.net_gain` 这条断言
        // 就是**空断言**——判据看着在核对口径，实际任何只累加正行的实现都过。
        // 取 -8 后，`total_net()` 必须含这 -8，与 `saved_bytes()` 的 +4080
        // 构成**两个口径必须不同**的可区分点（十诫：断言两侧在测试点上同值
        // 等于没断言）。
        t.push(GainRow::compute(&AliasReq::new(
            2,
            3,
            Span::new(0, 1),
            Span::new(1, 2),
            8,
            24,
            4,
        )));
        // 独立重算：只累加正收益行的 (separate - aliased - overhead)
        let want: u64 = t
            .rows
            .iter()
            .filter(|r| r.net_gain > 0)
            .map(|r| (r.separate_bytes - r.aliased_bytes) - r.overhead as u64)
            .sum();
        // 判据侧**独立重算**净值合计（不看 `total_net()` 的实现）：
        // 逐行把有符号净收益累加，负行必须如实拉低合计。
        let want_total: i64 = t.rows.iter().fold(0i64, |acc, r| acc + r.net_gain);
        let row2 = t.rows[1];
        s.add(
            "A33-收益-省下字节只计正收益行",
            t.len() == 2
                && t.saved_bytes() == want
                && t.saved_bytes() == 4096 - 16
                // 前置：第二行**确为严格负收益行**且尺寸形态与预期一致。
                // 若这一行取 0，则下面两条口径断言在测试点上同值 ⇒ 空断言。
                && row2.net_gain == -8
                && row2.separate_bytes == 32
                && row2.aliased_bytes == 24
                && row2.overhead == ALIAS_OVERHEAD
                // 口径一：省下的字节**只**含正收益行 ⇒ 4080
                && t.saved_bytes() == 4080
                // 口径二：净值合计**如实含**负行 ⇒ 4080 - 8 = 4072。
                // 与口径一**必须不同**（差值恰为该负行的净收益）——这条
                // 「两者不等」本身就是对「两口径未被人合并成同一个」的钉死。
                && t.total_net() == want_total
                && t.total_net() == 4072
                && t.total_net() != t.saved_bytes() as i64
                && t.total_net() - t.saved_bytes() as i64 == row2.net_gain,
            "省下字节只累加正收益行(4080)；净值合计如实含严格负收益行(4072)；两口径差恰为该负行净收益(-8)",
        );
    }

    // --- 判据 9：候选对预算超限登记且不静默丢弃 -----------------------------
    {
        let mut a = AliasArbiter::new();
        let req = AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), 4096, 4096, 256);
        let mut i = 0;
        let mut last = Verdict::Aliased;
        while i < MAX_ALIAS_PAIRS + 3 {
            last = a.arbitrate(&req).verdict;
            i += 1;
        }
        let c = a.verdict_counts();
        // 非常规结论**不混入**常规三类计数：判据侧独立重算「常规三类之和」
        // 必须恰等于已登记的对数——若 PairBudgetExceeded 被并入某一类，
        // 这个和会大于 MAX_ALIAS_PAIRS。用**行为**钉住，不用「拿常量比自己」
        // （`ordinal() == VERDICT_COUNT` 是自证式断言：读的就是定义处）。
        let regular_sum: u32 = c.iter().fold(0u32, |acc, v| acc.saturating_add(*v));
        s.add(
            "A33-边界-候选对预算超限登记非常规结论",
            last == Verdict::PairBudgetExceeded
                && a.pairs() == MAX_ALIAS_PAIRS
                && c[Verdict::Aliased.ordinal()] == MAX_ALIAS_PAIRS as u32
                // 常规三类之和 == 已登记对数 ⇒ 超限的 3 次**一条都没进**常规计数
                && regular_sum == MAX_ALIAS_PAIRS as u32
                // 且每次超限都留痕（audit 行数 == 登记对数 + 超限次数），
                // 证明是「登记」而不是「静默丢弃」
                && a.audit_lines().len() == MAX_ALIAS_PAIRS + 3
                // 超限结论的下标越界但可辨（它不在 Verdict::ALL 里）
                && !Verdict::ALL.contains(&Verdict::PairBudgetExceeded),
            "候选对超预算后登记非常规结论并逐次留痕，该结论不进常规三类计数",
        );
    }

    // --- 判据 9b：别名键是**单射**（下标越过 8 位边界也不与对齐量碰撞）-----
    //
    // 这条判据针对一个**已修的真实缺陷**：旧键布局 `a | (b<<32) | (align<<8)`
    // 让对齐段压在较小下标段之上，于是 `(256,257,align=1)` 与 `(0,257,align=1)`
    // 算出同一个键——指纹失去区分力，仲裁失误无从发现。
    {
        // 碰撞对：lhs 差 256（2^8，恰是旧布局里 align<<8 的最低位所在）
        let k_big = AliasReq::new(256, 257, Span::new(0, 1), Span::new(1, 2), 4096, 4096, 1).key();
        let k_small = AliasReq::new(0, 257, Span::new(0, 1), Span::new(1, 2), 4096, 4096, 1).key();
        // 对齐段边界：align 差 1 必须改键。注意对齐在**第 3 段 [64,96)**，
        // 故 align+1 不是「键 + 1」而是「键 + 2^64」——我第一版写成
        // `k_a1 == k_a2 - 1` 是**判据自己写错了**（拿第 0 段的步长去比第 2 段），
        // 实测 k_a1 = 0x…0001_00000907_00000007、k_a2 = 0x…0002_…，
        // 差值是 2^64 而非 1。判据参考值必须按**段位**独立算出，不能凭感觉。
        let k_a1 = AliasReq::new(7, 9, Span::new(0, 1), Span::new(1, 2), 4096, 4096, 1).key();
        let k_a2 = AliasReq::new(7, 9, Span::new(0, 1), Span::new(1, 2), 4096, 4096, 2).key();
        // 大对齐：align 取 u32::MAX 也不该溢出到下标段
        let k_max = AliasReq::new(7, 9, Span::new(0, 1), Span::new(1, 2), 4096, 4096, u32::MAX).key();
        s.add(
            "A33-别名键-三段位域不重叠故三元组到键为单射",
            // 曾经的碰撞对必须异键
            k_big != k_small
                // 曾经的对齐段吞下标位：align=1 时 lhs=256 与 lhs=0 曾同键，
                // 现在不仅异键，且小下标那个的键不含 align 的低位污染
                && (k_big & 0xFFFF_FFFF) == 256
                && (k_small & 0xFFFF_FFFF) == 0
                // 对齐量进第 3 段：差 1 ⇒ 键差恰为 2^64（按段位独立算出）
                && k_a1 != k_a2
                && k_a2 - k_a1 == (1u128 << AliasReq::KEY_ALIGN_SHIFT)
                && (k_a1 >> AliasReq::KEY_ALIGN_SHIFT) == 1
                && (k_a2 >> AliasReq::KEY_ALIGN_SHIFT) == 2
                // u32::MAX 对齐不溢出、不污染下标段
                && (k_max >> AliasReq::KEY_ALIGN_SHIFT) == u128::from(u32::MAX)
                && (k_max & 0xFFFF_FFFF) == 7,
            "别名键按下标(32)/下标(32)/对齐(32) 三段布局；越过 8 位边界的下标不再与对齐量碰撞",
        );
    }

    // --- 判据 10：安全先于收益（重叠对即便收益为正也不进收益表）------------
    {
        let mut a = AliasArbiter::new();
        // 重叠但尺寸差巨大 ⇒ 收益很正，安全仍须阻断
        let req = AliasReq::new(0, 1, Span::new(0, 100), Span::new(50, 150), 4096, 65536, 256);
        let v = a.arbitrate(&req);
        s.add(
            "A33-仲裁-安全先于收益重叠即阻断",
            v.verdict == Verdict::Blocked
                && a.gains.is_empty()
                && a.saved_bytes() == 0,
            "区间重叠的一对即便净收益很高也阻断，且不进收益表不省显存",
        );
    }

    // --- 判据 11：结论四类标签与下标互异 ------------------------------------
    {
        let mut seen = [false; VERDICT_COUNT];
        let mut distinct = true;
        for v in Verdict::ALL.iter() {
            if v.ordinal() >= VERDICT_COUNT || seen[v.ordinal()] {
                distinct = false;
            }
            seen[v.ordinal()] = true;
        }
        let mut zhs = Vec::new();
        for v in Verdict::ALL.iter() {
            zhs.push(v.zh());
        }
        s.add(
            "A33-结论-三类下标互异且标签互不相同",
            distinct
                && seen[0]
                && seen[1]
                && seen[2]
                && Verdict::ALL.len() == VERDICT_COUNT
                && zhs[0] != zhs[1]
                && zhs[1] != zhs[2]
                && Verdict::Aliased.tag() != Verdict::Blocked.tag(),
            "常规三类结论下标互异、中文标签互不相同（合并则判据无法区分）",
        );
    }

    // --- 判据 12：仲裁器端到端——别名/阻断/不别名三路并存且计数独立 ---------
    {
        let mut a = AliasArbiter::new();
        // 1) 可别名且有收益
        a.arbitrate(&AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), 4096, 4096, 256));
        // 2) 同时活跃 → 阻断
        a.arbitrate(&AliasReq::new(2, 3, Span::new(0, 10), Span::new(5, 15), 4096, 4096, 256));
        // 3) 收益为负 → 不别名
        a.arbitrate(&AliasReq::new(4, 5, Span::new(0, 1), Span::new(1, 2), 8, 8, 4));
        let c = a.verdict_counts();
        s.add(
            "A33-仲裁-三路并存且计数彼此独立",
            a.pairs() == 3
                && c[Verdict::Aliased.ordinal()] == 1
                && c[Verdict::Blocked.ordinal()] == 1
                && c[Verdict::NotWorthIt.ordinal()] == 1
                // 独立重算：被别名的一对才进收益表
                && a.gains.len() == 1
                && a.saved_bytes() == 4096 - 16
                && a.audit_lines().len() == 3,
            "可别名/同时活跃阻断/收益为负三路并存各计一次，只有被别名的对进收益表",
        );
    }

    // --- 判据 13：读屏面板双语齐备且不泄漏资源尺寸 --------------------------
    {
        let mut a = AliasArbiter::new();
        // 尺寸取**互质的大数**，使「任何单对尺寸的十进制形态」都独一无二：
        // 原先用 65536/65536，判据只查 `contains("65536")` 一个字面量——
        // 实测把 `separate_bytes`（= 131072）塞进面板时**判据全绿**，
        // 因为 "131072" 里不含 "65536" 子串。**只对一个具体数字做泄漏检查
        // 等于没检查**。
        const L: u64 = 65537; // 素数，独一无二
        const R: u64 = 131071;
        a.arbitrate(&AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), L, R, 256));
        let lines = a.a11y_lines();
        let joined = lines.join("|");
        // 判据侧**独立重算**该对的**私有**数字形态。
        //
        // **只禁私有信息，不禁聚合计数**：面板第 4 行「省下字节」正是锚点
        // 要求的「省了多少显存可查」，它是**聚合结果**不是单资源属性，
        // 必须报出来。我第一版把净收益绝对值也列进禁形态，结果把**基线判红**
        // ——那是判据写错（把该报的当成泄漏），不是实现泄漏。同理别名键的
        // lo/mid 段在这对里是 0 与 1，平凡小整数在任何面板里都会出现
        // （"pairs: 1"），禁它们无意义且必假红。只有**对齐段**（hi=256）
        // 是私有属性，纳入检查。
        let block = if L > R { L } else { R };
        let separate = L + R;
        let net = separate as i64 - block as i64 - ALIAS_OVERHEAD;
        let key = AliasReq::new(0, 1, Span::new(0, 1), Span::new(1, 2), L, R, 256).key();
        let forbidden: [String; 5] = [
            format!("{}", L),         // 左资源尺寸（私有）
            format!("{}", R),         // 右资源尺寸（私有）
            format!("{}", block),     // 共用块尺寸（私有）
            format!("{}", separate), // 分开时合计（私有）
            format!("{}", key >> 64), // 别名键的对齐段（私有）
        ];
        let mut leaks = 0usize;
        for f in forbidden.iter() {
            if joined.contains(f.as_str()) {
                leaks += 1;
            }
        }
        s.add(
            "A33-读屏-六行双语且不泄漏资源尺寸",
            lines.len() == 6
                && lines.iter().all(|l| !l.is_empty())
                && lines.iter().any(|l| l.contains("pairs"))
                && lines.iter().any(|l| l.contains("blocked"))
                && lines.iter().any(|l| l.contains("saved bytes"))
                // 前置：这一对**确实被别名了**（否则「面板无它」是空洞的）
                && a.gains.len() == 1
                // 零泄漏：左右尺寸、块尺寸、分开合计、别名键对齐段，
                // 五种私有形态逐一验证都不在面板里
                && leaks == 0
                && !joined.contains("哈希")
                // 反向对照：聚合计数**必须**在面板里（锚点要求「省了多少
                // 显存可查」）——漏报同样是缺陷，与泄漏同条判据里钉死
                && joined.contains(&format!("{}", a.saved_bytes()))
                && joined.contains(&format!("{}", net.unsigned_abs())),
            "面板六行中英双语：五种私有形态（两侧尺寸/块/合计/键对齐段）均不泄漏，聚合计数必须可查",
        );
    }

    // --- 判据 14：区间帧数与空区间口径自洽 ----------------------------------
    {
        let cases = [(0u32, 1u32, 1u32), (0, 10, 10), (5, 5, 0), (9, 3, 0), (0, u32::MAX, u32::MAX)];
        let mut ok = true;
        let mut i = 0;
        while i < cases.len() {
            let sp = Span::new(cases[i].0, cases[i].1);
            ok &= sp.frames() == cases[i].2;
            ok &= sp.is_empty() == (cases[i].2 == 0);
            i += 1;
        }
        s.add(
            "A33-区间-帧数与空区间口径自洽",
            ok && Span::new(9, 3).is_empty() && !Span::new(0, 1).is_empty(),
            "帧数与空区间判定逐例自洽（含 end<begin 的畸形区间按空处理）",
        );
    }

    s
}