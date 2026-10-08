//! VE-F3606 · 创作预览运行时（VE-S 域 · 所见即所得的创作预览 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3606`
//!
//! **职责定位（锚点原文）**：所见即所得的创作预览——创作内容隔离渲染（沙箱
//! 复述）；预览保真（预览效果 = 发布效果，保真红线：**预览与实况分歧 = 创作者
//! 被骗**，保真断言 = 预览→发布对拍抽样）；预览性能（编辑操作→预览刷新延迟
//! ≤100ms，实时感红线：**预览卡顿 = 创作体验死穴**）；大资产预览分块（渐进
//! 复用）。
//!
//! **判据（锚点原文）**：保真对拍、100ms 实时、沙箱隔离、渐进复用、
//! 所见即所得、判据。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//!
//! - 保真分歧 → **P1**（红线实测——对拍断言）
//! - 预览延迟 >100ms → **优化立案**（红线实测）
//! - 污染 → **隔离**（复用 F3605）
//! - 渐进破碎 → **自洽**（复用）
//!
//! **数据结构**：沙箱预览环境 [`SandboxPreview`]；保真对拍器
//! [`FidelityParity`]；实时断言 [`RealtimeAssert`]；渐进复用 [`ProgressiveReuse`]。
//!
//! **性能逐项分解（锚点原文）**：刷新 O(变更量)；对拍 O(抽样)；沙箱 O(1)；
//! 渐进 O(复用)。
//!
//! **跨批对接点**：**F3605 隔离单源**（[`super::ves05_bridge`] 的
//! [`DelegateLedger`] / [`PreviewChannel`] / [`parity_sample`] 均为**单源**，
//! 本条只调用不另立）；F3622/F3623 编辑对端；F3088 渐进复用。
//!
//! ## 设计要点
//!
//! - **隔离靠结构不靠自觉**（[`SandboxPreview`]）：预览帧一律走 F3605 的
//!   [`PreviewChannel`]，该通道类型上**没有**写正式缓存的方法，故从预览侧
//!   写脏正式缓存在**编译期**就不可能，而不是靠运行期检查。
//! - **保真红线是可测的，不是声明的**（[`FidelityParity`]）：预览侧与发布侧
//!   各自算出指纹，抽样对拍；`drifted > 0` 即记 **P1**。注意**空抽样不算
//!   通过**（沿用 F3605 [`ParityReport::all_match`] 的口径）——「没查」被读成
//!   「都对」是最危险的误读。
//! - **100ms 是硬红线不是软目标**（[`RealtimeAssert`]）：超限**立案**而非
//!   静默放行；判据侧用**字面量 100/101** 卡两侧，不引用常量。
//! - **渐进复用按块计数而非按字节**（[`ProgressiveReuse`]）：分块数与复用
//!   次数分别记账，破碎（块数超上限）时**保持自洽**——账目仍对得上，只是
//!   复用率下降，不静默截断。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F3605（`ves05_bridge`）管票据与通道本身**（报名/盖章/消费/隔离核验/
//!   对拍原语），本条是它的**消费方**：把「预览运行时」这件事用起来，并对
//!   **预览侧**的保真与实时另立红线。F3605 的 `parity_sample` 钉的是「票据
//!   指纹 vs 引擎指纹」，本条 [`FidelityParity`] 钉的是「**预览**指纹 vs
//!   **发布**指纹」——两个方向不同，判据也不同，不可互相替代。
//! - **F3088渐进复用**管复用机制本身，本条只**按块记账与破碎自洽**。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::ves05_bridge::{
    parity_sample, DelegateLedger, FormalChannel, ParityReport, PreviewChannel, PreviewFrame,
    Product,
};

/// 预览刷新延迟红线（毫秒，锚点明文 100ms）。
pub const REALTIME_BUDGET_MS: u32 = 100;

/// 大资产分块上限（渐进预览单次可请求的块数）。
pub const MAX_CHUNKS: usize = 64;

/// 一次刷新允许变更的最大对象数（刷新是 O(变更量) 而非 O(全量)）。
pub const MAX_CHANGED: usize = 4096;

// ---------------------------------------------------------------------------
// 一、诊断码（自建，F3605 的封闭枚举无权加变体）
// ---------------------------------------------------------------------------

/// F3606 诊断码。**独占段0x36xx**（F3605 用 `E_*` 字符串码，此处用数值码，
/// 两套编码并存但互不映射——映射多对少不可反推，故不合并）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreviewCode(pub u16);

impl PreviewCode {
    /// 保真分歧（**红线**：预览与实况不一致 = 创作者被骗）。
    pub const PARITY_DRIFT: PreviewCode = PreviewCode(0x3601);
    /// 预览刷新超预算（**红线**：预览卡顿 = 创作体验死穴）。
    pub const REALTIME_OVER: PreviewCode = PreviewCode(0x3602);
    /// 空抽样被当作通过（退化口径，必须显性拒绝）。
    pub const PARITY_EMPTY: PreviewCode = PreviewCode(0x3603);
    /// 正式缓存被预览侧污染（隔离红线，复用 F3605 核验）。
    pub const CONTAMINATED: PreviewCode = PreviewCode(0x3604);
    /// 渐进块数越界。
    pub const CHUNK_RANGE: PreviewCode = PreviewCode(0x3605);
    /// 变更对象数越界。
    pub const CHANGE_RANGE: PreviewCode = PreviewCode(0x3606);

    /// 全集（判据按此集合推导，不硬编码下标）。
    pub const ALL: [PreviewCode; 6] = [
        PreviewCode::PARITY_DRIFT,
        PreviewCode::REALTIME_OVER,
        PreviewCode::PARITY_EMPTY,
        PreviewCode::CONTAMINATED,
        PreviewCode::CHUNK_RANGE,
        PreviewCode::CHANGE_RANGE,
    ];

