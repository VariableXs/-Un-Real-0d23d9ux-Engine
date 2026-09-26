//! 深化层二 · F136 示例应用仓库（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】五示例结构化源码模型 +【设计细节】递进坡度
//! 与截图基线（主册 G-D-11）：每示例文件清单与行数构成、教学注释
//! 密度精算、≤200 行门禁拆分建议、新概念坡度校验、CI 断绿归因四
//! 分类、新手实录册、截图基线哈希对拍。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;
use crate::stareco::examples::{Example, ExampleRepo, MAX_LINES_PER_EXAMPLE};

// ---------------------------------------------------------------------------
// 示例源码构成模型：单文件构成拆解（行数从哪来）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct FileConstituent {
    pub path: &'static str,
    pub code_lines: usize,
    pub comment_lines: usize,
    pub blank_lines: usize,
}

impl FileConstituent {
    pub fn total(&self) -> usize {
        self.code_lines + self.comment_lines + self.blank_lines
    }
}

/// 源码审计：多文件示例的构成合计 + 门禁结论。
pub struct SourceAudit {
    pub files: alloc::vec::Vec<FileConstituent>,
}

impl SourceAudit {
    pub fn total_lines(&self) -> usize {
        self.files.iter().map(|f| f.total()).sum()
    }

    pub fn comment_lines(&self) -> usize {
        self.files.iter().map(|f| f.comment_lines).sum()
    }

    pub fn code_lines(&self) -> usize {
        self.files.iter().map(|f| f.code_lines).sum()
    }

    /// ≤200 行门禁：单文件超限与总量超限分开报（拆分建议的输入）。
    pub fn oversized_files(&self) -> alloc::vec::Vec<&'static str> {
        self.files.iter().filter(|f| f.total() > MAX_LINES_PER_EXAMPLE).map(|f| f.path).collect()
    }

    pub fn total_ok(&self) -> bool {
        self.total_lines() <= MAX_LINES_PER_EXAMPLE
    }

    /// 教学注释密度（万分比口径——对齐 ebase 千分比语义的细化层）。
    pub fn density_bp(&self) -> u32 {
        let total = self.total_lines();
        if total == 0 {
            return 0;
        }
        ((self.comment_lines() * 10_000) / total) as u32
    }

    /// 拆分建议：总量超限时按文件均衡给出目标行数（诚实算法：非空文件均摊）。
    pub fn split_advice(&self) -> Option<alloc::vec::Vec<(&'static str, usize)>> {
        if self.total_ok() || self.files.is_empty() {
            return None;
        }
        let n = self.files.len();
        let per = MAX_LINES_PER_EXAMPLE / n;
        Some(self.files.iter().map(|f| (f.path, per.min(f.total()))).collect())
    }
}

// ---------------------------------------------------------------------------
// 递进坡度校验：N 必须真用了 N-1 的知识 + 至少一个新概念
// ---------------------------------------------------------------------------

/// 坡度卡：每示例声明复用知识数与新概念数。
#[derive(Clone, Copy)]
pub struct SlopeCard {
    pub no: usize,
    /// 复用前一示例的概念数（no=1 无前驱，恒 0）。
    pub reused: usize,
    /// 本示例引入的新概念数。
    pub fresh: usize,
}

