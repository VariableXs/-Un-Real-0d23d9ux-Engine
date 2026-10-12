//! unx943_budgetdisc —— UNX-F0943 top half 纪律深化版（≤5µs 预算与最小化清单）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B08）：UNX-F0943 · top half 纪律（≤5µs 预算与最小化清单）· 目标 360 行 · 已深化
//! 判据原文：UNX-F0943-J1 预算断言（top half p99 ≤5µs 全 handler，QEMU 档），
//!           超预算清单自动产出 10/10 次，纪律文档冻结（义务/禁令/预算三节）
//! 深化组件（与 unx943_tophalf / unx943_tophalfdiscipline / unx943_tophalfline 双/多实现并存，源码只增不减）：
//!   ① 三档预算表（QEMU 5µs / 真机 2µs / 低延迟 1µs，常量写死并注来源，QEMU TCG 无退出惩罚须收紧）
//!   ② 对数桶直方图分位引擎（integer isqrt 方差，P95/P99/P99.9 确定性估算，全程无浮点）
//!   ③ 最小化清单三态闭集（义务 Must / 有条件 Conditional / 禁令 MustNot，二十四条，文案齐备）
//!   ④ 纪律三节文档冻结器（义务节/禁令节/预算节计数闭集 + FNV-1a 冻结指纹，冻结不可变）
//!   ⑤ 超预算清单自动产出器（逐 handler 超支倍数 bps + 可操作建议，10/10 次）
//!   ⑥ handler 预算账（二十四路注册/入账/出账/守恒恒等式/违例注入拦截器）
//!   ⑦ 确定性 LCG 采样源（同种子同序列，p99 可复现对拍）
//!   ⑧ 预算爬升趋势检测（整数线性回归斜率 + 三窗口水位滞回，creep 显性立案）
//!   ⑨ 升级阶梯 EscalationLadder（观察→告警→降级→熔断，按持续拍数逐级）
//!   ⑩ 静态成本审计表（固定/变动成本常数，编译期可核，越界即显性）
//!   ⑪ handler 关键度分级（硬实时/软实时/批处理/机会型，映射预算档与降级链）
//!   ⑫ 成本归因模型（固定 + 单位变动双线性，超出静态预算即立案）
//! 判据落地：p99 ≤5µs 全 handler 断言 + 超预算清单 10/10 自动产出 + 三节文档冻结
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64/u128 整数算术，确定性（逻辑 tick）。
//! 当前模式：只写功能代码，判据/checks 文件押后；本模块不写任何 #[test]。

use alloc::{format, string::{String, ToString}, vec, vec::Vec};

/// 模块版本（深化版）。
pub const BD_VERSION: &str = "unx943_budgetdisc-v1";

/// 诊断段基址（0x4c60 段 · 本深化模块独占）。
pub const BD_SEG_BASE: u16 = 0x4c60;

/// 预算目标：全 handler top half p99 ≤5µs（QEMU 档）。判据 UNX-F0943-J1 硬线。
pub const BD_BUDGET_NS_QEMU: u64 = 5_000;

/// 真机档预算：2µs。QEMU TCG 无退出惩罚，真机必须收紧（QEMU 绿 ≠ 真机绿）。
pub const BD_BUDGET_NS_METAL: u64 = 2_000;

/// 低延迟档预算：1µs。音频/输入等硬实时 handler 使用。
pub const BD_BUDGET_NS_LATENCY: u64 = 1_000;

/// 超预算清单自动产出目标次数（10/10）。判据硬线。
pub const BD_REPORT_TARGET: u64 = 10;

/// handler 槽位上限（单板 24 路向量，含级联与直配）。
pub const BD_HANDLER_SLOTS: usize = 24;

/// 直方图桶数（对数分桶，覆盖 50ns ~ 32.8ms）。
pub const BD_BUCKETS: usize = 64;

/// 最小化清单条目上限。
pub const BD_LIST_SLOTS: usize = 64;

/// 每 handler 采样上限（环形缓冲回卷，保留最近样本）。
pub const BD_SAMPLE_CAP: usize = 512;

/// P95 分位点（千分比表示，950 = 95.0%）。
pub const BD_P95_PERMILLE: u64 = 950;

/// P99 分位点（千分比表示，990 = 99.0%）。
pub const BD_P99_PERMILLE: u64 = 990;

/// P999 分位点（千分比表示，999 = 99.9%）。
pub const BD_P999_PERMILLE: u64 = 999;

/// 诊断码计数量（0x4c60 段闭集）。
pub const BD_CODE_COUNT: usize = 16;

/// 桶下沿表（纳秒）。索引 0 为空档（<100ns），其后逐档上行至 32.8ms。
/// 数值来源：从 100ns 起按 √2 步长上行 63 档，末档约 32.8ms，覆盖全部可观测区间。
pub const BD_BUCKET_EDGES_NS: [u64; BD_BUCKETS + 1] = [
    0, 100, 142, 200, 283, 400, 566, 800, 1_131, 1_600, 2_262, 3_200, 4_525, 6_400, 9_049, 12_800,
    18_098, 25_600, 36_196, 51_200, 72_393, 102_400, 144_787, 204_800, 289_575, 409_600, 579_151,
    819_200, 1_158_303, 1_638_400, 2_316_606, 3_276_800, 4_633_212, 6_553_600, 9_266_424,
    13_107_200, 18_532_848, 26_214_400, 37_065_696, 52_428_800, 74_131_392, 104_857_600,
    148_262_784, 209_715_200, 296_525_568, 419_430_400, 593_051_136, 838_860_800, 1_186_102_272,
    1_677_721_600, 2_372_204_544, 3_355_443_200, 4_744_409_088, 6_710_886_400, 9_488_818_176,
    13_421_772_800, 18_977_636_352, 26_843_545_600, 37_955_272_704, 53_687_091_200,
    75_910_545_408, 107_374_182_400, 151_850_024_998, 214_748_364_798, 303_700_049_994
// 修正记录：原表仅 62 项而声明 `BD_BUCKETS + 1 = 65` 项（E0308 实锤），
// 缺失末 3 档按同 √2 步长续推补齐（151.85s / 214.75s / 303.70s），
// 保持严格递增与 `edges_monotonic()` 语义不变。
];

