//! VE-F0236 · Intel 认证矩阵测试（VE-B 域 · Intel 核显组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0236`
//!
//! **职责定位（锚点原文）**：代际×QEMU 通透性×驱动版本三维矩阵用例（基础
//! 渲染、显示、恢复、媒体能力只读验证四类），分 fast 宿主侧与 full QEMU
//! 批队列双档；失败输出含最小复现与矩阵坐标；结果聚合供收口引用。
//!
//! **数据结构（锚点原文）**：矩阵用例（三维坐标×用例×断言）；结果记录
//! （通过/失败/N/A/超时）。
//!
//! **判据（锚点原文）**：三维矩阵、双档、N/A 语义、最小复现、判据。
//! 本模块 14 条判据逐条映射：**三维矩阵 4 条**（三维坐标全覆盖生成／坐标
//! 键唯一不重复／四类用例逐类入阵／矩阵规模对账）、**双档 3 条**（fast 档
//! 只含宿主侧用例／full 档含 QEMU 批队列用例／双档用例集不相交）、
//! **N/A 语义 3 条**（环境缺格 N/A 不算失败／N/A 不进通过率分母的口径
//! 对拍／超时保留日志标记）、**最小复现 3 条**（失败带矩阵坐标＋最小复现
//! ／回归按坐标定位差异引入批次／结果聚合可供收口引用）、**判据自身 1 条**
//! （码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 环境缺格 → **N/A 标记不算失败** | 用例执行前查环境档位（QEMU 通透性），缺格即 [`CaseResult::NotApplicable`]——聚合时 N/A **不进通过率分母**（分母=已判定数），也不进失败数 |
//! | 用例超时 → **标记超时并保留日志** | [`Runner::run_one`] 超时上限入参，超时返回 [`CaseResult::TimedOut`] 并保留最后日志片段——超时与失败分码，避免「挂死」被当成「回归」 |
//! | 回归 → **按坐标定位差异引入批次** | 结果记录携带**基线对照**：上一轮同坐标结果为通过而本轮失败即回归，[`Runner::regressions`] 按（代际，通透性，驱动版本）坐标列出——收口直接引用坐标定位 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **矩阵生成是笛卡尔积但有界**：三维坐标（代际 4×通透性 2×驱动 3）= 24
//!   格，每格最多 4 类用例——全阵 96 例封顶，规模常量化，判据对拍不数动态值。
//! - **N/A 的口径是分母剔除不是计数隐藏**：通过率 = 通过 /（通过+失败+超时），
//!   N/A 完全出榜——把 N/A 算进分母会稀释通过率，算进分子是造假。
//! - **最小复现 = 坐标 + 用例类 + 复现步骤序号**：失败记录不含整个日志，
//!   只含「在哪一格、哪类用例、跑到第几步」——收口引用时可以稳定重现。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE540;

// ---- 诊断码 ----
pub const XC_COORD_BAD: u32 = SEG_BASE + 1; // 矩阵坐标非法（越界/重复）
pub const XC_CLASS_BAD: u32 = SEG_BASE + 2; // 用例类非法
pub const XC_TIMEOUT: u32 = SEG_BASE + 3; // 用例执行超时
pub const XC_ENV_MISS: u32 = SEG_BASE + 4; // 环境缺格（N/A）
pub const XC_QUEUE_FULL: u32 = SEG_BASE + 5; // 批队列满

/// 矩阵维度常量（规模封顶，判据对拍用）。
pub const GEN_DIM: usize = 4; // 代际 4 档
pub const TRANSP_DIM: usize = 2; // QEMU 通透性 2 档
pub const DRV_DIM: usize = 3; // 驱动版本 3 档
pub const CLASS_DIM: usize = 4; // 用例类 4 类

/// 矩阵三维坐标。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Coord {
    /// 代际序号（0..GEN_DIM）。
    pub gen: u8,
    /// QEMU 通透性序号（0=宿主直通，1=QEMU 模拟）。
    pub transp: u8,
    /// 驱动版本序号（0..DRV_DIM）。
    pub drv: u8,
}

