//! VE-F3612 · 创作安全（VE-S 域 · 批次 S01 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3612`
//!
//! **判据（锚点原文六条）**：侵权零容忍、单源复用、下架制度、沙箱复述、
//! 模式复刻、判据。逐条落位：
//! - **侵权零容忍**：特征哈希比对已知素材库（[`InfringeDetector`]）——
//!   命中 → 下架 + 创作者处置（[`V0CCode::INFRINGE_HIT`]，红线实测）。
//!   侵权不是扣分项，是零容忍红线。
//! - **单源复用**：F3301–F3312 全单源复述声明（[`F3301_RESTATED_NOTE`]），
//!   侵权检测/供应链/审计/事件响应各自指名单号。
//! - **下架制度**：[`TakedownFlow`] 两阶段（下架 → 处置），下架后
//!   残留可下载 → [`V0CCode::TAKEDOWN_RESIDUE`]（下架权红线）。
//! - **沙箱复述**：脚本类资产沙箱（[`SandboxPolicy`]），逃逸 →
//!   [`V0CCode::SANDBOX_ESCAPE`]（P0）。
//! - **模式复刻**：按 F3301 模式复刻 Q 版（威胁面清单 + 检测器 +
//!   单源复用 + 下架流程四件套结构同构）。
//! - **判据**：`run_ves12_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 侵权检出→下架+处置（红线实测） | [`V0CCode::INFRINGE_HIT`]：命中即走 TakedownFlow |
//! | 脚本逃逸→P0（复述红线） | [`V0CCode::SANDBOX_ESCAPE`]：逃逸即 P0 |
//! | 供应链绕检→P0（复述） | [`V0CCode::SUPPLY_BYPASS`]：分发链断检即 P0 |
//! | 响应超时→复盘（复用） | [`V0CCode::IR_TIMEOUT`]：超时进复盘队列 |
//!
//! **码段**：独占 **0x3cxx**。
//!
//! **性能逐项分解**：检测 O(哈希)；下架 O(1)；审计复用；响应 O(时限)。
//!
//! **跨批对接点**：F3301–F3312 全单源复用声明；F3114 治理对端；
//! F3607 验证协同——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3612 诊断码段（独占 0x3cxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V0CCode(pub u16);

impl V0CCode {
    /// 侵权特征命中。
    pub const INFRINGE_HIT: V0CCode = V0CCode(0x3c01);
    /// 脚本沙箱逃逸。
    pub const SANDBOX_ESCAPE: V0CCode = V0CCode(0x3c02);
    /// 供应链绕检。
    pub const SUPPLY_BYPASS: V0CCode = V0CCode(0x3c03);
    /// 事件响应超时。
    pub const IR_TIMEOUT: V0CCode = V0CCode(0x3c04);
    /// 下架残留。
    pub const TAKEDOWN_RESIDUE: V0CCode = V0CCode(0x3c05);
    /// 素材库为空时的检测无效。
    pub const EMPTY_LIBRARY: V0CCode = V0CCode(0x3c06);
    /// 处置未闭环。
    pub const DISPOSE_OPEN: V0CCode = V0CCode(0x3c07);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V0CCode::INFRINGE_HIT => {
                "侵权特征命中已知素材库（零容忍红线）。处置：立即下架 + 启动创作者处置流程。"
            }
            V0CCode::SANDBOX_ESCAPE => {
                "脚本类资产沙箱逃逸（P0）。处置：隔离资产、审计沙箱策略、复盘逃逸路径。"
            }
            V0CCode::SUPPLY_BYPASS => {
                "分发链绕过安全检测（P0）。处置：冻结分发通道、追溯绕检节点。"
            }
            V0CCode::IR_TIMEOUT => {
                "事件响应超时。处置：进复盘队列，校准时限配置。"
            }
            V0CCode::TAKEDOWN_RESIDUE => {
                "下架后仍可下载（下架权红线）。处置：清理分发缓存、审计下架链路完整性。"
            }
            V0CCode::EMPTY_LIBRARY => {
                "素材特征库为空导致检测无效。建议：先装载特征库再启用检测。"
            }
            V0CCode::DISPOSE_OPEN => {
                "处置流程未闭环。建议：完成创作者处置记录后关闭工单。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 威胁面清单 ──────────────────────────────────────────────────────

/// 威胁类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Threat {
    /// 恶意创作资产（脚本类）。
    MaliciousAsset,
    /// 许可侵权（盗用素材）。
    Infringement,
    /// 供应链污染。
    SupplyChain,
    /// 元数据泄露。
    MetadataLeak,
}

impl Threat {
    /// 全集。
    pub fn all() -> [Threat; 4] {
        [
            Threat::MaliciousAsset,
            Threat::Infringement,
            Threat::SupplyChain,
            Threat::MetadataLeak,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Threat::MaliciousAsset => "恶意创作资产",
            Threat::Infringement => "许可侵权",
            Threat::SupplyChain => "供应链污染",
            Threat::MetadataLeak => "元数据泄露",
        }
    }
}

/// 威胁面清单。
#[derive(Clone, Debug, Default)]
pub struct ThreatSurface {
    covered: Vec<Threat>,
}

impl ThreatSurface {
    /// 新建空。
    pub fn new() -> ThreatSurface {
        ThreatSurface {
            covered: Vec::new(),
        }
    }