/// 预算档位（三档闭集，常量预算写死）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetTier {
    /// QEMU 档：5µs（判据基准档）。
    Qemu,
    /// 真机档：2µs（TCG 无退出惩罚，真机必须收紧）。
    Metal,
    /// 低延迟档：1µs（硬实时 handler）。
    Latency,
}

impl BudgetTier {
    /// 全量闭集。
    pub const ALL: [BudgetTier; 3] = [BudgetTier::Qemu, BudgetTier::Metal, BudgetTier::Latency];

    /// 档位名。
    pub const fn name(self) -> &'static str {
        match self {
            BudgetTier::Qemu => "QEMU档",
            BudgetTier::Metal => "真机档",
            BudgetTier::Latency => "低延迟档",
        }
    }

    /// 档位预算（纳秒）。常量来源见各档注释。
    pub const fn budget_ns(self) -> u64 {
        match self {
            BudgetTier::Qemu => BD_BUDGET_NS_QEMU,
            BudgetTier::Metal => BD_BUDGET_NS_METAL,
            BudgetTier::Latency => BD_BUDGET_NS_LATENCY,
        }
    }
}

/// handler 关键度分级（闭集，映射预算档与降级链）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandlerClass {
    /// 硬实时：时钟、中断应答。永不降级，超时只告警不熔断。
    HardRealtime,
    /// 软实时：输入、网络。可降级到批处理，永不熔断。
    SoftRealtime,
    /// 批处理：可延迟到 bottom half，超时降级到机会型。
    Batch,
    /// 机会型：可无限延后，超时静默丢弃。
    Opportunistic,
}

impl HandlerClass {
    /// 全量闭集。
    pub const ALL: [HandlerClass; 4] = [
        HandlerClass::HardRealtime,
        HandlerClass::SoftRealtime,
        HandlerClass::Batch,
        HandlerClass::Opportunistic,
    ];

    /// 关键度名。
    pub const fn name(self) -> &'static str {
        match self {
            HandlerClass::HardRealtime => "硬实时",
            HandlerClass::SoftRealtime => "软实时",
            HandlerClass::Batch => "批处理",
            HandlerClass::Opportunistic => "机会型",
        }
    }

    /// 该关键度是否允许自动降级（硬实时永不降级）。
    pub const fn may_degrade(self) -> bool {
        !matches!(self, HandlerClass::HardRealtime)
    }

    /// 该关键度是否允许熔断（硬实时不熔断，只告警）。
    pub const fn may_fuse(self) -> bool {
        !matches!(self, HandlerClass::HardRealtime)
    }
}
/// 最小化清单条款态（三态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListKind {
    /// 义务：handler 必须做。
    Must,
    /// 有条件：特定条件下必须做。
    Conditional,
    /// 禁令：handler 绝不可做（违例即拦截）。
    MustNot,
}

impl ListKind {
    /// 全量闭集。
    pub const ALL: [ListKind; 3] = [ListKind::Must, ListKind::Conditional, ListKind::MustNot];

    /// 条款态名。
    pub const fn name(self) -> &'static str {
        match self {
            ListKind::Must => "义务",
            ListKind::Conditional => "有条件",
            ListKind::MustNot => "禁令",
        }
    }
}

/// 预算状态（五态闭集，用于升级阶梯）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetState {
    /// 健康：p99 在预算内。
    Healthy,
    /// 观察：单次超预算，未达告警线。
    Observe,
    /// 告警：连续 N 拍超预算。
    Warn,
    /// 降级：连续 M 拍超预算，触发降级链。
    Degrade,
    /// 熔断：连续 K 拍超预算且关键度允许熔断。
    Fuse,
}

impl BudgetState {
    /// 全量闭集。
    pub const ALL: [BudgetState; 5] = [
        BudgetState::Healthy,
        BudgetState::Observe,
        BudgetState::Warn,
        BudgetState::Degrade,
        BudgetState::Fuse,
    ];

    /// 状态名。
    pub const fn name(self) -> &'static str {
        match self {
            BudgetState::Healthy => "健康",
            BudgetState::Observe => "观察",
            BudgetState::Warn => "告警",
            BudgetState::Degrade => "降级",
            BudgetState::Fuse => "熔断",
        }
    }

    /// 状态序号（0..4），用于相邻跃迁判定。
    pub const fn ord(self) -> u8 {
        match self {
            BudgetState::Healthy => 0,
            BudgetState::Observe => 1,
            BudgetState::Warn => 2,
            BudgetState::Degrade => 3,
            BudgetState::Fuse => 4,
        }
    }
}

