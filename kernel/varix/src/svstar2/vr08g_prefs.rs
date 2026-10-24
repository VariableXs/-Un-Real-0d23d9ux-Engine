//! VE-F3747 · 创作偏好体系
//!
//! 锚点：创作偏好体系（F3146 模式创作版复刻：创作偏好项（画布默认/工具布局/
//! 快捷键方案/辅助强度——偏好模型复用（复述单源；创作无障碍偏好（辅助强度可调
//! （提示频率/自动化程度——辅助强度红线：辅助强度用户定（系统不强制辅助
//! （强制辅助=主权（强制禁令；偏好与无障碍联动（复用禁令矩阵（复述顶层；
//! 创作偏好同步（复用同步范围（复述默认最小。
//!
//! 数据结构：偏好模型复用；强度可调（禁令）；禁令矩阵复用；同步复用。
//! 错误路径与降级矩阵：强制辅助检出→P1（红线实测——审计扫描）；偏好项无登记→
//! 不可发布（复述硬门）；同步默认全→修正（复述红线）；顶层失效→修正（复用）。
//! 跨批对接点：F3146/F3111 单源复用声明；R01-R03 工具对端；F3672 隐私联动。
//!
//! 诊断码段：独占 0x8Axx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x8Axx）
// ─────────────────────────────────────────────────────────────

/// F3747 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P7Code(pub u16);

impl P7Code {
    /// 强制辅助检出——主权禁令 P1。
    pub const FORCE_ASSIST: P7Code = P7Code(0x8A01);
    /// 偏好项未登记——发布硬门。
    pub const PREF_UNREGISTERED: P7Code = P7Code(0x8A02);
    /// 同步范围默认全——默认最小红线。
    pub const SYNC_ALL_DEFAULT: P7Code = P7Code(0x8A03);
    /// 顶层禁令矩阵失效——复用失效。
    pub const MATRIX_BROKEN: P7Code = P7Code(0x8A04);
    /// 强度越界——可调范围断言。
    pub const STRENGTH_OOB: P7Code = P7Code(0x8A05);
    /// 单源漂移——对账。
    pub const SOURCE_DRIFT: P7Code = P7Code(0x8A06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8A01 => "检出强制辅助，违反辅助强度主权禁令 P1。",
            0x8A02 => "偏好项未登记即发布，硬门拦截。",
            0x8A03 => "同步默认全量，违反默认最小红线。",
            0x8A04 => "禁令矩阵顶层失效，复用机制不完整。",
            0x8A05 => "辅助强度越界，可调范围断言不通过。",
            0x8A06 => "偏好模型与单源声明漂移，需对账。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 偏好模型（复用 F3146 单源）
// ─────────────────────────────────────────────────────────────

/// 偏好项类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefKind {
    /// 画布默认。
    CanvasDefault,
    /// 工具布局。
    ToolLayout,
    /// 快捷键方案。
    Keymap,
    /// 辅助强度。
    AssistStrength,
}

impl PrefKind {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            PrefKind::CanvasDefault => 0,
            PrefKind::ToolLayout => 1,
            PrefKind::Keymap => 2,
            PrefKind::AssistStrength => 3,
        }
    }

    /// 全集。
    pub fn all() -> [PrefKind; 4] {
        [
            PrefKind::CanvasDefault,
            PrefKind::ToolLayout,
            PrefKind::Keymap,
            PrefKind::AssistStrength,
        ]
    }
}

/// 单条偏好。
#[derive(Clone, Debug)]
pub struct PrefEntry {
    /// 类别。
    pub kind: PrefKind,
    /// 是否已登记（发布硬门）。
    pub registered: bool,
    /// 是否参与同步（默认最小=否）。
    pub synced: bool,
    /// 数值载荷（0-255）。
    pub value: u8,
}

impl PrefEntry {
    /// 构造（默认不同步）。
    pub fn new(kind: PrefKind) -> PrefEntry {
        PrefEntry {
            kind,
            registered: false,
            synced: false,
            value: 0,
        }
    }

    /// 可发布：已登记。
    pub fn publishable(&self) -> bool {
        self.registered
    }
}

/// 偏好集。
#[derive(Clone, Debug, Default)]
pub struct PrefModel {
    entries: Vec<PrefEntry>,
    source_tag: u64,
}

impl PrefModel {
    /// 空集。
    pub fn new() -> PrefModel {
        PrefModel {
            entries: Vec::new(),
            source_tag: 0,
        }
    }

    /// 带单源。
    pub fn with_source(tag: u64) -> PrefModel {
        PrefModel {
            entries: Vec::new(),
            source_tag: tag,
        }
    }