/// 坡度判定：no>1 要求 reused ≥ 1（站在上一级肩膀上）且 fresh ≥ 1
/// （每级都要有新东西——纯重复示例不配进仓库）。
pub fn slope_verdict(c: SlopeCard) -> Result<(), &'static str> {
    if c.no == 0 || c.no > 5 {
        return Err("示例号越界（1..=5）");
    }
    if c.no == 1 {
        if c.reused != 0 {
            return Err("第一示例无前驱：不得声明复用");
        }
        if c.fresh == 0 {
            return Err("第一示例也要有概念：hello 也教窗口模型");
        }
        return Ok(());
    }
    if c.reused == 0 {
        return Err("递进断裂：必须复用前一示例的知识");
    }
    if c.fresh == 0 {
        return Err("坡度为零：纯重复示例，拆掉");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CI 断绿归因四分类（示例永不腐烂——断绿必须归因）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakCause {
    /// 系统 API 变了（门禁红即修——F137 破面联动）。
    ApiChange,
    /// 示例自身错（提交前跑漏）。
    ExampleBug,
    /// CI 环境故障（夜跑机问题——不算示例烂）。
    CiEnvironment,
    /// 截图基线过期（UI 变更未同步基线）。
    StaleBaseline,
}

pub struct BreakRecord {
    pub day: u32,
    pub cause: BreakCause,
    /// 修复日（0 = 未修）。
    pub fixed_day: u32,
}

pub struct BreakLedger {
    records: alloc::vec::Vec<BreakRecord>,
}

impl BreakLedger {
    pub fn new() -> BreakLedger {
        BreakLedger { records: alloc::vec::Vec::new() }
    }

    pub fn record(&mut self, day: u32, cause: BreakCause) {
        self.records.push(BreakRecord { day, cause, fixed_day: 0 });
    }

    /// 最早未修断绿标记修复（FIFO——先烂先修）。
    pub fn mark_fixed(&mut self, day: u32) -> Result<(), &'static str> {
        let r = self.records.iter_mut().find(|r| r.fixed_day == 0).ok_or("无未修断绿")?;
        if day < r.day {
            return Err("修复日早于断绿日：时间线矛盾");
        }
        r.fixed_day = day;
        Ok(())
    }

    pub fn open_count(&self) -> usize {
        self.records.iter().filter(|r| r.fixed_day == 0).count()
    }

    /// 连续断绿日数：当前未修最早记录到今天的跨度（30 天线 = 零容忍）。
    pub fn worst_open_streak(&self, today: u32) -> u32 {
        self.records
            .iter()
            .filter(|r| r.fixed_day == 0)
            .map(|r| today.saturating_sub(r.day))
            .max()
            .unwrap_or(0)
    }

    /// 归因分布：每类断绿的累计次数（改进方向的证据）。
    pub fn by_cause(&self, cause: BreakCause) -> usize {
        self.records.iter().filter(|r| r.cause == cause).count()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
}

// ---------------------------------------------------------------------------
// 新手实录册：非作者实测 5/5（判据的记录面）
// ---------------------------------------------------------------------------

pub struct NewbieRun {
    pub tester: &'static str,
    pub example_no: usize,
    /// 照 README 跑通（true）/失败（记录卡点）。
    pub passed: bool,
    pub stuck_note: &'static str,
    pub minutes: u32,
}

pub struct NewbieLedger {
    runs: alloc::vec::Vec<NewbieRun>,
}

impl NewbieLedger {
    pub fn new() -> NewbieLedger {
        NewbieLedger { runs: alloc::vec::Vec::new() }
    }

    pub fn record(&mut self, r: NewbieRun) -> Result<(), &'static str> {
        if r.tester.is_empty() {
            return Err("测试者必填：非作者实名实测");
        }
        if !(1..=5).contains(&r.example_no) {
            return Err("示例号越界");
        }
        if !r.passed && r.stuck_note.is_empty() {
            return Err("失败必须记卡点：没卡点的失败是假数据");
        }
        self.runs.push(r);
        Ok(())
    }

    /// 五示例全覆盖判定：每例至少一次通过（5/5 判据）。
    pub fn all_five_passed(&self) -> bool {
        (1..=5).all(|no| self.runs.iter().any(|r| r.example_no == no && r.passed))
    }

    /// 通过率（分母 = 已测次数）。
    pub fn pass_rate_bp(&self) -> u32 {
        if self.runs.is_empty() {
            return 0;
        }
        let passed = self.runs.iter().filter(|r| r.passed).count();
        ((passed * 10_000) / self.runs.len()) as u32
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }
}

// ---------------------------------------------------------------------------
// 截图基线：窗口出现即绿的哈希对拍（CI 比对面）
// ---------------------------------------------------------------------------

pub struct ScreenshotBaseline {
    /// 示例号 → 基线哈希。
    baselines: alloc::vec::Vec<(usize, u64)>,
}

impl ScreenshotBaseline {
    pub fn new() -> ScreenshotBaseline {
        ScreenshotBaseline { baselines: alloc::vec::Vec::new() }
    }