    /// 码位（线上/报告用）。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话标签（**未知码必须有兜底**——不得 panic，也不得静默）。
    pub const fn label(self) -> &'static str {
        match self {
            PreviewCode::PARITY_DRIFT => "预览与发布保真分歧：创作者会看到与发布不同的画面",
            PreviewCode::REALTIME_OVER => "预览刷新超出 100ms 实时预算：编辑手感已死",
            PreviewCode::PARITY_EMPTY => "对拍抽样为空：什么都没查不得读成全都对",
            PreviewCode::CONTAMINATED => "预览侧污染正式缓存：沙箱隔离被绕过",
            PreviewCode::CHUNK_RANGE => "渐进预览块数越界",
            PreviewCode::CHANGE_RANGE => "单次刷新变更对象数越界",
            // 兜底：码位越界时给人话而不是崩掉——判据会拿一个越界码来
            // 验证「未知码须可渲染」，这里panic 就正好被它抓到。
            _ => "未知预览诊断码（未登记）",
        }
    }

    /// 是否 P1（红线级：创作者被骗或手感已死）。
    pub const fn is_p1(self) -> bool {
        matches!(self, PreviewCode::PARITY_DRIFT | PreviewCode::REALTIME_OVER)
    }
}

/// 严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 红线（P1）。
    P1,
    /// 一般。
    Major,
}

/// 一条诊断。
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewIssue {
    /// 码。
    pub code: PreviewCode,
    /// 严重度。
    pub severity: Severity,
    /// 定位（帧号 / 块号 / 对象名，单源可读）。
    pub subject: String,
    /// 人话处置建议。
    pub hint: String,
}

impl PreviewIssue {
    /// 构造。
    pub fn new(
        code: PreviewCode,
        severity: Severity,
        subject: impl Into<String>,
        hint: impl Into<String>,
    ) -> PreviewIssue {
        PreviewIssue { code, severity, subject: subject.into(), hint: hint.into() }
    }

    /// 渲染成一行（码 + 严重度 + 定位 + 处置）。
    pub fn render(&self) -> String {
        format!(
            "[{:#06x}] {} · 定位：{} · 处置：{}",
            self.code.code(),
            self.code.label(),
            self.subject,
            self.hint
        )
    }
}

/// 诊断袋（可查询的一等公民）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreviewBag {
    issues: Vec<PreviewIssue>,
}

impl PreviewBag {
    /// 空袋。
    pub fn new() -> PreviewBag {
        PreviewBag { issues: Vec::new() }
    }

    /// 记一条（严重度由码决定，**不允许调用方自选**——否则红线可被降级成
    /// 一般问题，这是不可接受的）。
    pub fn push(&mut self, code: PreviewCode, subject: impl Into<String>, hint: impl Into<String>) {
        let sev = if code.is_p1() { Severity::P1 } else { Severity::Major };
        self.issues.push(PreviewIssue::new(code, sev, subject, hint));
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.issues.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// 某码计数。
    pub fn count_of(&self, code: PreviewCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.issues.len() {
            if self.issues[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某严重度计数。
    pub fn count_severity(&self, s: Severity) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.issues.len() {
            if self.issues[i].severity == s {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// P1 条数（红线计数）。
    pub fn p1_count(&self) -> usize {
        self.count_severity(Severity::P1)
    }

    /// 是否含某码。
    pub fn has(&self, code: PreviewCode) -> bool {
        self.count_of(code) > 0
    }

    /// 逐条借出。
    pub fn issues(&self) -> &[PreviewIssue] {
        &self.issues
    }

    /// 渲染全部。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.issues.len() {
            out.push_str(&self.issues[i].render());
            out.push('\n');
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 二、沙箱预览环境（隔离单源在 F3605）
// ---------------------------------------------------------------------------

/// 沙箱预览环境。
///
/// **隔离靠结构**：本结构**只**持有 [`PreviewChannel`]，而该通道类型上
/// 没有写正式缓存的方法——预览帧进得来，正式缓存出不去。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SandboxPreview {
    /// 预览帧（按提交序）。
    frames: Vec<PreviewFrame>,
    /// 观测到的正式缓存条目数（由 F3605 正式通道回填）。
    formal_entries: u32,
    /// 隔离是否仍完好（构造时取基线，之后只读）。
    isolation_ok: bool,
}

impl SandboxPreview {
    /// 新建沙箱（`isolation_ok` 由调用方用 F3605 核验后写入）。
    pub fn new(isolation_ok: bool) -> SandboxPreview {
        SandboxPreview { frames: Vec::new(), formal_entries: 0, isolation_ok }
    }

    /// 提交一帧预览（**O(1)**——锚点沙箱 O(1)；不重建任何派生状态）。
    pub fn push(&mut self, frame: PreviewFrame) {
        self.frames.push(frame);
    }

    /// 帧数。
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// 借出帧。
    pub fn frames(&self) -> &[PreviewFrame] {
        &self.frames
    }

    /// 记录正式通道当前的条目数（供隔离核验）。
    pub fn observe_formal(&mut self, entries: u32) {
        self.formal_entries = entries;
    }

    /// 正式缓存条目数。
    pub fn formal_entries(&self) -> u32 {
        self.formal_entries
    }

    /// 隔离核验：**O(1)**，转发 F3605 的 [`PreviewChannel::isolation_intact`]
    /// 与 [`PreviewChannel::contamination`]。
    ///
    /// 刻意**不自己遍历帧或重算正式缓存**——隔离判定的权威在 F3605
    /// （基线快照 vs 观测值），本条只消费结论并叠加本地
    /// `isolation_ok` 开关。两处各自算一遍隔离，等于有两个真相。
    pub fn isolation_intact(&self, channel: &PreviewChannel) -> bool {
        self.isolation_ok && channel.isolation_intact() && channel.contamination().is_none()
    }
}

// ---------------------------------------------------------------------------
// 三、保真对拍器（预览 → 发布）
// ---------------------------------------------------------------------------

/// 一次保真对拍的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParityVerdict {
    /// 抽样数。
    pub sampled: usize,
    /// 一致数。
    pub matched: usize,
    /// 分歧数。
    pub drifted: usize,
}

impl ParityVerdict {
    /// 是否全对——**须抽样非空**（空抽样不算通过）。
    pub fn all_match(&self) -> bool {
        self.sampled > 0 && self.drifted == 0
    }

    /// 三计数是否自洽（和 == 抽样数）。
    pub fn is_consistent(&self) -> bool {
        self.matched + self.drifted == self.sampled
    }
}

/// 保真对拍器：把「预览侧指纹」与「发布侧指纹」逐条对拍。
///
/// **与 F3605 `parity_sample` 的分工**：F3605 钉「票据指纹 vs 引擎指纹」，
/// 本条钉「**预览**指纹 vs **发布**指纹」。两者方向不同，**不可互相替代**
/// ——用 F3605 的通过来论证预览保真，是拿A 证据办B 事。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FidelityParity {
    sampled: usize,
    matched: usize,
    drifted: usize,
}

impl FidelityParity {
    /// 新建对拍器。
    pub fn new() -> FidelityParity {
        FidelityParity { sampled: 0, matched: 0, drifted: 0 }
    }

    /// 追加一次对拍（`preview_fp` 与 `publish_fp` 相同即一致）。
    pub fn compare(&mut self, preview_fp: u64, publish_fp: u64) {
        self.sampled = self.sampled.saturating_add(1);
        if preview_fp == publish_fp {
            self.matched = self.matched.saturating_add(1);
        } else {
            self.drifted = self.drifted.saturating_add(1);
        }
    }

    /// 结算（**红线**：分歧 >0 记 P1；抽样为空单独记退化码）。
    pub fn settle(&self, bag: &mut PreviewBag) -> ParityVerdict {
        let v = ParityVerdict { sampled: self.sampled, matched: self.matched, drifted: self.drifted };
        if v.sampled == 0 {
            // 空抽样显性立案：绝不让「没查」沉默地读成「都对」
            bag.push(
                PreviewCode::PARITY_EMPTY,
                "sampled=0",
                "对拍必须至少抽一条；空抽样不得判为通过",
            );
        } else if v.drifted > 0 {
            bag.push(
                PreviewCode::PARITY_DRIFT,
                format!("drifted={}/{}", v.drifted, v.sampled),
                "预览与发布指纹不一致：创作者会看到与发布不同的画面，须修渲染或修对拍口径",
            );
        }
        v
    }

    /// 计数（独立于 [`Self::settle`]，便于判据读裸计数）。
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.sampled, self.matched, self.drifted)
    }
}

// ---------------------------------------------------------------------------
// 四、实时断言（≤100ms）
// ---------------------------------------------------------------------------

/// 一次预览刷新的耗时记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefreshTick {
    /// 编辑操作到预览刷新的毫秒数。
    pub elapsed_ms: u32,
    /// 本次刷新涉及的变更对象数（刷新是 O(变更量)）。
    pub changed: usize,
}

/// 实时断言器：把**超预算**变成显式立案。
///
/// 无墙钟环境里耗时由调用方注入（确定性工作量预算，非性能实测）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RealtimeAssert {
    /// 已记账的刷新次数。
    pub ticks: usize,
    /// 超预算次数。
    pub over: usize,
}