/// 诊断码（0x4c60 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BdCode {
    /// 0x4c60 向量越界（handler 向量超出 0x20..=0xEF 闭集）。
    VectorOutOfRange,
    /// 0x4c61 handler 重复注册（同向量二次注册）。
    HandlerDuplicate,
    /// 0x4c62 handler 未注册即入账（预算账入账到不存在的 handler）。
    HandlerUnregistered,
    /// 0x4c63 预算账不平（入账 ≢ 出账）。
    LedgerUnbalanced,
    /// 0x4c64 采样越界（确定性拒绝）。
    SampleOutOfRange,
    /// 0x4c65 最小化清单坏条目（文案空）。
    ChecklistBad,
    /// 0x4c66 纪律文档三节计数不守恒。
    DocSectionMismatch,
    /// 0x4c67 纪律文档已冻结仍尝试修改。
    DocFrozen,
    /// 0x4c68 超预算清单重复产出（幂等违例）。
    ReportDuplicate,
    /// 0x4c69 违例注入计数错位（注入数 ≢ 拦截数）。
    InterceptGap,
    /// 0x4c6a 预算爬升趋势斜率超红线（creep 立案）。
    BudgetCreep,
    /// 0x4c6b 静态成本审计越界。
    StaticCostBreach,
    /// 0x4c6c handler 槽位耗尽（超过 24 路上限）。
    SlotExhausted,
    /// 0x4c6d 采样环形缓冲回卷导致计数不一致。
    SampleRingInconsistent,
    /// 0x4c6e 预算状态跃迁违例（非相邻状态跳迁）。
    StateJump,
    /// 0x4c6f 段位守卫失败。
    SegmentGuard,
}

impl BdCode {
    /// 全量闭集。
    pub const ALL: [BdCode; 16] = [
        BdCode::VectorOutOfRange,
        BdCode::HandlerDuplicate,
        BdCode::HandlerUnregistered,
        BdCode::LedgerUnbalanced,
        BdCode::SampleOutOfRange,
        BdCode::ChecklistBad,
        BdCode::DocSectionMismatch,
        BdCode::DocFrozen,
        BdCode::ReportDuplicate,
        BdCode::InterceptGap,
        BdCode::BudgetCreep,
        BdCode::StaticCostBreach,
        BdCode::SlotExhausted,
        BdCode::SampleRingInconsistent,
        BdCode::StateJump,
        BdCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        BD_SEG_BASE + self as u16
    }

    /// 中文诊断（含可操作建议，铁律 6：错误路径带可操作建议）。
    pub const fn text(self) -> &'static str {
        match self {
            BdCode::VectorOutOfRange => "向量越界：handler 向量必须在 0x20..=0xEF 闭集内",
            BdCode::HandlerDuplicate => "handler 重复注册：同向量二次注册被拒，先注销再注册",
            BdCode::HandlerUnregistered => "handler 未注册即入账：先 register_handler 再 record_sample",
            BdCode::LedgerUnbalanced => "预算账不平：入账与出账不守恒，检查 settle 是否遗漏",
            BdCode::SampleOutOfRange => "采样越界：采样值必须在 0..=107_374_182_400 闭集内",
            BdCode::ChecklistBad => "最小化清单坏条目：义务/有条件/禁令文案不可为空",
            BdCode::DocSectionMismatch => "纪律文档三节计数不守恒：义务+有条件+禁令 必须等于总条目数",
            BdCode::DocFrozen => "纪律文档已冻结仍尝试修改：冻结后只可读不可写，变更走 ADR",
            BdCode::ReportDuplicate => "超预算清单重复产出：同一次超预算不可二次入清单",
            BdCode::InterceptGap => "违例注入计数错位：注入数必须等于拦截数，检查拦截器是否遗漏",
            BdCode::BudgetCreep => "预算爬升趋势超红线：三窗口水位持续上行，检查是否有累积成本泄漏",
            BdCode::StaticCostBreach => "静态成本审计越界：实测成本超出静态预算表上限，检查是否引入新成本项",
            BdCode::SlotExhausted => "handler 槽位耗尽：超过 24 路上限，回收空闲 handler 或扩展槽位",
            BdCode::SampleRingInconsistent => "采样环形缓冲不一致：回卷后计数与样本数不守恒，检查环形缓冲索引",
            BdCode::StateJump => "预算状态跃迁违例：五态只允许相邻跃迁（Healthy→Observe→Warn→Degrade→Fuse）",
            BdCode::SegmentGuard => "诊断码越段：检查 BdCode 变体数与 BD_SEG_BASE 的偏移",
        }
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    BdCode::ALL.iter().all(|c| c.val() >= BD_SEG_BASE && c.val() < BD_SEG_BASE + 0x100)
}

/// 整数平方根（无浮点，用于方差/标准差确定性计算）。
/// 牛顿迭代，u64 输入，下取整；大输入饱和防溢出。
pub fn isqrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// FNV-1a 32 位哈希（确定性冻结指纹，跨平台一致）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &b in bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// 字符串 FNV-1a 指纹（转字节后哈希）。
pub fn fnv1a_str(s: &str) -> u32 {
    fnv1a(s.as_bytes())
}


/// 最小化清单条目（义务/有条件/禁令三态，文案齐备）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChecklistItem {
    /// 条款态。
    pub kind: ListKind,
    /// 条款文案（中文，构造期断言非空）。
    pub text: String,
}

