//! F136 示例应用仓库 · 完整设计（STAR I 主册 G-D-11）。
//!
//! **判据（主册）**：五示例 CI 连续 30 天绿；新手实测（非作者）照
//! README 跑通 5/5。
//!
//! **设计要点（主册）**：五示例递进（①hello 窗口 ②按钮与布局 ③文件
//! 读写 ④多窗口与菜单 ⑤完整小应用）；每示例 ≤200 行（超了拆）；教学
//! 注释密度 30%；README 即索引（一句话/截图/构建命令/预计学习时长）；
//! CI 夜跑构建+运行冒烟（示例窗口出现即绿）；示例间递进关系文档化
//! （②用①的知识+一点新）；构建命令统一 `vxapp run examples/NN`；
//! 截图基线进 CI 比对。
//!
//! 本模块是示例仓库的**健康度账本**：五示例注册（递进关系+行数上限
//! 门禁）、CI 连绿账（30 天判据的计数引擎——断绿清零重计，如实记
//! 录断点）、注释密度核、新手实测记录册。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

pub const EXAMPLE_COUNT: usize = 5;
/// 每示例行数上限（超了拆）。
pub const MAX_LINES_PER_EXAMPLE: usize = 200;
/// 教学注释密度（百分比下限）。
pub const COMMENT_DENSITY_PCT: usize = 30;
/// CI 连绿天数线。
pub const CI_GREEN_STREAK_DAYS: u32 = 30;

// ---------------------------------------------------------------------------
// 示例注册
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Example {
    pub no: usize, // 1..=5
    pub title: &'static str,
    pub lines: usize,
    /// 注释行数（教学注释）。
    pub comment_lines: usize,
    /// 递进关系：no N 必须声明使用 N-1 的知识点（①除外）。
    pub builds_on: Option<usize>,
    /// README 四件：一句话/截图/构建命令/预计学习时长。
    pub readme_ok: bool,
}

impl Example {
    pub fn size_ok(&self) -> bool {
        self.lines <= MAX_LINES_PER_EXAMPLE
    }

    pub fn comment_density_ok(&self) -> bool {
        // 密度 = 注释行 / 总行数，≥30%（下限不是精确——上限 60% 防
        // 注释刷行数，主册口径是「教学注释密度 30%」的合理化区间）。
        if self.lines == 0 {
            return false;
        }
        let pct = self.comment_lines * 100 / self.lines;
        (30..=60).contains(&pct)
    }

    pub fn readme_ok(&self) -> bool {
        self.readme_ok
    }
}

// ---------------------------------------------------------------------------
// CI 连绿账
// ---------------------------------------------------------------------------

/// 夜跑结果流 → 连绿天数。断绿清零重计并如实记断点（不粉饰——
/// 与 F138「归档如实记录」同源纪律）。
pub struct CiStreak {
    streak: u32,
    best: u32,
    breaks: u32,
    /// 最近一次断绿的日序（0 = 从未断过）。
    pub last_break_day: u32,
}

impl CiStreak {
    pub const fn new() -> CiStreak {
        CiStreak { streak: 0, best: 0, breaks: 0, last_break_day: 0 }
    }

    /// 记一夜结果。绿 +1；红清零记断点。
    pub fn record(&mut self, day: u32, green: bool) {
        if green {
            self.streak += 1;
            if self.streak > self.best {
                self.best = self.streak;
            }
        } else {
            self.breaks += 1;
            self.last_break_day = day;
            self.streak = 0;
        }
    }

    pub fn green_now(&self) -> bool {
        self.streak > 0
    }

    /// 判据线：当前连绿 ≥30 天。
    pub fn meets_30d(&self) -> bool {
        self.streak >= CI_GREEN_STREAK_DAYS
    }

    pub fn best_streak(&self) -> u32 {
        self.best
    }

    pub fn breaks(&self) -> u32 {
        self.breaks
    }
}

// ---------------------------------------------------------------------------
// 示例仓库总账
// ---------------------------------------------------------------------------

pub struct ExampleRepo {
    examples: [Option<Example>; EXAMPLE_COUNT],
    ci: [CiStreak; EXAMPLE_COUNT],
    /// 新手实测记录：(示例号, 跑通)——非作者实测 5/5 判据的记录面。
    novice_runs: [(bool, bool); EXAMPLE_COUNT], // (recorded, passed)
}

impl ExampleRepo {
    pub fn new() -> ExampleRepo {
        ExampleRepo {
            examples: [None; EXAMPLE_COUNT],
            ci: [CiStreak::new(), CiStreak::new(), CiStreak::new(), CiStreak::new(), CiStreak::new()],
            novice_runs: [(false, false); EXAMPLE_COUNT],
        }
    }

