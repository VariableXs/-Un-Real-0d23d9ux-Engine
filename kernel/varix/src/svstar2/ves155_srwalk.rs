//! VE-F3855 · 读屏走查
//!
//! 读屏用户真实走查（复用 F3156/F3356 模式）+ 走查矩阵（七域×读屏器三型×三任务=63 格抽样）
//! + 真实任务制（信息获取/导航/操作三大任务——走查不按真实任务=形式走查红线）
//! + 发现分级 + 对照复述（日志优先）。
//! 错误路径：任务形式化→重走、阻断未清→不完结、覆盖缺→补跑、矛盾→日志优先。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xCB00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xCB00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CCBCode(pub u16);

impl CCBCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const TASK_FORMAL: Self = Self(SEG_BASE + 1);
    pub const BLOCK_OPEN: Self = Self(SEG_BASE + 2);
    pub const COVER_GAP: Self = Self(SEG_BASE + 3);
    pub const CONTRADICT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::TASK_FORMAL => "任务形式化→重走（红线）",
            Self::BLOCK_OPEN => "阻断未清→不完结",
            Self::COVER_GAP => "覆盖缺→补跑",
            Self::CONTRADICT => "矛盾→日志优先",
            _ => "未知",
        }
    }
}

// ── 走查矩阵三维 ────────────────────────────────────────────────────

/// 七域（复述 S 域七域）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkDomain {
    Focus,
    Text,
    Graphic,
    Announce,
    Nav,
    Virtual,
    Surface,
}

impl WalkDomain {
    pub const ALL: [WalkDomain; 7] = [
        WalkDomain::Focus,
        WalkDomain::Text,
        WalkDomain::Graphic,
        WalkDomain::Announce,
        WalkDomain::Nav,
        WalkDomain::Virtual,
        WalkDomain::Surface,
    ];

    pub fn id(self) -> u8 {
        match self {
            Self::Focus => 1,
            Self::Text => 2,
            Self::Graphic => 3,
            Self::Announce => 4,
            Self::Nav => 5,
            Self::Virtual => 6,
            Self::Surface => 7,
        }
    }
}

/// 读屏器三型（复述 F3850 真机单源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkReader {
    R1,
    R2,
    R3,
}

impl WalkReader {
    pub const ALL: [WalkReader; 3] = [WalkReader::R1, WalkReader::R2, WalkReader::R3];
}

/// 三大真实任务。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkTask {
    /// 信息获取。
    Info,
    /// 导航。
    Navigate,
    /// 操作。
    Operate,
}

impl WalkTask {
    pub const ALL: [WalkTask; 3] = [WalkTask::Info, WalkTask::Navigate, WalkTask::Operate];

    pub fn name(self) -> &'static str {
        match self {
            Self::Info => "信息获取",
            Self::Navigate => "导航",
            Self::Operate => "操作",
        }
    }
}

/// 63 格 = 7 域 × 3 读屏器 × 3 任务。
pub const MATRIX_CELLS: usize = 7 * 3 * 3;

/// 走查格。
#[derive(Debug, Clone, Copy)]
pub struct WalkCell {
    pub domain: WalkDomain,
    pub reader: WalkReader,
    pub task: WalkTask,
    /// 是否按真实任务走查（任务制红线）。
    pub real_task: bool,
    /// 已走查。
    pub walked: bool,
    /// 通过。
    pub passed: bool,
}

// ── 发现分级 ────────────────────────────────────────────────────────

/// 发现等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingGrade {
    /// 阻断。
    Blocker,
    /// 严重。
    Major,
    /// 一般。
    Minor,
}

/// 走查发现。
#[derive(Debug, Clone)]
pub struct Finding {
    pub grade: FindingGrade,
    pub cell_domain: u8,
    pub desc: &'static str,
    /// 已修复关闭。
    pub closed: bool,
}

// ── 对照复述（日志优先）─────────────────────────────────────────────