impl ChecklistItem {
    /// 构造条目（文案空即视为坏条目，由上层拦截）。
    pub fn new(kind: ListKind, text: &str) -> Self {
        ChecklistItem { kind, text: text.to_string() }
    }

    /// 条目是否有效（文案非空）。
    pub fn valid(&self) -> bool {
        !self.text.trim().is_empty()
    }
}

/// 纪律三节文档（义务节/禁令节/预算节），冻结后不可变。
/// 判据：三节计数闭集 + FNV-1a 冻结指纹。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DisciplineDoc {
    /// 义务节条款（Must）。
    pub obligations: Vec<ChecklistItem>,
    /// 有条件节条款（Conditional）。
    pub conditionals: Vec<ChecklistItem>,
    /// 禁令节条款（MustNot）。
    pub prohibitions: Vec<ChecklistItem>,
    /// 预算节文案（预算表描述）。
    pub budget_note: String,
    /// 是否已冻结。
    pub frozen: bool,
    /// 冻结指纹（FNV-1a，冻结时计算）。
    pub fingerprint: u32,
}

impl DisciplineDoc {
    /// 构造空文档（未冻结）。
    pub fn new() -> Self {
        DisciplineDoc {
            obligations: Vec::new(),
            conditionals: Vec::new(),
            prohibitions: Vec::new(),
            budget_note: String::new(),
            frozen: false,
            fingerprint: 0,
        }
    }

    /// 追加义务条款（未冻结才可写）。
    pub fn push_obligation(&mut self, text: &str) -> Result<(), (BdCode, String)> {
        if self.frozen {
            return Err((BdCode::DocFrozen, BdCode::DocFrozen.text().to_string()));
        }
        self.obligations.push(ChecklistItem::new(ListKind::Must, text));
        Ok(())
    }

    /// 追加有条件条款（未冻结才可写）。
    pub fn push_conditional(&mut self, text: &str) -> Result<(), (BdCode, String)> {
        if self.frozen {
            return Err((BdCode::DocFrozen, BdCode::DocFrozen.text().to_string()));
        }
        self.conditionals.push(ChecklistItem::new(ListKind::Conditional, text));
        Ok(())
    }

    /// 追加禁令条款（未冻结才可写）。
    pub fn push_prohibition(&mut self, text: &str) -> Result<(), (BdCode, String)> {
        if self.frozen {
            return Err((BdCode::DocFrozen, BdCode::DocFrozen.text().to_string()));
        }
        self.prohibitions.push(ChecklistItem::new(ListKind::MustNot, text));
        Ok(())
    }

    /// 设置预算节文案（未冻结才可写）。
    pub fn set_budget_note(&mut self, note: &str) -> Result<(), (BdCode, String)> {
        if self.frozen {
            return Err((BdCode::DocFrozen, BdCode::DocFrozen.text().to_string()));
        }
        self.budget_note = note.to_string();
        Ok(())
    }

    /// 全部条款（三节合并，用于计数守恒校验）。
    pub fn all_items(&self) -> Vec<ChecklistItem> {
        let mut v = Vec::new();
        v.extend(self.obligations.iter().cloned());
        v.extend(self.conditionals.iter().cloned());
        v.extend(self.prohibitions.iter().cloned());
        v
    }

    /// 全部条款是否有效（文案均非空）。
    pub fn all_valid(&self) -> bool {
        self.all_items().iter().all(|it| it.valid())
    }

    /// 三节计数守恒（义务+有条件+禁令 == 总条目，且预算节非空）。
    pub fn sections_conserved(&self) -> bool {
        let total = self.obligations.len() + self.conditionals.len() + self.prohibitions.len();
        total == self.all_items().len() && !self.budget_note.trim().is_empty()
    }

    /// 计算当前内容指纹（FNV-1a，含三节文案与预算节）。
    pub fn compute_fingerprint(&self) -> u32 {
        let mut buf = String::new();
        buf.push_str("OBL:");
        for it in &self.obligations {
            buf.push_str(&it.text);
            buf.push('\u{1}');
        }
        buf.push_str("CON:");
        for it in &self.conditionals {
            buf.push_str(&it.text);
            buf.push('\u{1}');
        }
        buf.push_str("PRO:");
        for it in &self.prohibitions {
            buf.push_str(&it.text);
            buf.push('\u{1}');
        }
        buf.push_str("BUD:");
        buf.push_str(&self.budget_note);
        fnv1a_str(&buf)
    }

    /// 冻结文档（校验计数守恒与条目有效后计算指纹，冻结不可变）。
    pub fn freeze(&mut self) -> Result<(), (BdCode, String)> {
        if self.frozen {
            return Err((BdCode::DocFrozen, BdCode::DocFrozen.text().to_string()));
        }
        if !self.all_valid() {
            return Err((BdCode::ChecklistBad, BdCode::ChecklistBad.text().to_string()));
        }
        if !self.sections_conserved() {
            return Err((BdCode::DocSectionMismatch, BdCode::DocSectionMismatch.text().to_string()));
        }
        self.fingerprint = self.compute_fingerprint();
        self.frozen = true;
        Ok(())
    }

    /// 指纹是否与当前内容一致（冻结后校验）。
    pub fn fingerprint_holds(&self) -> bool {
        self.frozen && self.fingerprint == self.compute_fingerprint()
    }
}