impl RealtimeAssert {
    /// 新建断言器。
    pub fn new() -> RealtimeAssert {
        RealtimeAssert { ticks: 0, over: 0 }
    }

    /// 记一次刷新并判定是否超预算（**同时**记袋——记账与立案不分离，
    /// 否则调用方可能只记账不立案，红线就软了）。
    pub fn record(&mut self, tick: RefreshTick, bag: &mut PreviewBag) -> bool {
        self.ticks = self.ticks.saturating_add(1);
        if tick.changed > MAX_CHANGED {
            bag.push(
                PreviewCode::CHANGE_RANGE,
                format!("changed={}", tick.changed),
                format!("单次刷新变更对象数须 ≤ {MAX_CHANGED}"),
            );
        }
        if tick.elapsed_ms > REALTIME_BUDGET_MS {
            self.over = self.over.saturating_add(1);
            bag.push(
                PreviewCode::REALTIME_OVER,
                format!("{}ms > {}ms", tick.elapsed_ms, REALTIME_BUDGET_MS),
                "预览刷新超出实时预算：须优化或降渐进粒度，不得静默放行",
            );
            return false;
        }
        true
    }

    /// 超预算比例的整数口径（分子, 分母）——不返浮点。
    pub fn over_ratio(&self) -> (usize, usize) {
        (self.over, self.ticks)
    }
}

// ---------------------------------------------------------------------------
// 五、渐进复用
// ---------------------------------------------------------------------------

/// 渐进复用账本。
///
/// **破碎时的口径**：块数越界只**拒绝新增块**并记一般问题，已入库的块与
/// 复用次数**原样保留**——账目仍自洽（这是「复用」而非「重算」的意义）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressiveReuse {
    /// 已加载块。
    chunks: Vec<u64>,
    /// 复用命中次数（块已在缓存里直接用）。
    pub reused: usize,
    /// 实际加载次数。
    pub loaded: usize,
}

impl ProgressiveReuse {
    /// 新建账本。
    pub fn new() -> ProgressiveReuse {
        ProgressiveReuse { chunks: Vec::new(), reused: 0, loaded: 0 }
    }

    /// 请求一块：`chunk_fp` 已在账本内即**复用**，否则加载。
    ///
    /// 返回 `true` 表示复用、`false` 表示新加载。
    pub fn request(&mut self, chunk_fp: u64, bag: &mut PreviewBag) -> bool {
        let mut i = 0usize;
        while i < self.chunks.len() {
            if self.chunks[i] == chunk_fp {
                self.reused = self.reused.saturating_add(1);
                return true;
            }
            i += 1;
        }
        if self.chunks.len() >= MAX_CHUNKS {
            // 破碎：拒绝新增但**不丢已有**，账目自洽
            bag.push(
                PreviewCode::CHUNK_RANGE,
                format!("chunks={}/{}", self.chunks.len(), MAX_CHUNKS),
                format!("渐进块数已达上限 {MAX_CHUNKS}：请合并块或降粒度，已有块与复用计数保持不变"),
            );
            return false;
        }
        self.chunks.push(chunk_fp);
        self.loaded = self.loaded.saturating_add(1);
        false
    }