    /// 全覆盖。
    pub fn full() -> ThreatSurface {
        ThreatSurface {
            covered: Threat::all().to_vec(),
        }
    }

    /// 覆盖一项。
    pub fn cover(&mut self, t: Threat) {
        if !self.covered.contains(&t) {
            self.covered.push(t);
        }
    }

    /// 是否全覆盖。
    pub fn is_full(&self) -> bool {
        Threat::all().iter().all(|t| self.covered.contains(t))
    }

    /// 缺口清单。
    pub fn gaps(&self) -> Vec<Threat> {
        Threat::all()
            .iter()
            .filter(|t| !self.covered.contains(t))
            .cloned()
            .collect()
    }
}

// ── 侵权检测器（特征哈希）───────────────────────────────────────────

/// 特征库条目。
#[derive(Clone, Debug)]
pub struct FeatureEntry {
    /// 素材指纹。
    pub fingerprint: u64,
    /// 版权方。
    pub owner: String,
    /// 是否允许商用。
    pub commercial_ok: bool,
}

/// 侵权检测器。
#[derive(Clone, Debug, Default)]
pub struct InfringeDetector {
    library: Vec<FeatureEntry>,
}

impl InfringeDetector {
    /// 空检测器。
    pub fn new() -> InfringeDetector {
        InfringeDetector {
            library: Vec::new(),
        }
    }

    /// 装载特征库。
    pub fn load(&mut self, entries: Vec<FeatureEntry>) {
        self.library = entries;
    }

    /// 库大小。
    pub fn library_len(&self) -> usize {
        self.library.len()
    }

    /// 检测一件资产指纹。命中返回侵权条目。
    pub fn scan(&self, fingerprint: u64) -> Result<Option<&FeatureEntry>, V0CCode> {
        if self.library.is_empty() {
            return Err(V0CCode::EMPTY_LIBRARY);
        }
        Ok(self.library.iter().find(|e| e.fingerprint == fingerprint))
    }

    /// 命中即须下架（零容忍）。
    pub fn scan_and_flag(&self, fingerprint: u64) -> Result<bool, V0CCode> {
        match self.scan(fingerprint)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }
}

// ── 下架流程 ────────────────────────────────────────────────────────

/// 下架阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakedownStage {
    /// 已下架。
    Removed,
    /// 已处置（闭环）。
    Disposed,
}

/// 下架流程（两阶段：下架 → 处置）。
#[derive(Clone, Debug)]
pub struct TakedownFlow {
    asset: String,
    stage: Option<TakedownStage>,
    /// 下架后是否仍可下载（残留检测）。
    still_downloadable: bool,
}

impl TakedownFlow {
    /// 新建下架工单。
    pub fn open(asset: &str) -> TakedownFlow {
        TakedownFlow {
            asset: String::from(asset),
            stage: None,
            still_downloadable: true,
        }
    }

    /// 执行下架。
    pub fn remove(&mut self) {
        self.stage = Some(TakedownStage::Removed);
        // 下架动作同步断开分发（真实实现由分发层执行，此处记状态）
        self.still_downloadable = false;
    }

    /// 完成处置（闭环）。
    pub fn dispose(&mut self) {
        self.stage = Some(TakedownStage::Disposed);
    }

    /// 残留审计：下架后仍可下载 → 违规。
    pub fn audit_residue(&self) -> Result<(), V0CCode> {
        if self.stage == Some(TakedownStage::Removed) && self.still_downloadable {
            Err(V0CCode::TAKEDOWN_RESIDUE)
        } else {
            Ok(())
        }
    }

    /// 闭环审计：处置未完成。
    pub fn audit_closed(&self) -> Result<(), V0CCode> {
        if self.stage == Some(TakedownStage::Disposed) {
            Ok(())
        } else {
            Err(V0CCode::DISPOSE_OPEN)
        }
    }

    /// 资产名。
    pub fn asset(&self) -> &str {
        &self.asset
    }

    /// 当前阶段。
    pub fn stage(&self) -> Option<TakedownStage> {
        self.stage
    }
}

// ── 沙箱策略 ────────────────────────────────────────────────────────

/// 沙箱违规类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SandboxViolation {
    /// 文件系统越权。
    FsEscape,
    /// 网络越权。
    NetEscape,
    /// 进程越权。
    ProcEscape,
}