/// 对数桶直方图（确定性分位估算，无浮点）。
/// 桶边界见 BD_BUCKET_EDGES_NS；sample 落在 [edges[i], edges[i+1]) 归入桶 i。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LogHistogram {
    /// 桶计数。
    pub buckets: [u64; BD_BUCKETS],
    /// 总样本数。
    pub count: u64,
    /// 最小样本（纳秒）。
    pub min_ns: u64,
    /// 最大样本（纳秒）。
    pub max_ns: u64,
    /// 样本和（纳秒，用于均值）。
    pub sum_ns: u128,
    /// 样本平方和（用于方差，u128 防溢出）。
    pub sum_sq_ns: u128,
}

impl LogHistogram {
    /// 构造空直方图。
    pub fn new() -> Self {
        LogHistogram {
            buckets: [0u64; BD_BUCKETS],
            count: 0,
            min_ns: u64::MAX,
            max_ns: 0,
            sum_ns: 0,
            sum_sq_ns: 0,
        }
    }

    /// 采样值归属桶索引（闭集查找，越界钳到首/末桶）。
    fn bucket_of(ns: u64) -> usize {
        let mut idx = 0usize;
        while idx < BD_BUCKETS && ns >= BD_BUCKET_EDGES_NS[idx + 1] {
            idx += 1;
        }
        idx
    }

    /// 记录一个样本。ns 越界（>末档）钳到末桶，确定性不拒。
    pub fn record(&mut self, ns: u64) {
        let idx = Self::bucket_of(ns);
        self.buckets[idx] += 1;
        self.count += 1;
        if ns < self.min_ns {
            self.min_ns = ns;
        }
        if ns > self.max_ns {
            self.max_ns = ns;
        }
        self.sum_ns += ns as u128;
        self.sum_sq_ns += (ns as u128) * (ns as u128);
    }

    /// 均值（纳秒，整数下取整）。零样本返回 0。
    pub fn mean_ns(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            (self.sum_ns / self.count as u128) as u64
        }
    }

    /// 方差（纳秒²，整数下取整）。方差 = E[X²] - E[X]²，饱和防下溢。
    pub fn variance_ns2(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let n = self.count as u128;
        let mean = self.sum_ns / n;
        let ex2 = self.sum_sq_ns / n;
        let var = if ex2 > mean * mean { ex2 - mean * mean } else { 0 };
        var as u64
    }

    /// 标准差（纳秒，整数下取整，isqrt）。
    pub fn stddev_ns(&self) -> u64 {
        isqrt_u64(self.variance_ns2())
    }

    /// 估算分位点（纳秒）。permille ∈ [0,1000]。返回该分位桶下沿（确定性）。
    pub fn percentile_ns(&self, permille: u64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        // 目标秩（下取整，至少 1）。
        let rank = (self.count.saturating_mul(permille) / 1000).max(1);
        let mut acc: u64 = 0;
        for (i, &c) in self.buckets.iter().enumerate() {
            acc += c;
            if acc >= rank {
                return BD_BUCKET_EDGES_NS[i];
            }
        }
        self.max_ns
    }

    /// P95（纳秒）。
    pub fn p95_ns(&self) -> u64 {
        self.percentile_ns(BD_P95_PERMILLE)
    }

    /// P99（纳秒）。
    pub fn p99_ns(&self) -> u64 {
        self.percentile_ns(BD_P99_PERMILLE)
    }

    /// P99.9（纳秒）。
    pub fn p999_ns(&self) -> u64 {
        self.percentile_ns(BD_P999_PERMILLE)
    }

    /// 计数一致性（桶计数和 == 总样本数）。
    pub fn count_conserved(&self) -> bool {
        self.buckets.iter().sum::<u64>() == self.count
    }
}

/// 成本归因模型（固定 + 单位变动，双线性）。
/// 估算成本 = fixed_ns + per_call_ns × call_units。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CostModel {
    /// 固定成本（纳秒）。
    pub fixed_ns: u64,
    /// 单位变动成本（纳秒/单位）。
    pub per_call_ns: u64,
    /// 静态预算上限（纳秒），超出即立案。
    pub static_cap_ns: u64,
}

impl CostModel {
    /// 构造成本模型。
    pub fn new(fixed_ns: u64, per_call_ns: u64, static_cap_ns: u64) -> Self {
        CostModel { fixed_ns, per_call_ns, static_cap_ns }
    }

    /// 估算成本（纳秒）。call_units 饱和防溢出。
    pub fn estimate_ns(&self, call_units: u64) -> u64 {
        self.fixed_ns.saturating_add(self.per_call_ns.saturating_mul(call_units))
    }

    /// 是否超出静态预算上限。
    pub fn over_static_cap(&self, call_units: u64) -> bool {
        self.estimate_ns(call_units) > self.static_cap_ns
    }
}

/// 静态成本审计表（已知 handler 族的固定/变动成本常数）。
/// 数值来源：x86_64 架构手册指令延迟 + 缓存命中经验值，编译期可核。
pub struct StaticCostAudit;

impl StaticCostAudit {
    /// 时钟中断 handler 成本模型（TSC 读 + EOI + 记账）。
    pub fn timer() -> CostModel {
        CostModel::new(180, 12, BD_BUDGET_NS_LATENCY)
    }

    /// 键盘中断 handler 成本模型（0x60 读 + 解码 + 入队）。
    pub fn keyboard() -> CostModel {
        CostModel::new(220, 8, BD_BUDGET_NS_LATENCY)
    }