    /// 块数。
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// 块指纹（判据可借出核对去重性）。
    pub fn chunks(&self) -> &[u64] {
        &self.chunks
    }

    /// 复用率整数口径（分子, 分母）。
    pub fn reuse_ratio(&self) -> (usize, usize) {
        (self.reused, self.reused + self.loaded)
    }

    /// 账目自洽（**恰等于**，不是恒真式）。
    ///
    /// 不变量有两条，缺一即是账本被写坏：
    /// ① 每次成功加载恰向账本推入一块 ⇒ `chunk_count == loaded`——
    ///    `loaded` 多计（推块失败仍计数）或块被静默丢弃（推入却没计数）
    ///    都会打破它；破碎路径只拒绝新增、不动已有账目，故破碎时
    ///    依旧成立（这正是「自洽」的含义：账目对得上，复用率下降而已）。
    /// ② 记账只增不减 ⇒ `reused + loaded` 是总请求中「复用+加载」的
    ///    全部去向（被拒绝的请求既不复用也不加载，不在其内）。
    /// 旧版曾写成 `reused + loaded >= reused`——那是恒真式，等于没有门。
    pub fn is_consistent(&self) -> bool {
        self.chunks.len() == self.loaded
    }
}

// ---------------------------------------------------------------------------
// 六、预览运行时（编排）
// ---------------------------------------------------------------------------

/// 创作预览运行时：把沙箱、对拍、实时、渐进四件事编排到一条编辑回路。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreviewRuntime {
    /// 沙箱。
    pub sandbox: SandboxPreview,
    /// 保真对拍器。
    pub parity: FidelityParity,
    /// 实时断言器。
    pub realtime: RealtimeAssert,
    /// 渐进复用账本。
    pub progressive: ProgressiveReuse,
    /// 本次预览已落地的帧数（**所见即所得**的实测量）。
    ///
    /// 与 `sandbox.frames.len()` **不是同一个计数器**：沙箱记「收了多少帧」，
    /// 本字段记「创作者真的看到了多少帧」。两者的差就是「在沙箱里但没被呈现」
    /// 的帧（超 100ms 预算来不及显示），正是所见即所得要抓的破口。
    landed_frames: usize,
    /// 最近一次渐进块请求是否**复用**（由 [`ProgressiveReuse::request`] 写入）。
    last_chunk_reused: bool,
}

impl PreviewRuntime {
    /// 新建运行时（`isolation_ok` 由 F3605 核验给出）。
    pub fn new(isolation_ok: bool) -> PreviewRuntime {
        PreviewRuntime {
            sandbox: SandboxPreview::new(isolation_ok),
            parity: FidelityParity::new(),
            realtime: RealtimeAssert::new(),
            progressive: ProgressiveReuse::new(),
            landed_frames: 0,
            last_chunk_reused: false,
        }
    }

    /// 编辑回路一步：提交预览帧 → 对拍 → 记实时 → 请求渐进块。
    ///
    /// **O(变更量)**：帧按提交序追加，不重建全量派生状态。
    ///
    /// **落地口径（本条最容易被做假的地方）**：沙箱 `push` 与
    /// `landed_frames` 自增**刻意不是同一件事**——只有**在实时预算内**
    /// 刷新完成的帧才算「创作者真的看到了」。超预算的帧进了沙箱（数据在）
    /// 但没被呈现（用户眼前仍是上一帧），此时两计数器出现差值，
    /// [`Self::is_what_you_see_is_what_you_get`] 才有假可抓。
    ///
    /// 若把两者绑在一起自增，谓词恒真、判据无从证伪——这不是更简洁，
    /// 是把红线做成装饰。
    pub fn step(
        &mut self,
        frame: PreviewFrame,
        publish_fp: u64,
        tick: RefreshTick,
        chunk_fp: u64,
        bag: &mut PreviewBag,
    ) -> ParityVerdict {
        let preview_fp = frame.fp;
        self.sandbox.push(frame);
        let within_budget = self.realtime.record(tick, bag);
        if within_budget {
            self.landed_frames = self.landed_frames.saturating_add(1);
        }
        self.parity.compare(preview_fp, publish_fp);
        self.last_chunk_reused = self.progressive.request(chunk_fp, bag);
        self.parity.settle(bag)
    }

    /// 已落地（**真的被呈现**）帧数。
    pub fn landed_frames(&self) -> usize {
        self.landed_frames
    }

    /// 最近一步的渐进块是否走了复用。
    pub fn last_chunk_reused(&self) -> bool {
        self.last_chunk_reused
    }

    /// 所见即所得自检：落地帧数与沙箱帧数必须**恰等于**。
    ///
    /// 「所见」（`landed_frames`，创作者真的看到的）与「所得」
    /// （`sandbox.len()`，沙箱真的收到的）是两个**各自记**的计数器。
    /// 超预算帧落在沙箱却没落地 ⇒ 两值不等 ⇒ 此处为假，红线生效。
    pub fn is_what_you_see_is_what_you_get(&self) -> bool {
        self.landed_frames == self.sandbox.len()
    }

    /// 「在沙箱里但没被呈现」的帧数（所见即所得的**缺口**）。
    ///
    /// 饱和减：口径是「缺口」，负数无意义。
    pub fn unshown_frames(&self) -> usize {
        self.sandbox.len().saturating_sub(self.landed_frames)
    }

    /// 隔离核验（转发 F3605 口径）。
    pub fn isolation_intact(&self, channel: &PreviewChannel) -> bool {
        self.sandbox.isolation_intact(channel)
    }