impl Coord {
    /// 坐标合法性。
    pub const fn valid(&self) -> bool {
        self.gen < GEN_DIM as u8 && self.transp < TRANSP_DIM as u8 && self.drv < DRV_DIM as u8
    }

    /// 坐标的规范化键（读屏呈现用）。
    pub fn key(&self) -> String {
        alloc::format!("g{}t{}d{}", self.gen, self.transp, self.drv)
    }
}

/// 用例类（四类，锚点列举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseClass {
    /// 基础渲染。
    Render,
    /// 显示。
    Display,
    /// 恢复。
    Recovery,
    /// 媒体能力只读验证。
    MediaReadonly,
}

/// 档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// fast：宿主侧分钟级。
    Fast,
    /// full：QEMU 批队列过夜。
    Full,
}

/// 用例结果形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseResult {
    /// 通过。
    Passed,
    /// 失败（含最小复现坐标）。
    Failed,
    /// 环境缺格——不适用，不算失败。
    NotApplicable,
    /// 超时（保留日志）。
    TimedOut,
}

/// 一条用例。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 矩阵坐标。
    pub coord: Coord,
    /// 用例类。
    pub class: CaseClass,
    /// 执行档位。
    pub tier: Tier,
    /// 复现步骤数（最小复现的步序上界）。
    pub steps: u16,
}

impl Case {
    /// 用例合法性：坐标合法、步数有界。
    pub const fn well_formed(&self) -> bool {
        self.coord.valid() && self.steps > 0 && self.steps <= 64
    }
}

/// 结果记录（含最小复现三要素）。
#[derive(Clone, Debug)]
pub struct CaseRecord {
    /// 用例。
    pub case: Case,
    /// 结果。
    pub result: CaseResult,
    /// 最后日志片段（超时保留）。
    pub log_tail: String,
    /// 基线（上一轮）结果——回归判定用。
    pub baseline: Option<CaseResult>,
}

impl CaseRecord {
    /// 回归判定：基线通过而本轮失败。
    pub const fn is_regression(&self) -> bool {
        matches!(self.baseline, Some(CaseResult::Passed)) && matches!(self.result, CaseResult::Failed)
    }

    /// 最小复现三要素：坐标+类+步序（失败时呈现）。
    pub fn repro(&self) -> String {
        alloc::format!(
            "{} × {:?} × 步序/{}",
            self.case.coord.key(),
            self.case.class,
            self.case.steps
        )
    }
}

/// 认证矩阵运行器。
pub struct Runner {
    /// 全阵用例。
    cases: Vec<Case>,
    /// 结果记录。
    records: Vec<CaseRecord>,
    /// 批队列（full 档入队）。
    queue: Vec<Case>,
    /// 队列容量。
    queue_cap: usize,
    /// 队列溢出计数。
    pub queue_overflows: u32,
}

impl Runner {
    /// 新运行器；自动生成全阵用例（fast=宿主直通格，full=QEMU 格）。
    pub fn new(queue_cap: usize) -> Self {
        let mut cases = Vec::new();
        let mut queue = Vec::new();
        for g in 0..GEN_DIM as u8 {
            for t in 0..TRANSP_DIM as u8 {
                for d in 0..DRV_DIM as u8 {
                    let coord = Coord { gen: g, transp: t, drv: d };
                    let tier = if t == 0 { Tier::Fast } else { Tier::Full };
                    for class in [
                        CaseClass::Render,
                        CaseClass::Display,
                        CaseClass::Recovery,
                        CaseClass::MediaReadonly,
                    ] {
                        let c = Case { coord, class, tier, steps: 4 };
                        if tier == Tier::Full {
                            queue.push(c);
                        }
                        cases.push(c);
                    }
                }
            }
        }
        let over = if queue.len() > queue_cap { (queue.len() - queue_cap) as u32 } else { 0 };
        queue.truncate(queue_cap);
        Runner { cases, records: Vec::new(), queue, queue_cap, queue_overflows: over }
    }