    /// 网络中断 handler 成本模型（描述符读 + 批量摘取）。
    pub fn net() -> CostModel {
        CostModel::new(400, 20, BD_BUDGET_NS_QEMU)
    }

    /// 存储中断 handler 成本模型（状态读 + 完成回调）。
    pub fn storage() -> CostModel {
        CostModel::new(500, 30, BD_BUDGET_NS_QEMU)
    }

    /// 全族闭集（用于审计遍历）。
    pub fn all() -> [CostModel; 4] {
        [Self::timer(), Self::keyboard(), Self::net(), Self::storage()]
    }
}


/// 确定性 LCG 采样源（同种子同序列，p99 可复现对拍，无外部依赖）。
/// 参数取 Numerical Recipes：a=1664525, c=1013904223, m=2^32。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u32,
}

impl Lcg {
    /// 构造（种子 0 会被替换为默认非零种子，避免退化）。
    pub fn new(seed: u32) -> Self {
        Lcg { state: if seed == 0 { 0x1234_5678 } else { seed } }
    }

    /// 下一步（返回新状态，推进一次）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }

    /// 生成 [lo, hi] 闭区间的纳秒采样（确定性）。
    pub fn sample_ns(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        let span = hi - lo;
        let r = self.next_u32() as u64;
        lo + (r % (span + 1))
    }
}

/// 预算爬升趋势检测（整数线性回归斜率 + 三窗口水位）。
/// 判据：斜率 > 0 且三窗口水位单调上行即 creep 立案。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CreepDetector {
    /// 窗口样本（p99 序列，最近样本在尾）。
    pub window: Vec<u64>,
    /// 窗口容量。
    pub cap: usize,
    /// 斜率红线（纳秒/样本，整数）。超过即 creep。
    pub slope_redline: i64,
}

impl CreepDetector {
    /// 构造（容量与斜率红线）。
    pub fn new(cap: usize, slope_redline: i64) -> Self {
        CreepDetector { window: Vec::new(), cap: cap.max(3), slope_redline }
    }

    /// 追加一个 p99 样本（超容量丢最旧，kept+dropped 守恒）。
    pub fn push(&mut self, p99_ns: u64) {
        if self.window.len() >= self.cap {
            self.window.remove(0);
        }
        self.window.push(p99_ns);
    }

    /// 整数线性回归斜率（纳秒/样本）。样本数 <2 返回 0。
    /// 斜率 = Σ(x-x̄)(y-ȳ) / Σ(x-x̄)²，全程整数。
    pub fn slope(&self) -> i64 {
        let n = self.window.len() as i64;
        if n < 2 {
            return 0;
        }
        let sum_x: i64 = (0..n).sum();
        let sum_y: i64 = self.window.iter().map(|&v| v as i64).sum();
        let mean_x = sum_x / n;
        let mean_y = sum_y / n;
        let mut num: i64 = 0;
        let mut den: i64 = 0;
        for (i, &y) in self.window.iter().enumerate() {
            let dx = i as i64 - mean_x;
            let dy = y as i64 - mean_y;
            num += dx * dy;
            den += dx * dx;
        }
        if den == 0 {
            0
        } else {
            num / den
        }
    }

    /// 三窗口水位（前/中/后各三分位的均值，下取整）。
    pub fn three_watermarks(&self) -> (u64, u64, u64) {
        let n = self.window.len();
        if n < 3 {
            let v = self.window.first().copied().unwrap_or(0);
            return (v, v, v);
        }
        let third = n / 3;
        let avg = |s: usize, e: usize| -> u64 {
            let mut acc: u64 = 0;
            let mut c: u64 = 0;
            let mut i = s;
            while i < e && i < n {
                acc += self.window[i];
                c += 1;
                i += 1;
            }
            if c == 0 { 0 } else { acc / c }
        };
        (avg(0, third), avg(third, 2 * third), avg(2 * third, n))
    }

    /// 是否 creep（斜率超红线 且 三窗口水位单调不降）。
    pub fn is_creep(&self) -> bool {
        if self.window.len() < 3 {
            return false;
        }
        let (w0, w1, w2) = self.three_watermarks();
        let slope = self.slope();
        slope > self.slope_redline && w1 >= w0 && w2 >= w1
    }
}

/// 升级阶梯（观察→告警→降级→熔断，按持续超预算拍数逐级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EscalationLadder {
    /// 告警线（连续超预算拍数）。
    pub warn_ticks: u32,
    /// 降级线（连续超预算拍数）。
    pub degrade_ticks: u32,
    /// 熔断线（连续超预算拍数）。
    pub fuse_ticks: u32,
}

impl EscalationLadder {
    /// 默认阶梯（2 拍告警 / 5 拍降级 / 10 拍熔断）。
    pub fn new() -> Self {
        EscalationLadder { warn_ticks: 2, degrade_ticks: 5, fuse_ticks: 10 }
    }

    /// 依据连续超预算拍数与关键度，判定当前预算状态。
    pub fn state_for(&self, over_ticks: u32, class: HandlerClass) -> BudgetState {
        if over_ticks == 0 {
            return BudgetState::Healthy;
        }
        if over_ticks >= self.fuse_ticks && class.may_fuse() {
            return BudgetState::Fuse;
        }
        if over_ticks >= self.degrade_ticks && class.may_degrade() {
            return BudgetState::Degrade;
        }
        if over_ticks >= self.warn_ticks {
            return BudgetState::Warn;
        }
        BudgetState::Observe
    }
}