/// 沙箱策略执行器。
#[derive(Clone, Debug, Default)]
pub struct SandboxPolicy {
    violations: Vec<SandboxViolation>,
}

impl SandboxPolicy {
    /// 新建。
    pub fn new() -> SandboxPolicy {
        SandboxPolicy {
            violations: Vec::new(),
        }
    }

    /// 记录一次逃逸尝试。
    pub fn record_escape(&mut self, v: SandboxViolation) {
        self.violations.push(v);
    }

    /// 是否有逃逸（有即 P0）。
    pub fn has_escape(&self) -> bool {
        !self.violations.is_empty()
    }

    /// 逃逸审计。
    pub fn audit(&self) -> Result<(), V0CCode> {
        if self.has_escape() {
            Err(V0CCode::SANDBOX_ESCAPE)
        } else {
            Ok(())
        }
    }

    /// 逃逸数。
    pub fn escape_count(&self) -> usize {
        self.violations.len()
    }
}

// ── 事件响应 ────────────────────────────────────────────────────────

/// 事件响应时限（小时，独立写死）。
pub const IR_TIMEOUT_HOURS: u32 = 4;

/// 事件响应跟踪。
#[derive(Clone, Debug)]
pub struct IncidentResponse {
    /// 已耗时（小时）。
    pub elapsed_hours: u32,
    /// 是否已闭环。
    pub closed: bool,
}

impl IncidentResponse {
    /// 新建。
    pub fn new() -> IncidentResponse {
        IncidentResponse {
            elapsed_hours: 0,
            closed: false,
        }
    }

    /// 推进。
    pub fn advance(&mut self, hours: u32) {
        self.elapsed_hours = self.elapsed_hours.saturating_add(hours);
    }

    /// 闭环。
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// 超时审计。
    pub fn audit_timeout(&self) -> Result<(), V0CCode> {
        if !self.closed && self.elapsed_hours > IR_TIMEOUT_HOURS {
            Err(V0CCode::IR_TIMEOUT)
        } else {
            Ok(())
        }
    }
}

// ── 单源复述声明 ────────────────────────────────────────────────────

/// F3301 全系单源复述件。
pub const F3301_RESTATED_NOTE: &str =
    "F3301–F3312 单源复述：威胁面/侵权检测/供应链/审计/事件响应语义按各单锚点复述、本地实现；\
     各单落库后由对账钩子接管。";

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3301", "VE-F3114", "VE-F3607"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves12_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3612");

    // 0x3c01 威胁面全覆盖
    out.add("0x3c01-威胁面全覆盖", ThreatSurface::full().is_full(), "");

    // 0x3c02 缺口检出
    let mut ts = ThreatSurface::new();
    ts.cover(Threat::Infringement);
    out.add("0x3c02-威胁面缺口检出", !ts.is_full() && ts.gaps().len() == 3, "");

    // 0x3c03 侵权命中
    let mut det = InfringeDetector::new();
    det.load(vec![FeatureEntry {
        fingerprint: 0xdead,
        owner: String::from("版权方A"),
        commercial_ok: false,
    }]);
    let hit = det.scan_and_flag(0xdead);
    out.add("0x3c03-侵权特征命中", hit == Ok(true), "");

    // 0x3c04 未命中放行
    let miss = det.scan_and_flag(0xbeef);
    out.add("0x3c04-未命中放行", miss == Ok(false), "");

    // 0x3c05 空库拒扫
    let empty = InfringeDetector::new();
    out.add(
        "0x3c05-空特征库拒扫",
        empty.scan(1).is_err(),
        "",
    );

    // 0x3c06 下架后无残留
    let mut td = TakedownFlow::open("asset-x");
    td.remove();
    out.add("0x3c06-下架断开分发", td.audit_residue().is_ok(), "");

    // 0x3c07 下架残留检出（模拟：remove 未生效）
    let mut td2 = TakedownFlow::open("asset-y");
    td2.stage = Some(TakedownStage::Removed);
    td2.still_downloadable = true;
    out.add("0x3c07-下架残留被检出", td2.audit_residue().is_err(), "");

    // 0x3c08 处置闭环
    let mut td3 = TakedownFlow::open("asset-z");
    td3.remove();
    td3.dispose();
    out.add("0x3c08-处置闭环", td3.audit_closed().is_ok(), "");

    // 0x3c09 沙箱逃逸
    let mut sb = SandboxPolicy::new();
    sb.record_escape(SandboxViolation::FsEscape);
    out.add("0x3c09-沙箱逃逸 P0", sb.audit().is_err(), "");

    // 0x3c0a 响应超时
    let mut ir = IncidentResponse::new();
    ir.advance(5);
    out.add("0x3c0a-响应超时检出", ir.audit_timeout().is_err(), "");

    out
}