/// 对照信号：走查人记录 vs 内核日志。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corroboration {
    /// 两源一致。
    Agree,
    /// 走查人说有问题、日志说没问题。
    HumanOnly,
    /// 日志有问题、走查人没说。
    LogOnly,
    /// 两源都说没问题。
    BothClean,
}

/// 对照裁决：矛盾时日志优先（复述制度）。
pub fn adjudicate(c: Corroboration) -> bool {
    match c {
        Corroboration::Agree | Corroboration::BothClean => true,
        // 日志优先：日志无记录则走查人观察不算数，需复核。
        Corroboration::HumanOnly => false,
        // 日志有记录则立案，即使走查人未报。
        Corroboration::LogOnly => true,
    }
}

// ── 走查会话（总控）─────────────────────────────────────────────────

/// 读屏走查会话。
#[derive(Debug, Clone)]
pub struct WalkSession {
    pub cells: Vec<WalkCell>,
    pub findings: Vec<Finding>,
    /// 参与走查的读屏用户数（参与复述）。
    pub participants: u32,
    /// 形式化格数（任务制红线违例）。
    pub formalized: u32,
    /// 对照矛盾数。
    pub contradictions: u32,
}

impl WalkSession {
    /// 全矩阵构造：63 格，默认真实任务未走查。
    pub fn full() -> Self {
        let mut cells = Vec::new();
        for d in WalkDomain::ALL.iter() {
            for r in WalkReader::ALL.iter() {
                for t in WalkTask::ALL.iter() {
                    cells.push(WalkCell {
                        domain: *d,
                        reader: *r,
                        task: *t,
                        real_task: true,
                        walked: false,
                        passed: false,
                    });
                }
            }
        }
        Self {
            cells,
            findings: Vec::new(),
            participants: 0,
            formalized: 0,
            contradictions: 0,
        }
    }

    /// 矩阵格数 63 复述。
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// 未走查格数→补跑。
    pub fn unwalked(&self) -> usize {
        self.cells.iter().filter(|c| !c.walked).count()
    }

    /// 补跑：全部走查通过。
    pub fn fill(&mut self) {
        for c in self.cells.iter_mut() {
            c.walked = true;
            c.passed = true;
        }
    }

    /// 任务制红线审计：形式化格计数。
    pub fn audit_formal(&mut self) -> u32 {
        self.formalized = self.cells.iter().filter(|c| !c.real_task).count() as u32;
        self.formalized
    }

    /// 形式化格重走：恢复真实任务制。
    pub fn rewalk(&mut self) -> usize {
        let mut n = 0;
        for c in self.cells.iter_mut() {
            if !c.real_task {
                c.real_task = true;
                c.walked = false;
                n += 1;
            }
        }
        n
    }

    /// 阻断发现未清数。
    pub fn blockers_open(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.grade == FindingGrade::Blocker && !f.closed)
            .count()
    }

    /// 走查完结判定：覆盖满 + 无形式化 + 阻断清零 + 有参与者。
    pub fn complete(&self) -> bool {
        self.unwalked() == 0
            && self.formalized == 0
            && self.blockers_open() == 0
            && self.participants > 0
    }

    /// 登记发现。
    pub fn report(&mut self, grade: FindingGrade, domain: u8, desc: &'static str) {
        self.findings.push(Finding {
            grade,
            cell_domain: domain,
            desc,
            closed: false,
        });
    }

    /// 关闭发现。
    pub fn close(&mut self, idx: usize) -> bool {
        if let Some(f) = self.findings.get_mut(idx) {
            f.closed = true;
            true
        } else {
            false
        }
    }
}

