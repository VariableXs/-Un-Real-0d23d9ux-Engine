//! VE-F3659 · 视觉创作 fuzz（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3659`
//!
//! **职责定位（锚点原文）**：画布操作 fuzz——
//! - **三变异**：图层变异（千层/循环混合/蒙版环）/ 格式变异（畸形导入）/
//!   时序变异（undo×导出×预览竞态）；
//! - **三不变量**：编辑不丢数据 / 导出正确 / 不崩溃（丢数据 P0 复用）；
//! - **最小化 + 闭环复述**。
//!
//! **判据（锚点原文六条）**：三不变量、丢数据 P0、三变异、
//! 闭环复述、基建单源、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 丢数据→P0（复述红线实测） | [`F59Code::DATA_LOSS`] |
//! | 崩溃→最小化入库（复述） | [`F59Code::CRASH`] |
//! | 挂起→看门狗（复述） | [`F59Code::HANG`] |
//! | 闭环缺→阻断（复述） | [`F59Code::LOOP_OPEN`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3659 诊断码段（独占 0x6Bxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct F59Code(pub u16);

impl F59Code {
    /// 丢数据（P0 最高级）。
    pub const DATA_LOSS: F59Code = F59Code(0x6B01);
    /// 崩溃（最小化入库）。
    pub const CRASH: F59Code = F59Code(0x6B02);
    /// 挂起（看门狗）。
    pub const HANG: F59Code = F59Code(0x6B03);
    /// 闭环缺失（阻断）。
    pub const LOOP_OPEN: F59Code = F59Code(0x6B04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            F59Code::DATA_LOSS => "丢数据（P0 红线实测）。建议：最小化语料入库立案。",
            F59Code::CRASH => "画布崩溃。建议：最小化用例入库。",
            F59Code::HANG => "挂起。建议：看门狗超时回收。",
            F59Code::LOOP_OPEN => "闭环缺失。建议：补闭环断言后重放。",
            _ => "未知诊断码。",
        }
    }
}

// ── 变异 ────────────────────────────────────────────────────────────

/// 变异类型（三级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutation {
    /// 图层变异（千层/循环混合/蒙版环）。
    LayerStorm,
    /// 格式变异（畸形导入）。
    FormatFuzz,
    /// 时序变异（undo×导出×预览竞态）。
    RaceFuzz,
}

impl Mutation {
    pub const ALL: [Mutation; 3] = [Mutation::LayerStorm, Mutation::FormatFuzz, Mutation::RaceFuzz];
}

// ── 不变量 ──────────────────────────────────────────────────────────

/// 三不变量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Invariant {
    /// 编辑不丢数据（P0）。
    NoDataLoss,
    /// 导出正确。
    ExportCorrect,
    /// 不崩溃。
    NoCrash,
}

impl Invariant {
    pub const ALL: [Invariant; 3] = [Invariant::NoDataLoss, Invariant::ExportCorrect, Invariant::NoCrash];

    /// 违例严重度（丢数据 = P0 最高级）。
    pub fn severity_p0(self) -> bool {
        self == Invariant::NoDataLoss
    }
}

// ── 语料 ────────────────────────────────────────────────────────────

/// 用例语料。
#[derive(Clone, Copy)]
pub struct Case {
    pub id: u32,
    pub mutation: Mutation,
    /// 是否最小化入库（崩溃/违例后）。
    pub minimized: bool,
}

// ── fuzz 驱动 ───────────────────────────────────────────────────────

/// 视觉创作 fuzz 驱动。
pub struct CanvasFuzzer {
    corpus: Vec<Case>,
    violations: Vec<Invariant>,
    crashes: u32,
    hangs: u32,
    loop_closed: bool,
    watchdog_hits: u32,
    last_code: F59Code,
    has_error: bool,
}