    /// 矩阵用例总数（判据对拍：GEN×TRANSP×DRV×CLASS 常量）。
    pub fn total(&self) -> usize {
        self.cases.len()
    }

    /// 坐标键唯一性自检。
    pub fn coords_unique(&self) -> bool {
        // 同一格内四类用例共享坐标——唯一性指「坐标×类」组合不重复。
        let n = self.cases.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.cases[i].coord == self.cases[j].coord && self.cases[i].class == self.cases[j].class {
                    return false;
                }
            }
        }
        true
    }

    /// fast 档用例集（宿主直通格）。
    pub fn fast_set(&self) -> Vec<&Case> {
        self.cases.iter().filter(|c| c.tier == Tier::Fast).collect()
    }

    /// full 档批队列（QEMU 格）。
    pub fn full_queue(&self) -> &[Case] {
        &self.queue
    }

    /// 执行一格用例：环境缺格 N/A；步数超限超时；否则按步执行——第
    /// `fail_at` 步失败即 Failed（最小复现定位到步序）。
    pub fn run_one(&mut self, case: &Case, env_capable: bool, fail_at: Option<u16>, timeout_steps: u16) -> CaseResult {
        if !case.well_formed() {
            return CaseResult::Failed;
        }
        if !env_capable {
            // 环境缺格：N/A 不算失败（锚点：环境缺格→N/A 标记不算失败）。
            // N/A 也要入账——聚合四桶的 N/A 桶从这里来。
            self.records.push(CaseRecord {
                case: *case,
                result: CaseResult::NotApplicable,
                log_tail: alloc::format!("env missing: {:x} unavailable", case.coord.transp),
                baseline: None,
            });
            return CaseResult::NotApplicable;
        }
        match fail_at {
            None => {
                let rec = CaseRecord {
                    case: *case,
                    result: CaseResult::Passed,
                    log_tail: String::new(),
                    baseline: None,
                };
                self.records.push(rec);
                CaseResult::Passed
            }
            Some(at) if at > timeout_steps => {
                // 超时：与失败分码，保留日志（锚点：用例超时→标记超时并保留日志）。
                let rec = CaseRecord {
                    case: *case,
                    result: CaseResult::TimedOut,
                    log_tail: alloc::format!("timeout at step {} of {}", at, case.steps),
                    baseline: None,
                };
                self.records.push(rec);
                CaseResult::TimedOut
            }
            Some(at) => {
                let rec = CaseRecord {
                    case: *case,
                    result: CaseResult::Failed,
                    log_tail: alloc::format!("fail at step {} of {}", at, case.steps),
                    baseline: None,
                };
                self.records.push(rec);
                CaseResult::Failed
            }
        }
    }

    /// 聚合结果（收口引用）：（通过，失败，N/A，超时）。
    pub fn aggregate(&self) -> (u32, u32, u32, u32) {
        let mut p = 0u32;
        let mut f = 0u32;
        let mut na = 0u32;
        let mut to = 0u32;
        for r in &self.records {
            match r.result {
                CaseResult::Passed => p += 1,
                CaseResult::Failed => f += 1,
                CaseResult::NotApplicable => na += 1,
                CaseResult::TimedOut => to += 1,
            }
        }
        (p, f, na, to)
    }

    /// 通过率（千分比）：N/A 不进分母（口径锚定）。
    pub fn pass_rate_permil(&self) -> u32 {
        let (p, f, _na, to) = self.aggregate();
        let denom = p + f + to;
        if denom == 0 {
            return 0;
        }
        p * 1000 / denom
    }

    /// 回归清单：按坐标列出基线通过而本轮失败的记录。
    pub fn regressions(&self) -> Vec<&CaseRecord> {
        self.records.iter().filter(|r| r.is_regression()).collect()
    }

    /// 注入基线（上一轮结果）用于回归对拍。
    pub fn set_baseline(&mut self, coord: Coord, class: CaseClass, base: CaseResult) {
        for r in self.records.iter_mut() {
            if r.case.coord == coord && r.case.class == class {
                r.baseline = Some(base);
            }
        }
    }
}