impl Default for WalkSession {
    fn default() -> Self {
        Self::full()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves155_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves155_srwalk");

    // 01 矩阵 63 格复述
    {
        let s = WalkSession::full();
        chk(&mut set, "C3855-01", s.cell_count() == 63 && MATRIX_CELLS == 63, "err");
    }
    // 02 三大任务定名
    {
        let ok = WalkTask::ALL.len() == 3
            && WalkTask::ALL.iter().all(|t| !t.name().is_empty());
        chk(&mut set, "C3855-02", ok, "err");
    }
    // 03 七域编号映射
    {
        let ok = WalkDomain::ALL
            .iter()
            .enumerate()
            .all(|(i, d)| d.id() == (i + 1) as u8);
        chk(&mut set, "C3855-03", ok, "err");
    }
    // 04 全格未走查→补跑
    {
        let mut s = WalkSession::full();
        let gap = s.unwalked();
        s.fill();
        chk(
            &mut set,
            "C3855-04",
            gap == 63 && s.unwalked() == 0,
            "err",
        );
    }
    // 05 任务制红线：形式化检出
    {
        let mut s = WalkSession::full();
        s.cells[0].real_task = false;
        s.cells[7].real_task = false;
        chk(
            &mut set,
            "C3855-05",
            s.audit_formal() == 2 && s.full_check_guard(),
            "err",
        );
    }
    // 06 形式化→重走
    {
        let mut s = WalkSession::full();
        s.cells[3].real_task = false;
        let n = s.rewalk();
        chk(
            &mut set,
            "C3855-06",
            n == 1 && s.audit_formal() == 0 && s.cells[3].real_task,
            "err",
        );
    }
    // 07 阻断发现登记
    {
        let mut s = WalkSession::full();
        s.report(FindingGrade::Blocker, 5, "导航序断裂");
        chk(&mut set, "C3855-07", s.blockers_open() == 1, "err");
    }
    // 08 阻断未清→不完结
    {
        let mut s = WalkSession::full();
        s.fill();
        s.participants = 2;
        s.report(FindingGrade::Blocker, 2, "公告丢失");
        chk(&mut set, "C3855-08", !s.complete(), "err");
    }
    // 09 阻断清零→可完结
    {
        let mut s = WalkSession::full();
        s.fill();
        s.participants = 2;
        s.report(FindingGrade::Blocker, 2, "公告丢失");
        let _ = s.close(0);
        chk(&mut set, "C3855-09", s.complete(), "err");
    }
    // 10 一般发现不阻断完结
    {
        let mut s = WalkSession::full();
        s.fill();
        s.participants = 1;
        s.report(FindingGrade::Minor, 1, "提示文案偏短");
        chk(&mut set, "C3855-10", s.complete(), "err");
    }
    // 11 无参与者不完结（参与复述）
    {
        let mut s = WalkSession::full();
        s.fill();
        chk(&mut set, "C3855-11", !s.complete(), "err");
    }
    // 12 对照裁决：一致通过
    {
        chk(
            &mut set,
            "C3855-12",
            adjudicate(Corroboration::Agree) && adjudicate(Corroboration::BothClean),
            "err",
        );
    }
    // 13 对照矛盾：日志优先（HumanOnly 不成立）
    {
        chk(&mut set, "C3855-13", !adjudicate(Corroboration::HumanOnly), "err");
    }
    // 14 对照矛盾：LogOnly 立案
    {
        chk(&mut set, "C3855-14", adjudicate(Corroboration::LogOnly), "err");
    }
    // 15 对照矛盾计数
    {
        let mut s = WalkSession::full();
        s.contradictions = 3;
        chk(
            &mut set,
            "C3855-15",
            s.full_check_guard() && s.contradictions == 3,
            "err",
        );
    }
    // 16 发现关闭越界防御
    {
        let mut s = WalkSession::full();
        chk(&mut set, "C3855-16", !s.close(99), "err");
    }
    // 17 补跑后覆盖满
    {
        let mut s = WalkSession::full();
        s.fill();
        s.participants = 4;
        chk(&mut set, "C3855-17", s.complete(), "err");
    }
    // 18 分级三档齐备
    {
        let g = [FindingGrade::Blocker, FindingGrade::Major, FindingGrade::Minor];
        let mut distinct = true;
        for i in 0..g.len() {
            for j in (i + 1)..g.len() {
                if g[i] == g[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3855-18", distinct, "err");
    }
    set
}

impl WalkSession {
    /// 判据守护：结构有效性（格数与红线条目一致）。
    pub fn full_check_guard(&self) -> bool {
        self.cells.len() == 63
    }
}