    /// 注册示例：编号 1..=5；递进关系强制（no>1 必须声明 builds_on =
    /// no-1，跳级构建拒绝）；行数上限门禁；README 四件必齐。
    pub fn register(&mut self, ex: Example) -> Result<(), &'static str> {
        if !(1..=EXAMPLE_COUNT).contains(&ex.no) {
            return Err("example no out of range");
        }
        if !ex.size_ok() {
            return Err("over 200 lines — split it");
        }
        if !ex.comment_density_ok() {
            return Err("teaching comment density below 30% or absurdly high");
        }
        if !ex.readme_ok() {
            return Err("readme four items required");
        }
        match (ex.no, ex.builds_on) {
            (1, None) => {}
            (n, Some(prev)) if prev == n - 1 => {}
            _ => return Err("progressive build chain violated"),
        }
        self.examples[ex.no - 1] = Some(ex);
        Ok(())
    }

    /// 注册齐全（五示例全在册）。
    pub fn complete(&self) -> bool {
        self.examples.iter().all(|o| o.is_some())
    }

    pub fn record_ci(&mut self, no: usize, day: u32, green: bool) -> Result<(), &'static str> {
        if !(1..=EXAMPLE_COUNT).contains(&no) {
            return Err("bad example no");
        }
        self.ci[no - 1].record(day, green);
        Ok(())
    }

    /// CI 判据：五示例全部当前连绿 ≥30 天。
    pub fn ci_all_green_30d(&self) -> bool {
        self.ci.iter().all(|c| c.meets_30d())
    }

    /// 新手实测登记（非作者口径由流程保证——本层只记账）。
    pub fn record_novice(&mut self, no: usize, passed: bool) -> Result<(), &'static str> {
        if !(1..=EXAMPLE_COUNT).contains(&no) {
            return Err("bad example no");
        }
        self.novice_runs[no - 1] = (true, passed);
        Ok(())
    }

    /// 新手判据：5/5 跑通。
    pub fn novice_5_of_5(&self) -> bool {
        self.novice_runs.iter().all(|&(rec, pass)| rec && pass)
    }

    pub fn streak_of(&self, no: usize) -> &CiStreak {
        &self.ci[no - 1]
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F136_TAG: &str = "stareco-F136-examples";

pub fn run_examples_checks() -> CheckSet {
    let mut set = CheckSet::new(F136_TAG);

    fn ex(no: usize, title: &'static str, lines: usize, comments: usize) -> Example {
        Example {
            no,
            title,
            lines,
            comment_lines: comments,
            builds_on: if no == 1 { None } else { Some(no - 1) },
            readme_ok: true,
        }
    }

    let mut repo = ExampleRepo::new();
    set.add("f136 empty repo incomplete", !repo.complete(), "nothing registered");

    // 行数上限门禁
    set.add("f136 over-200 rejected", repo.register(ex(1, "hello", 201, 60)).is_err(), "split it");
    // 注释密度门禁
    set.add("f136 low comments rejected", repo.register(ex(1, "hello", 100, 10)).is_err(), "<30%");
    // README 门禁
    let mut no_readme = ex(1, "hello", 100, 40);
    no_readme.readme_ok = false;
    set.add("f136 no readme rejected", repo.register(no_readme).is_err(), "four items");
    // 递进链门禁：②跳过①不行
    let mut skip = ex(2, "buttons", 100, 40);
    skip.builds_on = None;
    set.add("f136 skipped build-on rejected", repo.register(skip).is_err(), "progressive chain");
    set.add("f136 hello needs no build-on", repo.register(ex(1, "hello", 100, 40)).is_ok(), "entry point");

    // 五示例全注册（行数压在 200 内、注释 30-60%）
    assert!(repo.register(ex(2, "buttons-layout", 120, 45)).is_ok());
    assert!(repo.register(ex(3, "file-io", 150, 55)).is_ok());
    assert!(repo.register(ex(4, "menus-multiwin", 180, 70)).is_ok());
    assert!(repo.register(ex(5, "notepad-mini", 200, 75)).is_ok());
    set.add("f136 five examples complete", repo.complete(), "ladder 1→5");

    // CI 连绿 30 天：前 60 天全绿
    for day in 1..=60u32 {
        repo.record_ci(1, day, true).ok();
        repo.record_ci(2, day, true).ok();
        repo.record_ci(3, day, true).ok();
        repo.record_ci(4, day, true).ok();
        repo.record_ci(5, day, true).ok();
    }
    set.add("f136 ci 30d streak green", repo.ci_all_green_30d(), "60 straight days");

    // 断绿清零 + 断点如实记录
    assert!(repo.record_ci(3, 61, false).is_ok());
    set.add(
        "f136 break resets and is logged",
        !repo.ci_all_green_30d()
            && repo.streak_of(3).green_now() == false
            && repo.streak_of(3).last_break_day == 61
            && repo.streak_of(3).breaks() == 1,
        "honest ledger",
    );
    set.add("f136 others unaffected", repo.streak_of(1).meets_30d(), "per-example ledger");
    // 重新养绿
    for day in 62..=92u32 {
        repo.record_ci(3, day, true).ok();
    }
    set.add("f136 streak regrown", repo.streak_of(3).meets_30d() && repo.ci_all_green_30d(), "30d again");

    // 新手实测 5/5
    for no in 1..=5usize {
        repo.record_novice(no, true).ok();
    }
    set.add("f136 novice 5/5", repo.novice_5_of_5(), "non-author runs");
    assert!(repo.record_novice(5, false).is_ok());
    set.add("f136 novice failure honest", !repo.novice_5_of_5(), "4/5 stays 4/5");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_window() {
        let mut e = Example {
            no: 1,
            title: "t",
            lines: 100,
            comment_lines: 30,
            builds_on: None,
            readme_ok: true,
        };
        assert!(e.comment_density_ok());
        e.comment_lines = 61;
        assert!(!e.comment_density_ok());
    }

    #[test]
    fn streak_best_tracked() {
        let mut c = CiStreak::new();
        for d in 1..=40 {
            c.record(d, true);
        }
        c.record(41, false);
        c.record(42, true);
        assert_eq!(c.best_streak(), 40);
        assert_eq!(c.last_break_day, 41);
        assert!(!c.meets_30d());
    }
}