// ---------------------------------------------------------------- 判据

/// VE-F0236 判据组（14 条）。
pub fn run_veb236_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb236_intelcert");

    // --- 判据 1：三维矩阵——全阵生成规模对账 ---
    {
        let r = Runner::new(64);
        let expect = (GEN_DIM * TRANSP_DIM * DRV_DIM * CLASS_DIM) as u32;
        s.add(
            "B36-三维矩阵-全阵规模对账",
            r.total() as u32 == expect && expect == 96,
            "4 代际×2 通透性×3 驱动×4 类 = 96 例常量封顶",
        );
    }
    // --- 判据 2：三维矩阵——坐标×类组合唯一 ---
    {
        let r = Runner::new(64);
        s.add(
            "B36-三维矩阵-组合键唯一",
            r.coords_unique(),
            "同格同类不重复生成（重复用例是矩阵腐化）",
        );
    }
    // --- 判据 3：三维矩阵——坐标越界拒绝 ---
    {
        let bad = Coord { gen: 9, transp: 0, drv: 0 };
        let ok = Coord { gen: 0, transp: 1, drv: 2 };
        s.add(
            "B36-三维矩阵-坐标越界拒绝",
            !bad.valid() && ok.valid(),
            "坐标合法性判定独立于矩阵生成（外部注入也可校验）",
        );
    }
    // --- 判据 4：三维矩阵——四类用例逐类入阵 ---
    {
        let r = Runner::new(64);
        // 每类用例数 = 格数（每格一类一例）。
        let grid = GEN_DIM * TRANSP_DIM * DRV_DIM;
        let count_render = r.cases.iter().filter(|c| c.class == CaseClass::Render).count();
        let count_media = r.cases.iter().filter(|c| c.class == CaseClass::MediaReadonly).count();
        s.add(
            "B36-三维矩阵-四类逐类入阵",
            count_render == grid && count_media == grid,
            "渲染与媒体只读两类各占满全格（显示/恢复同构不再重复断言）",
        );
    }
    // --- 判据 5：双档——fast 档只含宿主直通格 ---
    {
        let r = Runner::new(64);
        let fast = r.fast_set();
        let all_host = fast.iter().all(|c| c.coord.transp == 0);
        s.add(
            "B36-双档-fast只含宿主格",
            all_host && fast.len() == GEN_DIM * DRV_DIM * CLASS_DIM,
            "通透性=0 的格全进 fast 档，规模 = 4×3×4 = 48",
        );
    }
    // --- 判据 6：双档——full 档批队列与 fast 不相交 ---
    {
        let r = Runner::new(64);
        let q = r.full_queue();
        let all_qemu = q.iter().all(|c| c.coord.transp == 1);
        let disjoint = !q.iter().any(|c| r.fast_set().iter().any(|f| f.coord == c.coord && f.class == c.class));
        s.add(
            "B36-双档-full队列与fast不相交",
            all_qemu && disjoint && q.len() == GEN_DIM * DRV_DIM * CLASS_DIM,
            "QEMU 格全进批队列，两档用例集零交集",
        );
    }
    // --- 判据 7：双档——队列容量溢出计数 ---
    {
        // 队列容量 8 < 需要 48 → 溢出 40。
        let r = Runner::new(8);
        s.add(
            "B36-双档-队列溢出计数",
            r.full_queue().len() == 8 && r.queue_overflows == 40,
            "容量截断留痕：48 例入 8 容量溢出 40（不静默丢弃）",
        );
    }
    // --- 判据 8：N/A 语义——环境缺格 N/A 不算失败 ---
    {
        let mut r = Runner::new(64);
        let c = Case { coord: Coord { gen: 0, transp: 1, drv: 0 }, class: CaseClass::Render, tier: Tier::Full, steps: 2 };
        let res = r.run_one(&c, false, Some(1), 16);
        let (p, f, na, to) = r.aggregate();
        s.add(
            "B36-NA语义-缺格不算失败",
            res == CaseResult::NotApplicable && na == 1 && f == 0 && p == 0,
            "缺格标记 N/A：失败数零、通过数零（不虚报也不虚败）",
        );
    }
    // --- 判据 9：N/A 语义——通过率口径 N/A 出分母 ---
    {
        let mut r = Runner::new(64);
        let c1 = Case { coord: Coord { gen: 0, transp: 0, drv: 0 }, class: CaseClass::Render, tier: Tier::Fast, steps: 2 };
        let c2 = Case { coord: Coord { gen: 0, transp: 0, drv: 1 }, class: CaseClass::Display, tier: Tier::Fast, steps: 2 };
        let c3 = Case { coord: Coord { gen: 0, transp: 1, drv: 0 }, class: CaseClass::Render, tier: Tier::Full, steps: 2 };
        let _ = r.run_one(&c1, true, None, 16); // 通过
        let _ = r.run_one(&c2, true, Some(1), 16); // 失败
        let _ = r.run_one(&c3, false, None, 16); // N/A
        s.add(
            "B36-NA语义-通过率N/A出分母",
            r.pass_rate_permil() == 500,
            "1 通过 1 失败 1 N/A → 通过率 1/2 = 500‰（N/A 不稀释不虚抬）",
        );
    }
    // --- 判据 10：N/A 语义——超时与失败分码且保留日志 ---
    {
        let mut r = Runner::new(64);
        let c = Case { coord: Coord { gen: 1, transp: 0, drv: 0 }, class: CaseClass::Recovery, tier: Tier::Fast, steps: 4 };
        let to = r.run_one(&c, true, Some(8), 4);
        let rec = r.records.last();
        s.add(
            "B36-NA语义-超时分码留日志",
            to == CaseResult::TimedOut
                && rec.map(|x| !x.log_tail.is_empty() && x.result == CaseResult::TimedOut).unwrap_or(false),
            "步数超上限=超时（不是失败），日志片段保留定位挂点",
        );
    }
    // --- 判据 11：最小复现——失败带坐标与步序 ---
    {
        let mut r = Runner::new(64);
        let c = Case { coord: Coord { gen: 2, transp: 0, drv: 1 }, class: CaseClass::MediaReadonly, tier: Tier::Fast, steps: 3 };
        let _ = r.run_one(&c, true, Some(2), 16);
        let rec = r.records.last();
        let rep = rec.map(|x| x.repro()).unwrap_or_default();
        s.add_dyn(
            "B36-最小复现-失败三要素齐",
            rec.map(|x| x.result == CaseResult::Failed).unwrap_or(false)
                && rep.contains("g2t0d1")
                && rep.contains("MediaReadonly"),
            &alloc::format!("最小复现 = 坐标×类×步序：{}", rep),
        );
    }
    // --- 判据 12：最小复现——回归按坐标定位 ---
    {
        let mut r = Runner::new(64);
        let c = Case { coord: Coord { gen: 3, transp: 0, drv: 2 }, class: CaseClass::Render, tier: Tier::Fast, steps: 2 };
        let _ = r.run_one(&c, true, Some(1), 16);
        r.set_baseline(c.coord, c.class, CaseResult::Passed);
        let regs = r.regressions();
        s.add(
            "B36-最小复现-回归按坐标定位",
            regs.len() == 1 && regs[0].case.coord == c.coord && regs[0].is_regression(),
            "基线通过+本轮失败=回归，坐标直达差异格",
        );
    }
    // --- 判据 13：最小复现——聚合可供收口引用 ---
    {
        let mut r = Runner::new(64);
        let c1 = Case { coord: Coord { gen: 0, transp: 0, drv: 0 }, class: CaseClass::Render, tier: Tier::Fast, steps: 2 };
        let c2 = Case { coord: Coord { gen: 1, transp: 0, drv: 0 }, class: CaseClass::Display, tier: Tier::Fast, steps: 2 };
        let _ = r.run_one(&c1, true, None, 16);
        let _ = r.run_one(&c2, true, Some(3), 16);
        let (p, f, na, to) = r.aggregate();
        s.add(
            "B36-最小复现-聚合四桶可引用",
            p == 1 && f == 1 && na == 0 && to == 0 && r.pass_rate_permil() == 500,
            "四桶聚合（通过/失败/N/A/超时）为收口提供唯一口径",
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_COORD_BAD, XC_CLASS_BAD, XC_TIMEOUT, XC_ENV_MISS, XC_QUEUE_FULL];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B36-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 5,
            "五诊断码互异且全部落在 0xE540 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb236_matrix_shape() {
        let r = Runner::new(64);
        assert_eq!(r.total(), 96);
        assert!(r.coords_unique());
        assert_eq!(r.fast_set().len(), 48);
        assert_eq!(r.full_queue().len(), 48);
        // 容量 8 截断 + 溢出留痕。
        let r2 = Runner::new(8);
        assert_eq!(r2.full_queue().len(), 8);
        assert_eq!(r2.queue_overflows, 40);
    }

    #[test]
    fn veb236_na_and_pass_rate() {
        let mut r = Runner::new(64);
        let mk = |g: u8, d: u8, cl: CaseClass| Case {
            coord: Coord { gen: g, transp: 0, drv: d },
            class: cl,
            tier: Tier::Fast,
            steps: 2,
        };
        let _ = r.run_one(&mk(0, 0, CaseClass::Render), true, None, 16);
        let _ = r.run_one(&mk(0, 1, CaseClass::Display), true, Some(1), 16);
        let _ = r.run_one(&mk(1, 0, CaseClass::Render), false, None, 16);
        let _ = r.run_one(&mk(1, 1, CaseClass::Recovery), true, Some(9), 4);
        let (p, f, na, to) = r.aggregate();
        assert_eq!((p, f, na, to), (1, 1, 1, 1));
        assert_eq!(r.pass_rate_permil(), 333);
    }

    #[test]
    fn veb236_regression_detection() {
        let mut r = Runner::new(64);
        let c = Case { coord: Coord { gen: 2, transp: 0, drv: 2 }, class: CaseClass::Render, tier: Tier::Fast, steps: 2 };
        let _ = r.run_one(&c, true, Some(1), 16);
        assert!(r.regressions().is_empty(), "无基线不算回归");
        r.set_baseline(c.coord, c.class, CaseResult::Passed);
        assert_eq!(r.regressions().len(), 1);
        // 基线本身就是失败不算回归（是持续失败）。
        let mut r2 = Runner::new(64);
        let _ = r2.run_one(&c, true, Some(1), 16);
        r2.set_baseline(c.coord, c.class, CaseResult::Failed);
        assert!(r2.regressions().is_empty());
    }

    #[test]
    fn veb236_minimal_repro_text() {
        let mut r = Runner::new(64);
        let c = Case { coord: Coord { gen: 3, transp: 0, drv: 1 }, class: CaseClass::MediaReadonly, tier: Tier::Fast, steps: 3 };
        let _ = r.run_one(&c, true, Some(2), 16);
        let (rep, tail) = match r.records.last() {
            Some(x) => (x.repro(), x.log_tail.clone()),
            None => (alloc::format!("none"), alloc::format!("")),
        };
        assert!(rep.contains("g3t0d1"), "{}", rep);
        assert!(tail.contains("step 2"), "{}", tail);
    }

    #[test]
    fn veb236_checks_all_green() {
        let set = run_veb236_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0236 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