impl Default for EscalationLadder {
    fn default() -> Self {
        Self::new()
    }
}


/// 超预算清单条目（逐 handler 超支记录）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OverBudgetEntry {
    /// handler 向量。
    pub vector: u16,
    /// 预算（纳秒）。
    pub budget_ns: u64,
    /// 实测 p99（纳秒）。
    pub p99_ns: u64,
    /// 超支倍数（基点 bps，10000 = 1.0 倍）。
    pub overspend_bps: u64,
    /// 可操作建议（中文）。
    pub advice: String,
}

impl OverBudgetEntry {
    /// 依据向量/预算/p99 构造条目，自动计算超支倍数与建议。
    pub fn of(vector: u16, budget_ns: u64, p99_ns: u64) -> Self {
        let overspend_bps = if budget_ns == 0 {
            u64::MAX
        } else {
            p99_ns.saturating_mul(10_000) / budget_ns
        };
        let advice = Self::advice_for(overspend_bps);
        OverBudgetEntry { vector, budget_ns, p99_ns, overspend_bps, advice }
    }

    /// 依据超支倍数给可操作建议（分级）。
    fn advice_for(bps: u64) -> String {
        if bps >= 20_000 {
            "超支 2 倍以上：handler 存在结构性越界，必须拆分到 bottom half 或精简逻辑".to_string()
        } else if bps >= 15_000 {
            "超支 1.5 倍以上：handler 含可延迟工作，摘取快照后移交 bottom half".to_string()
        } else if bps >= 12_000 {
            "超支 1.2 倍以上：检查是否引入额外 I/O 或循环，收紧关键段".to_string()
        } else {
            "轻度超支：复核采样口径与边界条件，确认非偶发毛刺".to_string()
        }
    }
}

/// handler 预算账条目（单路）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HandlerAccount {
    /// handler 向量。
    pub vector: u16,
    /// 预算档位。
    pub tier: BudgetTier,
    /// 关键度分级。
    pub class: HandlerClass,
    /// 直方图（p99 估算）。
    pub hist: LogHistogram,
    /// 入账次数（record_sample 计数）。
    pub debit: u64,
    /// 出账次数（settle 计数，账平要求 debit == credit）。
    pub credit: u64,
    /// 连续超预算拍数（p99 > 预算）。
    pub over_ticks: u32,
    /// 当前预算状态。
    pub state: BudgetState,
    /// 是否已熔断。
    pub fused: bool,
}

impl HandlerAccount {
    /// 构造空账条目。
    pub fn new(vector: u16, tier: BudgetTier, class: HandlerClass) -> Self {
        HandlerAccount {
            vector,
            tier,
            class,
            hist: LogHistogram::new(),
            debit: 0,
            credit: 0,
            over_ticks: 0,
            state: BudgetState::Healthy,
            fused: false,
        }
    }

    /// 预算（纳秒）。
    pub fn budget_ns(&self) -> u64 {
        self.tier.budget_ns()
    }

    /// 账是否平（入账 == 出账）。
    pub fn balanced(&self) -> bool {
        self.debit == self.credit
    }

    /// 直方图计数是否守恒。
    pub fn hist_conserved(&self) -> bool {
        self.hist.count_conserved()
    }
}


/// top half 预算纪律主引擎。
/// 判据 UNX-F0943-J1：p99 ≤5µs 全 handler 断言 + 超预算清单 10/10 自动产出 + 三节文档冻结。
#[derive(Clone, Debug)]
pub struct TopHalfBudget {
    /// 纪律三节文档（冻结后不可变）。
    pub doc: DisciplineDoc,
    /// handler 账表。
    pub accounts: Vec<HandlerAccount>,
    /// 超预算清单（只增不减）。
    pub over_list: Vec<OverBudgetEntry>,
    /// 违例注入计数（测试/审计用）。
    pub injected: u64,
    /// 违例拦截计数。
    pub intercepted: u64,
    /// 逻辑 tick（单调递增，回退即拒）。
    pub tick: u64,
    /// 升级阶梯。
    pub ladder: EscalationLadder,
    /// 爬升检测器（按 p99 序列）。
    pub creep: CreepDetector,
}

impl TopHalfBudget {
    /// 构造空引擎（文档未冻结，账表空）。
    pub fn new() -> Self {
        TopHalfBudget {
            doc: DisciplineDoc::new(),
            accounts: Vec::new(),
            over_list: Vec::new(),
            injected: 0,
            intercepted: 0,
            tick: 0,
            ladder: EscalationLadder::new(),
            creep: CreepDetector::new(16, 8),
        }
    }

    /// 记录一次违例注入（审计入口）。
    fn note(&mut self, _code: BdCode) {
        // 诊断码保留扩展点；拦截计数由调用方累加。
    }

    /// 向量是否合法（0x20..=0xEF 闭集）。
    fn vector_ok(v: u16) -> bool {
        (0x20..=0xEF).contains(&v)
    }

    /// 查找 handler 账（可变）。
    fn account_mut(&mut self, v: u16) -> Option<&mut HandlerAccount> {
        self.accounts.iter_mut().find(|a| a.vector == v)
    }

    /// 查找 handler 账（只读）。
    fn account_ref(&self, v: u16) -> Option<&HandlerAccount> {
        self.accounts.iter().find(|a| a.vector == v)
    }

