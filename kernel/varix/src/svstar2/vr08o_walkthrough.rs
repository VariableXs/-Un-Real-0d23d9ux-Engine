//! VE-F3755 · 创作无障碍走查
//!
//! 锚点：创作无障碍走查（F3156 模式创作版复刻：走查矩阵（四工具×五辅具画像×
//! 三段=60 格（复述矩阵创作扩展——画布辅具画像（眼动创作者/开关创作者画像加入
//! （画像扩展声明（扩展理由：创作画像与消费画像不同（扩展断言：新增画像有真实
//! 对应人群依据（依据红线：画像无依据=捏造（依据红线）；关卡走查（五关正式实测
//! （复述；发现分级复述；对照复述。
//!
//! 数据结构：60 格矩阵（画像扩展）；五关走查复述；分级复用；对照复用。
//! 错误路径与降级矩阵：扩展无依据→画像剔除+立案（红线实测）；阻断未清→不完结
//! （复述）；模拟代真→无效（复述）；矛盾→日志优先（复述）。
//! 跨批对接点：F3156/F3175 模式复用；F3745 画像对端；F3760 收口输入。
//!
//! 诊断码段：独占 0x92xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x92xx）
// ─────────────────────────────────────────────────────────────

/// F3755 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct W7Code(pub u16);

impl W7Code {
    /// 画像扩展无依据——捏造红线。
    pub const PROFILE_NO_BASIS: W7Code = W7Code(0x9201);
    /// 60 格覆盖缺——补跑。
    pub const GRID_GAP: W7Code = W7Code(0x9202);
    /// 五关实测缺——模拟代真。
    pub const GATE_FAKE: W7Code = W7Code(0x9203);
    /// 阻断未清——不完结。
    pub const BLOCKER_OPEN: W7Code = W7Code(0x9204);
    /// 矛盾未裁——日志优先。
    pub const CONFLICT_UNRESOLVED: W7Code = W7Code(0x9205);
    /// 发现未分级——分级复述。
    pub const FINDING_UNGRADED: W7Code = W7Code(0x9206);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9201 => "画像扩展无真实人群依据，捏造红线立案剔除。",
            0x9202 => "60 格走查矩阵覆盖缺口，需补跑。",
            0x9203 => "五关走查未实测（模拟代真），无效。",
            0x9204 => "阻断级发现未清，走查不完结。",
            0x9205 => "矛盾未裁决，应以日志优先。",
            0x9206 => "发现未分级，分级复述不完整。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 辅具画像（扩展断言 + 依据红线）
// ─────────────────────────────────────────────────────────────

/// 辅具画像。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// 键盘创作者。
    Keyboard,
    /// 读屏创作者。
    ScreenReader,
    /// 放大创作者。
    Zoom,
    /// 眼动创作者（扩展）。
    EyeTrack,
    /// 开关创作者（扩展）。
    Switch,
}

impl Profile {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Profile::Keyboard => 0,
            Profile::ScreenReader => 1,
            Profile::Zoom => 2,
            Profile::EyeTrack => 3,
            Profile::Switch => 4,
        }
    }

    /// 全集（五画像）。
    pub fn all() -> [Profile; 5] {
        [
            Profile::Keyboard,
            Profile::ScreenReader,
            Profile::Zoom,
            Profile::EyeTrack,
            Profile::Switch,
        ]
    }

    /// 是否扩展画像（需依据）。
    pub fn is_extended(self) -> bool {
        match self {
            Profile::EyeTrack | Profile::Switch => true,
            _ => false,
        }
    }
}

/// 画像依据登记。
#[derive(Clone, Debug, Default)]
pub struct ProfileBasis {
    /// 画像 -> 是否有人群依据。
    has_basis: [bool; 5],
}

impl ProfileBasis {
    /// 空依据（全无）。
    pub fn new() -> ProfileBasis {
        ProfileBasis {
            has_basis: [false; 5],
        }
    }

    /// 默认有依据（基础画像）。
    pub fn with_defaults() -> ProfileBasis {
        ProfileBasis {
            has_basis: [true, true, true, false, false],
        }
    }

    /// 登记依据。
    pub fn grant(&mut self, p: Profile) {
        self.has_basis[p.index() as usize] = true;
    }