    /// 隔离核验 + 污染时**显性立案**（`0x3604`）。
    ///
    /// 转发结论的同时把污染**记进诊断袋**——只回一个 `false` 而袋里
    /// 空着，调用方容易当成「暂时没有」而放过，红线就断在这里。
    /// 严重度仍由 [`PreviewCode::is_p1`] 决定，调用方不得降级。
    pub fn check_isolation(
        &self,
        channel: &PreviewChannel,
        bag: &mut PreviewBag,
    ) -> bool {
        let intact = self.sandbox.isolation_intact(channel);
        if let Some(issue) = channel.contamination() {
            bag.push(
                PreviewCode::CONTAMINATED,
                format!("{} {}", issue.code, issue.subject),
                issue.detail,
            );
        }
        intact
    }
}

// ---------------------------------------------------------------------------
// 七、对外入口
// ---------------------------------------------------------------------------

/// 一次完整的预览刷新结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewOutcome {
    /// 保真判定。
    pub parity: ParityVerdict,
    /// 本步是否在实时预算内。
    pub realtime_ok: bool,
    /// 本步渐进块是否复用（读 `ProgressiveReuse::request` 的**本步返回值**）。
    pub chunk_reused: bool,
    /// P1 条数。
    pub p1: usize,
    /// 本步是否真的被呈现给创作者（预算内才为真）。
    pub landed: bool,
    /// 累计「在沙箱里但没被呈现」的帧数（所见即所得缺口）。
    pub unshown: usize,
}

/// 跑一步预览并给出结论（**唯一对外入口**，便于 F3622/F3623 编辑对端调用）。
pub fn preview_step(rt: &mut PreviewRuntime, input: PreviewInput, bag: &mut PreviewBag) -> PreviewOutcome {
    let p1_before = bag.p1_count();
    let over_before = bag.count_of(PreviewCode::REALTIME_OVER);
    let v = rt.step(input.frame, input.publish_fp, input.tick, input.chunk_fp, bag);
    let realtime_ok = bag.count_of(PreviewCode::REALTIME_OVER) == over_before;
    let chunk_reused = rt.last_chunk_reused();
    PreviewOutcome {
        parity: v,
        realtime_ok,
        chunk_reused,
        p1: bag.p1_count() - p1_before,
        landed: realtime_ok,
        unshown: rt.unshown_frames(),
    }
}

/// 预览一步的输入。
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewInput {
    /// 预览帧。
    pub frame: PreviewFrame,
    /// 发布侧指纹（对拍基准）。
    pub publish_fp: u64,
    /// 刷新耗时记录。
    pub tick: RefreshTick,
    /// 渐进块指纹。
    pub chunk_fp: u64,
}