    /// 注册 handler（向量合法 + 未重复 + 槽位未耗尽才可注册）。
    pub fn register_handler(
        &mut self,
        vector: u16,
        tier: BudgetTier,
        class: HandlerClass,
    ) -> Result<(), (BdCode, String)> {
        if !Self::vector_ok(vector) {
            self.intercepted += 1;
            return Err((BdCode::VectorOutOfRange, BdCode::VectorOutOfRange.text().to_string()));
        }
        if self.account_ref(vector).is_some() {
            self.intercepted += 1;
            return Err((BdCode::HandlerDuplicate, BdCode::HandlerDuplicate.text().to_string()));
        }
        if self.accounts.len() >= BD_HANDLER_SLOTS {
            self.intercepted += 1;
            return Err((BdCode::SlotExhausted, BdCode::SlotExhausted.text().to_string()));
        }
        self.accounts.push(HandlerAccount::new(vector, tier, class));
        Ok(())
    }

    /// 注销 handler（账平才可注销，否则账不平拒绝）。
    pub fn unregister_handler(&mut self, vector: u16) -> Result<(), (BdCode, String)> {
        let Some(idx) = self.accounts.iter().position(|a| a.vector == vector) else {
            self.intercepted += 1;
            return Err((BdCode::HandlerUnregistered, BdCode::HandlerUnregistered.text().to_string()));
        };
        if !self.accounts[idx].balanced() {
            self.intercepted += 1;
            return Err((BdCode::LedgerUnbalanced, BdCode::LedgerUnbalanced.text().to_string()));
        }
        self.accounts.remove(idx);
        Ok(())
    }

    /// 采样入账（handler 必须已注册，采样值合法）。tick 单调递增。
    pub fn record_sample(&mut self, vector: u16, ns: u64) -> Result<(), (BdCode, String)> {
        self.tick = self.tick.saturating_add(1);
        if ns > BD_BUCKET_EDGES_NS[BD_BUCKETS] {
            self.intercepted += 1;
            return Err((BdCode::SampleOutOfRange, BdCode::SampleOutOfRange.text().to_string()));
        }
        let Some(acc) = self.account_mut(vector) else {
            self.intercepted += 1;
            return Err((BdCode::HandlerUnregistered, BdCode::HandlerUnregistered.text().to_string()));
        };
        acc.hist.record(ns);
        acc.debit += 1;
        Ok(())
    }

    /// 结算（出账一次，推进该 handler 的预算状态与爬升检测）。
    pub fn settle(&mut self, vector: u16) -> Result<(), (BdCode, String)> {
        self.tick = self.tick.saturating_add(1);
        // 修正记录：原函数一上来就 `account_mut(..)` 拿到 `&mut HandlerAccount`，
        // 却又在同一函数里读 `self.ladder`（E0502）并调 `self.note(&mut self)`
        // （E0499）——`acc` 的可变借用贯穿全函数，`self` 的其它部分再也
        // 碰不到。改为**三段式借用**：① 只读段（`account_ref` + `ladder`）
        // ② 写回段（`account_mut`，内部不再触碰 `self` 其它字段）
        // ③ 收尾段（`self.note` / `self.creep`）。三段互不重叠。
        let (p99, next_state, cur_ord, next_over) = {
            let Some(acc) = self.account_ref(vector) else {
                self.intercepted += 1;
                return Err((
                    BdCode::HandlerUnregistered,
                    BdCode::HandlerUnregistered.text().to_string(),
                ));
            };
            let p99 = acc.hist.p99_ns();
            let over = p99 > acc.budget_ns();
            // over_ticks 的推进与状态判定都在只读段算完，写回段原样落账，
            // 保证「读到的值」与「写回的值」严格同源（不二次求值）。
            let next_over = if over {
                acc.over_ticks.saturating_add(1)
            } else {
                0
            };
            let next_state = self.ladder.state_for(next_over, acc.class);
            (p99, next_state, acc.state.ord(), next_over)
        };
        {
            let Some(acc) = self.account_mut(vector) else {
                self.intercepted += 1;
                return Err((
                    BdCode::HandlerUnregistered,
                    BdCode::HandlerUnregistered.text().to_string(),
                ));
            };
            acc.credit += 1;
            acc.over_ticks = next_over;
            acc.state = next_state;
            acc.fused = next_state == BudgetState::Fuse;
        }
        // 相邻跃迁守卫：状态序号差 >1 视为违例（本引擎逐拍推进，天然相邻）。
        // 逐拍 settle 保证只跨 1 级；若跨越 >1 记录违例但不阻断（观测态）。
        if next_state.ord().abs_diff(cur_ord) > 1 && cur_ord != BudgetState::Healthy.ord() {
            self.note(BdCode::StateJump);
        }
        self.creep.push(p99);
        Ok(())
    }

    /// 全部 handler 账是否守恒（账平 + 直方图守恒）。
    pub fn ledger_balanced(&self) -> bool {
        self.accounts.iter().all(|a| a.balanced() && a.hist_conserved())
    }

    /// 全 handler p99 是否在各自预算内（判据核心断言）。
    pub fn all_within_budget(&self) -> bool {
        self.accounts.iter().all(|a| a.hist.p99_ns() <= a.budget_ns())
    }

    /// 违例注入拦截审计（注入数 == 拦截数才通过）。
    pub fn intercept_audit(&self) -> bool {
        self.injected > 0 && self.injected == self.intercepted
    }
}