    /// 登记并入表。
    pub fn register(&mut self, e: PrefEntry) -> bool {
        for x in self.entries.iter() {
            if x.kind == e.kind {
                return false;
            }
        }
        let mut e2 = e;
        e2.registered = true;
        self.entries.push(e2);
        true
    }

    /// 取条目（可变）。
    pub fn get_mut(&mut self, kind: PrefKind) -> Option<&mut PrefEntry> {
        self.entries.iter_mut().find(|x| x.kind == kind)
    }

    /// 取条目（只读）。
    pub fn get(&self, kind: PrefKind) -> Option<&PrefEntry> {
        self.entries.iter().find(|x| x.kind == kind)
    }

    /// 扫描未登记。
    pub fn scan_unregistered(&self) -> Vec<PrefKind> {
        // 内部登记入口已强制 registered=true，此处兜底外部注入路径。
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.registered {
                bad.push(e.kind);
            }
        }
        bad
    }

    /// 扫描默认全同步（默认最小红线）。
    pub fn scan_sync_all(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.synced)
    }

    /// 单源标识。
    pub fn source_tag(&self) -> u64 {
        self.source_tag
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 辅助强度（可调 + 主权禁令）
// ─────────────────────────────────────────────────────────────

/// 辅助强度。
#[derive(Clone, Debug)]
pub struct AssistStrength {
    /// 提示频率（0-10）。
    pub hint_freq: u8,
    /// 自动化程度（0-10）。
    pub automation: u8,
    /// 是否系统强制（禁令：必须恒 false）。
    pub system_forced: bool,
}

impl AssistStrength {
    /// 构造（用户可控初值 5/5）。
    pub fn new() -> AssistStrength {
        AssistStrength {
            hint_freq: 5,
            automation: 5,
            system_forced: false,
        }
    }

    /// 范围断言。
    pub fn in_range(&self) -> bool {
        self.hint_freq <= 10 && self.automation <= 10
    }

    /// 主权禁令合规：非强制 + 在范围。
    pub fn sovereign_ok(&self) -> bool {
        !self.system_forced && self.in_range()
    }

    /// 演练注入强制。
    pub fn inject_force(&mut self) {
        self.system_forced = true;
    }

    /// 修正：清除强制。
    pub fn fix_force(&mut self) {
        self.system_forced = false;
    }

    /// 设置频率（越界拒绝）。
    pub fn set_hint_freq(&mut self, v: u8) -> bool {
        if v > 10 {
            return false;
        }
        self.hint_freq = v;
        true
    }

    /// 设置自动化（越界拒绝）。
    pub fn set_automation(&mut self, v: u8) -> bool {
        if v > 10 {
            return false;
        }
        self.automation = v;
        true
    }
}

impl Default for AssistStrength {
    fn default() -> AssistStrength {
        AssistStrength::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 禁令矩阵（复用顶层）
// ─────────────────────────────────────────────────────────────

/// 禁令矩阵：强制辅助 / 产出豁免 / 同步全量 / 未登记发布 四禁。
#[derive(Clone, Debug, Default)]
pub struct BanMatrix {
    /// 顶层开关（复用断言：必须 true）。
    pub top_level: bool,
    /// 强制辅助禁令。
    pub ban_force_assist: bool,
    /// 产出豁免禁令。
    pub ban_output_exempt: bool,
    /// 同步全量禁令。
    pub ban_sync_all: bool,
    /// 未登记发布禁令。
    pub ban_unreg_publish: bool,
}

impl BanMatrix {
    /// 全开（合规态）。
    pub fn full() -> BanMatrix {
        BanMatrix {
            top_level: true,
            ban_force_assist: true,
            ban_output_exempt: true,
            ban_sync_all: true,
            ban_unreg_publish: true,
        }
    }

    /// 顶层有效：所有禁令开。
    pub fn effective(&self) -> bool {
        self.top_level
            && self.ban_force_assist
            && self.ban_output_exempt
            && self.ban_sync_all
            && self.ban_unreg_publish
    }

    /// 演练：打断顶层。
    pub fn inject_break(&mut self) {
        self.top_level = false;
    }

    /// 修正。
    pub fn fix(&mut self) {
        *self = BanMatrix::full();
    }
}

// ─────────────────────────────────────────────────────────────
// 同步范围（默认最小）
// ─────────────────────────────────────────────────────────────

/// 同步范围。
#[derive(Clone, Debug, Default)]
pub struct SyncScope {
    /// 同步偏好数。
    pub enabled: u32,
    /// 总偏好数。
    pub total: u32,
}

impl SyncScope {
    /// 构造。
    pub fn new(total: u32) -> SyncScope {
        SyncScope { enabled: 0, total }
    }

    /// 默认最小合规：未显式开启即 0。
    pub fn minimal_ok(&self) -> bool {
        self.enabled < self.total || self.total == 0
    }

    /// 全量判断。
    pub fn is_all(&self) -> bool {
        self.total > 0 && self.enabled >= self.total
    }

    /// 开启一项。
    pub fn enable_one(&mut self) -> bool {
        if self.enabled >= self.total {
            return false;
        }
        self.enabled += 1;
        true
    }

    /// 修正为最小。
    pub fn fix_minimal(&mut self) {
        self.enabled = 0;
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作偏好审查器。
#[derive(Clone, Debug, Default)]
pub struct PrefAuditor {
    model: PrefModel,
    strength: AssistStrength,
    matrix: BanMatrix,
    scope: SyncScope,
}

impl PrefAuditor {
    /// 构造。
    pub fn new() -> PrefAuditor {
        PrefAuditor {
            model: PrefModel::new(),
            strength: AssistStrength::new(),
            matrix: BanMatrix::full(),
            scope: SyncScope::new(0),
        }
    }

    /// 取模型（可变）。
    pub fn model_mut(&mut self) -> &mut PrefModel {
        &mut self.model
    }

    /// 取强度（可变）。
    pub fn strength_mut(&mut self) -> &mut AssistStrength {
        &mut self.strength
    }

    /// 取矩阵（可变）。
    pub fn matrix_mut(&mut self) -> &mut BanMatrix {
        &mut self.matrix
    }

    /// 取同步（可变）。
    pub fn scope_mut(&mut self) -> &mut SyncScope {
        &mut self.scope
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<P7Code> {
        let mut bad = Vec::new();
        if self.strength.system_forced {
            bad.push(P7Code::FORCE_ASSIST);
        }
        if !self.model.scan_unregistered().is_empty() {
            bad.push(P7Code::PREF_UNREGISTERED);
        }
        if self.scope.is_all() {
            bad.push(P7Code::SYNC_ALL_DEFAULT);
        }
        if !self.matrix.effective() {
            bad.push(P7Code::MATRIX_BROKEN);
        }
        if !self.strength.in_range() {
            bad.push(P7Code::STRENGTH_OOB);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3747 判据集。
pub fn run_vr08g_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3747");

    // 0x8A01 强制辅助禁令
    {
        let mut s = AssistStrength::new();
        out.add("0x8A01-默认非强制", s.sovereign_ok(), "");
        s.inject_force();
        out.add("0x8A01-强制检出", !s.sovereign_ok(), "");
        s.fix_force();
        out.add("0x8A01-修正后过", s.sovereign_ok(), "");
    }

    // 0x8A02 发布硬门
    {
        let mut m = PrefModel::new();
        out.add("0x8A02-空集无未登记", m.scan_unregistered().is_empty(), "");
        m.register(PrefEntry::new(PrefKind::Keymap));
        let published = match m.get(PrefKind::Keymap) {
            Some(e) => e.publishable(),
            None => false,
        };
        out.add("0x8A02-登记后可发布", published, "");
    }

    // 0x8A03 默认最小
    {
        let mut sc = SyncScope::new(4);
        out.add("0x8A03-初始最小", sc.minimal_ok() && !sc.is_all(), "");
        sc.enable_one();
        sc.enable_one();
        sc.enable_one();
        sc.enable_one();
        out.add("0x8A03-全量检出", sc.is_all(), "");
        sc.fix_minimal();
        out.add("0x8A03-修正后过", sc.minimal_ok(), "");
    }

    // 0x8A04 矩阵顶层
    {
        let mut b = BanMatrix::full();
        out.add("0x8A04-全开有效", b.effective(), "");
        b.inject_break();
        out.add("0x8A04-打断检出", !b.effective(), "");
        b.fix();
        out.add("0x8A04-修正后过", b.effective(), "");
    }

    // 0x8A05 强度范围
    {
        let mut s = AssistStrength::new();
        out.add("0x8A05-越界拒收", !s.set_hint_freq(11) && s.hint_freq == 5, "");
        out.add("0x8A05-合法设值", s.set_automation(8) && s.automation == 8, "");
    }

    // 判据
    {
        let mut a = PrefAuditor::new();
        a.model_mut().register(PrefEntry::new(PrefKind::CanvasDefault));
        out.add("0x8A06-初始全过", a.audit().is_empty(), "");
        a.strength_mut().inject_force();
        let bad = a.audit();
        out.add("0x8A01-总审检出", bad.iter().any(|c| c.raw() == 0x8A01), "");
    }

    out
}