impl PreviewInput {
    /// 构造。
    pub fn new(
        frame: PreviewFrame,
        publish_fp: u64,
        elapsed_ms: u32,
        changed: usize,
        chunk_fp: u64,
    ) -> PreviewInput {
        PreviewInput {
            frame,
            publish_fp,
            tick: RefreshTick { elapsed_ms, changed },
            chunk_fp,
        }
    }
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

/// F3606 域自检。
pub fn run_ves06_checks() -> CheckSet {
    let mut s = CheckSet::new("ves06_preview");
    check_codes(&mut s);
    check_parity_independence(&mut s);
    check_sandbox(&mut s);
    check_parity(&mut s);
    check_realtime(&mut s);
    check_progressive(&mut s);
    check_wysiwyg(&mut s);
    check_isolation(&mut s);
    s
}

/// 判据 1：诊断码自洽（码位互异、标签互异、红线归属正确）。
fn check_codes(s: &mut CheckSet) {
    let n = PreviewCode::ALL.len();
    let mut i = 0usize;
    let mut code_ok = true;
    let mut label_ok = true;
    while i < n {
        let mut j = 0usize;
        while j < n {
            if i != j {
                code_ok &= PreviewCode::ALL[i].code() != PreviewCode::ALL[j].code();
                label_ok &= PreviewCode::ALL[i].label() != PreviewCode::ALL[j].label();
            }
            j += 1;
        }
        i += 1;
    }
    s.add("S06-诊断-码位与标签两两互异", code_ok && label_ok, "六码须两两互异（互异才能按码归类）");
    s.add(
        "S06-诊断-保真与实时为P1红线",
        PreviewCode::PARITY_DRIFT.is_p1() && PreviewCode::REALTIME_OVER.is_p1(),
        "保真分歧与超预算必须是 P1",
    );
    s.add(
        "S06-诊断-退化与越界不是P1",
        !PreviewCode::PARITY_EMPTY.is_p1()
            && !PreviewCode::CHUNK_RANGE.is_p1()
            && !PreviewCode::CHANGE_RANGE.is_p1()
            && !PreviewCode::CONTAMINATED.is_p1(),
        "空抽样/越界/污染不得冒充红线（否则红线通胀即等于没有红线）",
    );
    // 袋：P1 可查 + 精确计数
    let mut b = PreviewBag::new();
    b.push(PreviewCode::PARITY_DRIFT, "a", "x");
    b.push(PreviewCode::PARITY_DRIFT, "b", "x");
    b.push(PreviewCode::CHUNK_RANGE, "c", "x");
    s.add(
        "S06-诊断-袋精确计数且严重度由码决定",
        b.len() == 3 && b.count_of(PreviewCode::PARITY_DRIFT) == 2 && b.p1_count() == 2,
        "3 条：2 条保真分歧（均 P1）+ 1 条块越界（非 P1）",
    );
    s.add("S06-诊断-渲染含定位与处置", b.render().contains("定位：") && b.render().contains("处置："), "每条须可读可纠");
    // 未知码须有兜底标签，不得 panic
    s.add(
        "S06-诊断-未知码有兜底不panic",
        !PreviewCode(0xFFFF).label().is_empty(),
        "未登记码位须返回兜底人话而不是崩掉",
    );
}

/// 判据 1b：**两套对拍方向不可混同**（F3605 票据侧 vs 本条预览侧）。
///
/// 用真实 F3605 [`parity_sample`] 造一个「票据侧全一致」的账本，再验证：
///即便F3605 全一致，本条仍能独立判出预览侧分歧——证明本条不是拿F3605 的
/// 通过来代替预览保真论证。
fn check_parity_independence(s: &mut CheckSet) {
    let mut ledger = DelegateLedger::empty();
    // 报名两张票据，指纹与「引擎侧」一致 ⇒ F3605 对拍全 match
    let t1 = ledger.enroll(Product::Theme, 0xAAAA, 64).unwrap_or(0);
    let t2 = ledger.enroll(Product::Theme, 0xBBBB, 64).unwrap_or(0);
    let engine_fps: [(u32, u64); 2] = [(t1, 0xAAAA), (t2, 0xBBBB)];
    let f3605 = parity_sample(&ledger, &engine_fps, 2).unwrap_or(ParityReport {
        sampled: 0,
        matched: 0,
        drifted: 0,
    });
    s.add(
        "S06-对拍-票据侧全一致",
        f3605.all_match() && f3605.is_consistent() && f3605.sampled == 2,
        "F3605 口径：票据指纹与引擎指纹 2/2 一致",
    );
    // 同一时刻本条预览侧存在分歧：两个方向必须分别成立
    let mut mine = FidelityParity::new();
    mine.compare(0xAAAA, 0xAAAA);
    mine.compare(0xBBBB, 0x0000); // 预览画错了，但票据侧照样一致
    let mut bag = PreviewBag::new();
    let v = mine.settle(&mut bag);
    s.add(
        "S06-对拍-票据一致不掩盖预览分歧",
        f3605.all_match() && !v.all_match() && bag.count_of(PreviewCode::PARITY_DRIFT) == 1,
        "F3605 全match 同时本条记 1 条 P1：证明两方向独立、不可互相替代",
    );
    // 反向：预览侧全一致时也不该被 F3605 的结论污染
    let mut mine2 = FidelityParity::new();
    mine2.compare(0xAAAA, 0xAAAA);
    let mut bag2 = PreviewBag::new();
    let v2 = mine2.settle(&mut bag2);
    s.add(
        "S06-对拍-预览一致时零P1",
        v2.all_match() && bag2.p1_count() == 0,
        "预览侧一致须零 P1（反方向对照）",
    );
}

/// 判据 2：沙箱隔离（O(1) 两整数比对，不遍历）。
fn check_sandbox(s: &mut CheckSet) {
    let mut sp = SandboxPreview::new(true);
    sp.push(PreviewFrame::new(16, 0x1111));
    sp.push(PreviewFrame::new(32, 0x2222));
    s.add(
        "S06-沙箱-提交帧按序且计数正确",
        sp.len() == 2 && sp.frames()[0].fp == 0x1111 && sp.frames()[1].fp == 0x2222,
        "两帧须按提交序可读",
    );
    s.add("S06-沙箱-空环境无帧", SandboxPreview::new(true).is_empty(), "新建沙箱须为空");
}

/// 判据 3：保真对拍（含**空抽样不算通过**与三计数自洽）。
fn check_parity(s: &mut CheckSet) {
    // 全一致
    let mut p = FidelityParity::new();
    let mut i = 0u64;
    while i < 4 {
        p.compare(0xAA, 0xAA);
        i += 1;
    }
    let mut bag = PreviewBag::new();
    let v = p.settle(&mut bag);
    s.add(
        "S06-保真-全一致时通过且零P1",
        v.all_match() && v.is_consistent() && bag.p1_count() == 0,
        "4/4 一致须通过且不记 P1",
    );
    // 有分歧 ⇒ P1
    let mut p2 = FidelityParity::new();
    p2.compare(0xAA, 0xAA);
    p2.compare(0xAA, 0xBB); // 分歧
    let mut bag2 = PreviewBag::new();
    let v2 = p2.settle(&mut bag2);
    s.add(
        "S06-保真-分歧记P1红线",
        !v2.all_match() && bag2.count_of(PreviewCode::PARITY_DRIFT) == 1 && bag2.p1_count() == 1,
        "1 分歧须记 1 条 P1",
    );
    // 三计数之和恰等于抽样数（判据侧独立数）
    let total = v2.sampled;
    s.add(
        "S06-保真-三计数自洽",
        v2.matched + v2.drifted == total && v2.matched == 1 && v2.drifted == 1,
        "matched+drifted 恰等于 sampled",
    );
    // **空抽样不算通过**（退化口径）
    let empty = FidelityParity::new();
    let mut bag3 = PreviewBag::new();
    let v3 = empty.settle(&mut bag3);
    s.add(
        "S06-保真-空抽样不算通过且显性立案",
        !v3.all_match() && bag3.count_of(PreviewCode::PARITY_EMPTY) == 1,
        "sampled=0 须判不通过并记退化码（否则「没查」被读成「都对」）",
    );
}

/// 判据 4：100ms 实时（**用字面量 100/101 卡两侧**，不引用常量）。
fn check_realtime(s: &mut CheckSet) {
    let mut ra = RealtimeAssert::new();
    let mut bag = PreviewBag::new();
    // 字面量：恰 100ms 合法（不引用 REALTIME_BUDGET_MS）
    let at = ra.record(RefreshTick { elapsed_ms: 100, changed: 4 }, &mut bag);
    // 字面量：101ms 超限
    let over = ra.record(RefreshTick { elapsed_ms: 101, changed: 4 }, &mut bag);
    s.add(
        "S06-实时-100ms边界两侧判定正确",
        at && !over && ra.over == 1 && bag.count_of(PreviewCode::REALTIME_OVER) == 1,
        "100ms 合法 / 101ms 超限（用字面量钉死，不随常量漂移）",
    );
    s.add(
        "S06-实时-超限记P1红线",
        bag.p1_count() == 1,
        "超预算须为 P1",
    );
    // 变更对象数越界
    let mut ra2 = RealtimeAssert::new();
    let mut bag2 = PreviewBag::new();
    ra2.record(RefreshTick { elapsed_ms: 10, changed: MAX_CHANGED + 1 }, &mut bag2);
    s.add(
        "S06-实时-变更对象数越界被拒",
        bag2.count_of(PreviewCode::CHANGE_RANGE) == 1,
        "变更对象数须 ≤ MAX_CHANGED",
    );
    // 超限比例整数口径
    let (num, den) = ra.over_ratio();
    s.add("S06-实时-超限比例恰等于记账", num == 1 && den == 2, "1 次超限 / 共 2 次记账");
}

/// 判据 5：渐进复用（复用命中 + 破碎自洽）。
fn check_progressive(s: &mut CheckSet) {
    let mut pr = ProgressiveReuse::new();
    let mut bag = PreviewBag::new();
    // 同一块请求两次 ⇒ 第二次复用
    let a = pr.request(0xC1, &mut bag);
    let b = pr.request(0xC1, &mut bag);
    s.add(
        "S06-渐进-同块二次请求走复用",
        !a && b && pr.chunk_count() == 1 && pr.reused == 1 && pr.loaded == 1,
        "1 块加载 + 1 次复用",
    );
    // 不同块 ⇒ 新加载
    let c = pr.request(0xC2, &mut bag);
    s.add(
        "S06-渐进-异块走加载",
        !c && pr.chunk_count() == 2 && pr.loaded == 2,
        "第二块须新加载",
    );
    // 块指纹去重（账本内不得有重复）
    let mut dup = false;
    let ch = pr.chunks();
    let mut i = 0usize;
    while i < ch.len() {
        let mut j = i + 1;
        while j < ch.len() {
            if ch[i] == ch[j] {
                dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("S06-渐进-块账本无重复", !dup, "复用即不重复入库");
    // 破碎：灌满上限后再请求 ⇒ 拒绝新增但**已有块不丢**
    let mut pr2 = ProgressiveReuse::new();
    let mut bag2 = PreviewBag::new();
    let mut k = 0u64;
    while k < MAX_CHUNKS as u64 {
        pr2.request(k, &mut bag2);
        k += 1;
    }
    let before = pr2.chunk_count();
    let after_req = pr2.request(0xFFFF, &mut bag2);
    s.add(
        "S06-渐进-破碎拒绝新增但账目自洽",
        !after_req
            && pr2.chunk_count() == before
            && bag2.count_of(PreviewCode::CHUNK_RANGE) == 1
            && bag2.p1_count() == 0,
        "块数须仍等于 MAX_CHUNKS、已有块不丢、记一般问题非 P1",
    );
    s.add(
        "S06-渐进-复用率整数口径",
        {
            let (num, den) = pr.reuse_ratio();
            num == 1 && den == 3
        },
        "1 次复用 / 共 3 次请求",
    );
    // 账目自洽（恰等于口径）：正常态与破碎态都要成立——
    // 只在正常态断，会把「破碎丢账」这种最需要自洽的场景漏掉。
    s.add(
        "S06-渐进-账目自洽正常态块数恰等于加载数",
        pr.is_consistent() && pr.chunk_count() == pr.loaded && pr.loaded == 2,
        "2 块入库 == 2 次加载：loaded 多计或块被丢都会打破恰等于",
    );
    s.add(
        "S06-渐进-账目自洽破碎态仍成立",
        pr2.is_consistent() && pr2.chunk_count() == pr2.loaded && pr2.chunk_count() == MAX_CHUNKS,
        "破碎拒绝新增不动已有账目：块数仍 == 加载数（自洽而非重算）",
    );
}

/// 判据 6：所见即所得（两计数器**真能脱钩** + 缺口可观测）。
fn check_wysiwyg(s: &mut CheckSet) {
    let mut rt = PreviewRuntime::new(true);
    let mut bag = PreviewBag::new();
    let mut i = 0u64;
    while i < 3 {
        let input = PreviewInput::new(PreviewFrame::new(8, 0x100 + i), 0x100 + i, 10, 2, 0xC0 + i);
        let out = preview_step(&mut rt, input, &mut bag);
        // 每步都应全一致（发布指纹 == 预览指纹）
        if !out.parity.all_match() {
            s.add("S06-所见即所得-三步全一致", false, "发布指纹须与预览逐条相等");
            return;
        }
        i += 1;
    }
    s.add(
        "S06-所见即所得-三步全一致且落地帧数对得上",
        rt.landed_frames() == 3
            && rt.sandbox.len() == 3
            && rt.unshown_frames() == 0
            && rt.is_what_you_see_is_what_you_get(),
        "落地 3 帧 = 沙箱 3 帧，缺口 0",
    );
    s.add(
        "S06-所见即所得-一致路径零P1",
        bag.p1_count() == 0,
        "全一致且未超预算须零 P1",
    );

    // —— **反例**：超预算帧进沙箱但**没被呈现**，两计数器必须脱钩 ——
    // 这一条是 M10 的判据侧锚点。原实现把 landed 与 sandbox 绑在一起自增，
    // 谓词恒真、无从证伪；这里要求「超预算一步」后两值不等且缺口恰为 1。
    let mut rt2 = PreviewRuntime::new(true);
    let mut bag2 = PreviewBag::new();
    let out2 = preview_step(
        &mut rt2,
        PreviewInput::new(PreviewFrame::new(8, 1), 1, 500, 2, 0xC0),
        &mut bag2,
    );
    s.add(
        "S06-所见即所得-超预算步被标记且记P1",
        !out2.realtime_ok && out2.p1 == 1 && !out2.landed,
        "500ms 一步须 realtime_ok=false、landed=false 且新增 1 条 P1",
    );
    s.add(
        "S06-所见即所得-超预算帧在沙箱但不算落地",
        rt2.sandbox.len() == 1
            && rt2.landed_frames() == 0
            && rt2.unshown_frames() == 1
            && !rt2.is_what_you_see_is_what_you_get(),
        "沙箱收了 1 帧但创作者没看到：落地 0、缺口 1、所见即所得为假（否则该谓词恒真不可证伪）",
    );

    // 分歧一步 ⇒ P1（不影响落地口径：预算是 10ms，仍算呈现）
    let mut rt3 = PreviewRuntime::new(true);
    let mut bag3 = PreviewBag::new();
    let out3 = preview_step(
        &mut rt3,
        PreviewInput::new(PreviewFrame::new(8, 0xAA), 0xBB, 10, 2, 0xC0),
        &mut bag3,
    );
    s.add(
        "S06-所见即所得-分歧步被标记",
        !out3.parity.all_match() && out3.p1 == 1,
        "预览 0xAA vs 发布 0xBB 须记 1 条 P1",
    );

    // 混合序列：3 正常 + 1 超预算 ⇒ 缺口恰为 1（不是 >=，用 == 钉死）
    let mut rt4 = PreviewRuntime::new(true);
    let mut bag4 = PreviewBag::new();
    let seq: [(u64, u32); 4] = [(1, 10), (2, 10), (3, 900), (4, 10)];
    let mut k = 0usize;
    while k < seq.len() {
        let (fp, ms) = seq[k];
        preview_step(
            &mut rt4,
            PreviewInput::new(PreviewFrame::new(8, fp), fp, ms, 2, 0xC0),
            &mut bag4,
        );
        k += 1;
    }
    s.add(
        "S06-所见即所得-混合序列缺口恰为一",
        rt4.sandbox.len() == 4
            && rt4.landed_frames() == 3
            && rt4.unshown_frames() == 1
            && !rt4.is_what_you_see_is_what_you_get(),
        "4 帧里 1 帧超 100ms 未呈现：缺口恰为 1（== 钉死，不用 >=）",
    );

    // 渐进复用标记读本步真值：同块二次 ⇒ 复用
    let mut rt5 = PreviewRuntime::new(true);
    let mut bag5 = PreviewBag::new();
    let o1 = preview_step(
        &mut rt5,
        PreviewInput::new(PreviewFrame::new(8, 0x20), 0x20, 10, 2, 0xD1),
        &mut bag5,
    );
    let o2 = preview_step(
        &mut rt5,
        PreviewInput::new(PreviewFrame::new(8, 0x21), 0x21, 10, 2, 0xD1),
        &mut bag5,
    );
    s.add(
        "S06-渐进-复用标记逐步正确",
        !o1.chunk_reused && o2.chunk_reused && rt5.progressive.loaded == 1 && rt5.progressive.reused == 1,
        "同块首次非复用、二次为复用（读 request 本步返回值而非累计口径）",
    );
}

/// 判据 7：隔离核验（M15 的判据侧锚点——**污染反例必须可观测**）。
///
/// 三向对照：干净 ⇒ 真；污染 ⇒ 假**且显性立案**；构造期核验未过 ⇒ 假。
/// 少任何一向，`isolation_intact` 都可以恒真而不被任何判据抓到。
fn check_isolation(s: &mut CheckSet) {
    // 干净通道：正式缓存条目数与基线一致
    let formal = FormalChannel::new();
    let clean = PreviewChannel::new(&formal);
    let rt_ok = PreviewRuntime::new(true);
    let mut bag_ok = PreviewBag::new();
    s.add(
        "S06-隔离-干净通道核验通过",
        clean.isolation_intact()
            && clean.contamination().is_none()
            && rt_ok.check_isolation(&clean, &mut bag_ok)
            && bag_ok.p1_count() == 0,
        "基线==观测 ⇒ 隔离完好且零 P1",
    );

    // 污染通道：正式缓存被预览改动过（基线 0 → 观测 1）
    let mut dirty = PreviewChannel::new(&formal);
    dirty.observe_formal(1);
    let mut bag_bad = PreviewBag::new();
    let intact = rt_ok.check_isolation(&dirty, &mut bag_bad);
    s.add(
        "S06-隔离-污染通道判为不隔离",
        !dirty.isolation_intact()
            && dirty.contamination().is_some()
            && !intact
            && !rt_ok.isolation_intact(&dirty),
        "正式缓存被改 ⇒ 隔离核验须为假（否则该谓词恒真无从证伪）",
    );
    s.add(
        "S06-隔离-污染显性立案且非P1",
        bag_bad.count_of(PreviewCode::CONTAMINATED) == 1 && bag_bad.p1_count() == 0,
        "隔离破坏须留痕：1 条 CONTAMINATED；严重度由码决定，污染不冒充 P1（否则红线通胀）",
    );

    // 构造期核验未过：即便通道干净也须判不隔离（本地开关是独立一支）
    let rt_bad = PreviewRuntime::new(false);
    s.add(
        "S06-隔离-构造核验未过即便通道干净也判不隔离",
        clean.isolation_intact() && !rt_bad.isolation_intact(&clean) && !rt_bad.check_isolation(&clean, &mut PreviewBag::new()),
        "本地 isolation_ok=false 须独立生效（只转发 F3605 会漏掉本地这一支）",
    );

    // 双向对照：同一对通道，仅回填值不同 ⇒ 结论必须不同
    let mut ch2 = PreviewChannel::new(&formal);
    let mut bag2 = PreviewBag::new();
    let before = rt_ok.check_isolation(&ch2, &mut bag2);
    ch2.observe_formal(1);
    let after = rt_ok.check_isolation(&ch2, &mut bag2);
    s.add(
        "S06-隔离-结论随正式缓存变化而翻转",
        before && !after && bag2.count_of(PreviewCode::CONTAMINATED) == 1,
        "同一 runtime，仅正式缓存被改 ⇒ 结论须由真翻为假（恒真谓词过不了这一条）",
    );
}

// 引入 CheckSet（判据层类型）。
use crate::checks::CheckSet;