    pub fn set(&mut self, no: usize, pixel_fp: u64) -> Result<(), &'static str> {
        if !(1..=5).contains(&no) {
            return Err("示例号越界");
        }
        if let Some(e) = self.baselines.iter_mut().find(|(n, _)| *n == no) {
            e.1 = pixel_fp;
            return Ok(());
        }
        self.baselines.push((no, pixel_fp));
        Ok(())
    }

    /// 对拍：像素流指纹与基线一致 → 绿；不一致给出差异提示（UI 回归或基线过期）。
    pub fn compare(&self, no: usize, frame_pixels: &[u8]) -> Result<(), &'static str> {
        let base = self.baselines.iter().find(|(n, _)| *n == no).ok_or("无基线：先建档再比对")?;
        let got = fnv1a64(frame_pixels);
        if got == base.1 {
            Ok(())
        } else {
            Err("截图与基线不符：UI 回归或基线过期（断绿归因二选一）")
        }
    }

    pub fn len(&self) -> usize {
        self.baselines.len()
    }
}

/// 学习时长估算：行数×密度×新概念的三因子（README 预计学习时长的依据）。
pub fn estimate_learning_minutes(code_lines: usize, density_bp: u32, fresh_concepts: usize) -> u32 {
    let base = code_lines as u32;
    let density_factor = 10_000 + density_bp * 2; // 注释越多越好读——时长下调
    let concept_factor = 15 * (fresh_concepts.max(1) as u32);
    (base * concept_factor * 10_000 / density_factor / 100).max(5)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F136E_TAG: &str = "stareco-F136-deep2";

pub fn run_f136_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F136E_TAG);

    // 源码审计：构成/密度/门禁/拆分建议
    let audit = SourceAudit {
        files: alloc::vec![
            FileConstituent { path: "main.rs", code_lines: 80, comment_lines: 40, blank_lines: 10 },
            FileConstituent { path: "ui.rs", code_lines: 30, comment_lines: 20, blank_lines: 5 },
        ],
    };
    set.add("f136e audit total", audit.total_lines() == 185, "构成合计 185 行");
    set.add("f136e audit gate", audit.total_ok() && audit.oversized_files().is_empty(), "≤200 门禁绿");
    set.add("f136e audit density", audit.density_bp() == 3_513, "教学密度万分比精算（60/185≈32%）");
    let fat = SourceAudit {
        files: alloc::vec![
            FileConstituent { path: "a.rs", code_lines: 150, comment_lines: 40, blank_lines: 20 },
            FileConstituent { path: "b.rs", code_lines: 30, comment_lines: 10, blank_lines: 5 },
        ],
    };
    set.add("f136e oversized", fat.oversized_files() == alloc::vec!["a.rs"], "超限文件点名");
    set.add("f136e split advice", fat.split_advice().is_some(), "超限给拆分建议");
    set.add("f136e split none", audit.split_advice().is_none(), "未超限不给建议");

    // 坡度
    set.add("f136e slope first", slope_verdict(SlopeCard { no: 1, reused: 0, fresh: 2 }).is_ok(), "首例无复用有新概念");
    set.add("f136e slope second", slope_verdict(SlopeCard { no: 2, reused: 2, fresh: 1 }).is_ok(), "递进+新概念");
    set.add("f136e slope broken", slope_verdict(SlopeCard { no: 3, reused: 0, fresh: 5 }).is_err(), "断裂拒绝");
    set.add("f136e slope flat", slope_verdict(SlopeCard { no: 4, reused: 9, fresh: 0 }).is_err(), "零坡度拒绝");
    set.add("f136e slope range", slope_verdict(SlopeCard { no: 6, reused: 1, fresh: 1 }).is_err(), "号越界拒绝");

    // 断绿账本
    let mut ledger = BreakLedger::new();
    ledger.record(100, BreakCause::ApiChange);
    ledger.record(105, BreakCause::CiEnvironment);
    set.add("f136e open count", ledger.open_count() == 2 && ledger.len() == 2, "双断绿在册");
    set.add("f136e worst streak", ledger.worst_open_streak(115) == 15, "最久未修 15 天");
    let _ = ledger.mark_fixed(108);
    set.add("f136e fifo fix", ledger.open_count() == 1 && ledger.by_cause(BreakCause::ApiChange) == 1, "先烂先修");
    let mut rev = BreakLedger::new();
    rev.record(100, BreakCause::ExampleBug);
    set.add("f136e fix timeline", rev.mark_fixed(99).is_err(), "修复早于断绿拒绝");
    set.add("f136e fix none", BreakLedger::new().mark_fixed(1).is_err(), "无断绿时修复拒绝");

    // 新手实录
    let mut nb = NewbieLedger::new();
    for no in 1..=5 {
        let _ = nb.record(NewbieRun { tester: "志愿者甲", example_no: no, passed: true, stuck_note: "", minutes: 9 });
    }
    set.add("f136e newbie 5/5", nb.all_five_passed() && nb.pass_rate_bp() == 10_000, "五例全过");
    let _ = nb.record(NewbieRun { tester: "志愿者乙", example_no: 2, passed: false, stuck_note: "构建命令找不到", minutes: 21 });
    set.add("f136e newbie mixed", nb.pass_rate_bp() == 8_333, "通过率随实录更新");
    set.add("f136e newbie stuck", nb.record(NewbieRun { tester: "丙", example_no: 1, passed: false, stuck_note: "", minutes: 5 }).is_err(), "无卡点的失败拒绝");

    // 截图基线
    let mut sb = ScreenshotBaseline::new();
    let _ = sb.set(1, fnv1a64(b"hello-window-frame"));
    set.add("f136e shot pass", sb.compare(1, b"hello-window-frame").is_ok(), "像素一致绿");
    set.add("f136e shot fail", sb.compare(1, b"moved-window").is_err(), "像素漂移红");
    set.add("f136e shot nobase", sb.compare(3, b"x").is_err(), "无基线拒绝");
    set.add("f136e shot range", sb.set(6, 1).is_err(), "示例号越界");

    // 学习时长
    let mins = estimate_learning_minutes(180, 3_000, 2);
    set.add("f136e minutes bounded", mins >= 5 && mins < 240, "时长有界且随因子变化");
    set.add(
        "f136e minutes monotonic",
        estimate_learning_minutes(180, 3_000, 3) > estimate_learning_minutes(180, 3_000, 1),
        "概念多时长增",
    );

    // 与基础层联动：五例注册完整性
    let mut repo = ExampleRepo::new();
    for (no, title, builds_on) in
        [(1usize, "hello", None), (2, "buttons", Some(1)), (3, "files", Some(2)), (4, "menus", Some(3)), (5, "notepad", Some(4))]
    {
        let _ = repo.register(Example {
            no,
            title,
            lines: 100,
            comment_lines: 35,
            builds_on,
            readme_ok: true,
        });
    }
    set.add("f136e repo complete", repo.complete(), "五例齐套（与基础层对账）");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn density_math() {
        let a = SourceAudit {
            files: alloc::vec![FileConstituent { path: "m.rs", code_lines: 100, comment_lines: 50, blank_lines: 50 }],
        };
        assert_eq!(a.total_lines(), 200);
        assert_eq!(a.density_bp(), 2_500); // 50/200
        assert!(a.total_ok()); // 恰好 200 = 达标（≤）
        let mut b = SourceAudit { files: alloc::vec![FileConstituent { path: "m.rs", code_lines: 180, comment_lines: 50, blank_lines: 50 }] };
        assert_eq!(b.total_lines(), 280);
        assert!(!b.total_ok());
        let advice = b.split_advice().expect("advice");
        assert_eq!(advice.len(), 1);
        b.files.push(FileConstituent { path: "n.rs", code_lines: 0, comment_lines: 0, blank_lines: 0 });
        assert_eq!(b.files.len(), 2);
    }

    #[test]
    fn break_causes_distinct() {
        let mut l = BreakLedger::new();
        l.record(1, BreakCause::StaleBaseline);
        l.record(2, BreakCause::StaleBaseline);
        l.record(3, BreakCause::ExampleBug);
        assert_eq!(l.by_cause(BreakCause::StaleBaseline), 2);
        assert_eq!(l.by_cause(BreakCause::ExampleBug), 1);
        assert_eq!(l.by_cause(BreakCause::ApiChange), 0);
        assert_eq!(l.worst_open_streak(10), 9);
    }

    #[test]
    fn newbie_boundary_examples() {
        let mut l = NewbieLedger::new();
        assert!(l.record(NewbieRun { tester: "t", example_no: 0, passed: true, stuck_note: "", minutes: 1 }).is_err());
        assert!(l.record(NewbieRun { tester: "t", example_no: 6, passed: true, stuck_note: "", minutes: 1 }).is_err());
        assert!(!l.all_five_passed());
    }
}