impl CanvasFuzzer {
    pub fn new() -> Self {
        Self {
            corpus: Vec::new(),
            violations: Vec::new(),
            crashes: 0,
            hangs: 0,
            loop_closed: true,
            watchdog_hits: 0,
            last_code: F59Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> F59Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: F59Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 入语料。
    pub fn seed(&mut self, id: u32, mutation: Mutation) -> usize {
        self.corpus.push(Case { id, mutation, minimized: false });
        self.corpus.len()
    }

    /// 跑一条用例：报告违例/崩溃/挂起。
    pub fn execute(&mut self, id: u32, violation: Option<Invariant>, crash: bool, hang: bool) -> Result<u32, F59Code> {
        let idx = self.corpus.iter().position(|c| c.id == id);
        if hang {
            self.hangs += 1;
            self.watchdog_hits += 1;
            if let Some(i) = idx {
                self.corpus[i].minimized = true;
            }
            self.fail(F59Code::HANG);
            return Err(F59Code::HANG);
        }
        if crash {
            self.crashes += 1;
            if let Some(i) = idx {
                self.corpus[i].minimized = true;
            }
            self.fail(F59Code::CRASH);
            return Err(F59Code::CRASH);
        }
        if let Some(inv) = violation {
            self.violations.push(inv);
            if let Some(i) = idx {
                self.corpus[i].minimized = true;
            }
            if inv.severity_p0() {
                self.fail(F59Code::DATA_LOSS);
                return Err(F59Code::DATA_LOSS);
            }
            self.fail(F59Code::CRASH);
            return Err(F59Code::CRASH);
        }
        self.last_code = F59Code(0);
        self.has_error = false;
        Ok(id)
    }

    /// 三变异覆盖。
    pub fn mutations_covered(&self) -> bool {
        Mutation::ALL
            .iter()
            .all(|m| self.corpus.iter().any(|c| c.mutation == *m))
    }

    /// 丢数据违例数（P0）。
    pub fn data_loss_count(&self) -> usize {
        self.violations.iter().filter(|i| i.severity_p0()).count()
    }

    /// 最小化入库数。
    pub fn minimized_count(&self) -> usize {
        self.corpus.iter().filter(|c| c.minimized).count()
    }

    /// 看门狗命中。
    pub fn watchdog_hits(&self) -> u32 {
        self.watchdog_hits
    }

    /// 闭环断言：闭环缺失即阻断。
    pub fn close_loop(&mut self, closed: bool) -> Result<bool, F59Code> {
        self.loop_closed = closed;
        if closed {
            self.last_code = F59Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(F59Code::LOOP_OPEN);
            Err(F59Code::LOOP_OPEN)
        }
    }

    pub fn corpus_size(&self) -> usize {
        self.corpus.len()
    }

    pub fn crash_count(&self) -> u32 {
        self.crashes
    }

    pub fn hang_count(&self) -> u32 {
        self.hangs
    }

    /// 基建单源总评（F2878/F3155 复用面）：闭环 + 有语料。
    pub fn infra_green(&self) -> bool {
        self.loop_closed && !self.corpus.is_empty()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves59_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves59_canvasfuzz");
    {
        let f = CanvasFuzzer::new();
        chk(&mut set, "C3659-01", f.corpus_size() == 0 && f.crash_count() == 0, "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        let n = f.seed(1, Mutation::LayerStorm);
        chk(&mut set, "C3659-02", n == 1, "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        f.seed(1, Mutation::LayerStorm);
        let r = f.execute(1, None, false, false);
        chk(&mut set, "C3659-03", r == Ok(1), "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        f.seed(1, Mutation::FormatFuzz);
        let r = f.execute(1, Some(Invariant::NoDataLoss), false, false);
        chk(&mut set, "C3659-04", r.err() == Some(F59Code::DATA_LOSS) && f.data_loss_count() == 1, "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        f.seed(1, Mutation::RaceFuzz);
        let r = f.execute(1, None, true, false);
        chk(&mut set, "C3659-05", r.err() == Some(F59Code::CRASH) && f.minimized_count() == 1, "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        f.seed(1, Mutation::LayerStorm);
        let r = f.execute(1, None, false, true);
        chk(&mut set, "C3659-06", r.err() == Some(F59Code::HANG) && f.watchdog_hits() == 1, "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        f.seed(1, Mutation::LayerStorm);
        f.seed(2, Mutation::FormatFuzz);
        f.seed(3, Mutation::RaceFuzz);
        chk(&mut set, "C3659-07", f.mutations_covered(), "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        let r = f.close_loop(true);
        chk(&mut set, "C3659-08", r == Ok(true), "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        let r = f.close_loop(false);
        chk(&mut set, "C3659-09", r.err() == Some(F59Code::LOOP_OPEN), "err");
    }
    {
        let inv = Invariant::NoDataLoss;
        chk(&mut set, "C3659-10", inv.severity_p0() && !Invariant::ExportCorrect.severity_p0(), "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        f.seed(1, Mutation::FormatFuzz);
        let r = f.execute(1, Some(Invariant::ExportCorrect), false, false);
        chk(&mut set, "C3659-11", r.err() == Some(F59Code::CRASH) && f.data_loss_count() == 0, "err");
    }
    {
        let mut f = CanvasFuzzer::new();
        f.seed(1, Mutation::LayerStorm);
        f.seed(2, Mutation::FormatFuzz);
        f.seed(3, Mutation::RaceFuzz);
        f.close_loop(true);
        chk(&mut set, "C3659-12", f.infra_green() && f.mutations_covered(), "err");
    }
    set
}