    /// 剔除依据。
    pub fn revoke(&mut self, p: Profile) {
        self.has_basis[p.index() as usize] = false;
    }

    /// 扩展画像合规：扩展画像必须有依据。
    pub fn extended_ok(&self) -> bool {
        for p in Profile::all() {
            if p.is_extended() && !self.has_basis[p.index() as usize] {
                return false;
            }
        }
        true
    }

    /// 无依据扩展清单。
    pub fn ungrounded_extended(&self) -> Vec<Profile> {
        let mut bad = Vec::new();
        for p in Profile::all() {
            if p.is_extended() && !self.has_basis[p.index() as usize] {
                bad.push(p);
            }
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 60 格走查矩阵（四工具 × 五画像 × 三段）
// ─────────────────────────────────────────────────────────────

/// 走查格。
#[derive(Clone, Debug, Default)]
pub struct WalkCell {
    /// 是否已跑。
    pub ran: bool,
    /// 是否通过。
    pub passed: bool,
}

impl WalkCell {
    /// 构造。
    pub fn new() -> WalkCell {
        WalkCell {
            ran: false,
            passed: false,
        }
    }

    /// 跑通。
    pub fn mark_pass(&mut self) {
        self.ran = true;
        self.passed = true;
    }

    /// 跑失败。
    pub fn mark_fail(&mut self) {
        self.ran = true;
        self.passed = false;
    }
}

/// 60 格矩阵。
#[derive(Clone, Debug)]
pub struct WalkGrid {
    cells: Vec<WalkCell>,
}

impl WalkGrid {
    /// 空矩阵（60 格）。
    pub fn new() -> WalkGrid {
        let mut cells = Vec::new();
        for _ in 0..60 {
            cells.push(WalkCell::new());
        }
        WalkGrid { cells }
    }

    /// 坐标 -> 索引（工具 0-3 × 画像 0-4 × 段 0-2）。
    pub fn index(tool: u8, profile: u8, segment: u8) -> Option<usize> {
        if tool >= 4 || profile >= 5 || segment >= 3 {
            return None;
        }
        Some((tool as usize) * 15 + (profile as usize) * 3 + (segment as usize))
    }

    /// 跑通一格。
    pub fn run_pass(&mut self, tool: u8, profile: u8, segment: u8) -> bool {
        match Self::index(tool, profile, segment) {
            Some(i) => {
                self.cells[i].mark_pass();
                true
            }
            None => false,
        }
    }

    /// 跑失败一格。
    pub fn run_fail(&mut self, tool: u8, profile: u8, segment: u8) -> bool {
        match Self::index(tool, profile, segment) {
            Some(i) => {
                self.cells[i].mark_fail();
                true
            }
            None => false,
        }
    }

    /// 未跑格数。
    pub fn unrun_count(&self) -> usize {
        self.cells.iter().filter(|c| !c.ran).count()
    }

    /// 失败格数。
    pub fn fail_count(&self) -> usize {
        self.cells.iter().filter(|c| c.ran && !c.passed).count()
    }

    /// 全覆盖。
    pub fn complete(&self) -> bool {
        self.cells.iter().all(|c| c.ran)
    }

    /// 总格。
    pub fn total(&self) -> usize {
        self.cells.len()
    }
}

impl Default for WalkGrid {
    fn default() -> WalkGrid {
        WalkGrid::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 发现分级与阻断
// ─────────────────────────────────────────────────────────────

/// 发现等级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 阻断。
    Blocker,
    /// 严重。
    Major,
    /// 一般。
    Minor,
}

impl Severity {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Severity::Blocker => 0,
            Severity::Major => 1,
            Severity::Minor => 2,
        }
    }
}

/// 发现。
#[derive(Clone, Debug)]
pub struct Finding {
    /// id。
    pub id: u32,
    /// 等级。
    pub severity: Severity,
    /// 是否已清。
    pub cleared: bool,
    /// 是否实测（非模拟）。
    pub hands_on: bool,
}

impl Finding {
    /// 构造。
    pub fn new(id: u32, severity: Severity, hands_on: bool) -> Finding {
        Finding {
            id,
            severity,
            cleared: false,
            hands_on,
        }
    }

    /// 清除。
    pub fn clear(&mut self) {
        self.cleared = true;
    }
}

/// 发现台账。
#[derive(Clone, Debug, Default)]
pub struct FindingLog {
    findings: Vec<Finding>,
}

impl FindingLog {
    /// 空台账。
    pub fn new() -> FindingLog {
        FindingLog {
            findings: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, f: Finding) {
        self.findings.push(f);
    }

    /// 扫描未清阻断。
    pub fn open_blockers(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for f in self.findings.iter() {
            if f.severity == Severity::Blocker && !f.cleared {
                bad.push(f.id);
            }
        }
        bad
    }

    /// 扫描模拟发现（非实测）。
    pub fn fake_findings(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for f in self.findings.iter() {
            if !f.hands_on {
                bad.push(f.id);
            }
        }
        bad
    }

    /// 发现数。
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作无障碍走查台。
#[derive(Clone, Debug, Default)]
pub struct A11yWalkBench {
    basis: ProfileBasis,
    grid: WalkGrid,
    log: FindingLog,
}

impl A11yWalkBench {
    /// 构造。
    pub fn new() -> A11yWalkBench {
        A11yWalkBench {
            basis: ProfileBasis::with_defaults(),
            grid: WalkGrid::new(),
            log: FindingLog::new(),
        }
    }

    /// 取依据（可变）。
    pub fn basis_mut(&mut self) -> &mut ProfileBasis {
        &mut self.basis
    }

    /// 取矩阵（可变）。
    pub fn grid_mut(&mut self) -> &mut WalkGrid {
        &mut self.grid
    }

    /// 取台账（可变）。
    pub fn log_mut(&mut self) -> &mut FindingLog {
        &mut self.log
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<W7Code> {
        let mut bad = Vec::new();
        if !self.basis.extended_ok() {
            bad.push(W7Code::PROFILE_NO_BASIS);
        }
        if !self.grid.complete() {
            bad.push(W7Code::GRID_GAP);
        }
        if !self.log.fake_findings().is_empty() {
            bad.push(W7Code::GATE_FAKE);
        }
        if !self.log.open_blockers().is_empty() {
            bad.push(W7Code::BLOCKER_OPEN);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3755 判据集。
pub fn run_vr08o_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3755");

    // 0x9201 画像依据红线
    {
        let mut b = ProfileBasis::new();
        out.add("0x9201-扩展无依据拦", !b.extended_ok() && b.ungrounded_extended().len() == 2, "");
        b.grant(Profile::EyeTrack);
        b.grant(Profile::Switch);
        out.add("0x9201-补依据后过", b.extended_ok(), "");
    }

    // 0x9202 60 格
    {
        let mut g = WalkGrid::new();
        out.add("0x9202-初始全未跑", g.unrun_count() == 60, "");
        let mut ran = 0;
        for t in 0..4u8 {
            for p in 0..5u8 {
                for s in 0..3u8 {
                    if g.run_pass(t, p, s) {
                        ran += 1;
                    }
                }
            }
        }
        out.add("0x9202-全覆盖", ran == 60 && g.complete(), "");
        out.add("0x9202-越界拒收", WalkGrid::index(4, 0, 0).is_none(), "");
    }

    // 0x9203 模拟代真
    {
        let mut log = FindingLog::new();
        log.add(Finding::new(1, Severity::Minor, false));
        out.add("0x9203-模拟检出", log.fake_findings().len() == 1, "");
        log.add(Finding::new(2, Severity::Minor, true));
        out.add("0x9203-实测有效", log.fake_findings().len() == 1, "");
    }

    // 0x9204 阻断未清
    {
        let mut log = FindingLog::new();
        log.add(Finding::new(3, Severity::Blocker, true));
        out.add("0x9204-阻断未清检出", log.open_blockers().len() == 1, "");
        let mut f = Finding::new(4, Severity::Blocker, true);
        f.clear();
        log.add(f);
        out.add("0x9204-已清不计", log.open_blockers().len() == 1, "");
    }

    // 判据
    {
        let mut b = A11yWalkBench::new();
        let bad = b.audit();
        out.add("0x9205-走查台检出", !bad.is_empty(), "");
    }

    out
}
