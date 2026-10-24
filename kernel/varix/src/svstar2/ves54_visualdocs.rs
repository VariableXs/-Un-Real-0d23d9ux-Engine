//! VE-F3654 · 视觉创作文档（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3654`
//!
//! **职责定位（锚点原文）**：视觉创作文档——
//! - **工坊手册**：四工坊各一章 + 共享核章（五段）；
//! - **创作指南**：视觉设计规范（网格/色彩/精度规范——规范教学）；
//! - **发布指南复述**、**同源纪律复述**、**2 跳地图复用**。
//!
//! **判据（锚点原文六条）**：五段手册、规范教学、2 跳复用、
//! SOP 复述、家族管道、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 漂移→对账（复述） | [`D54Code::DOC_DRIFT`] |
//! | 规范缺→补（教学红线） | [`D54Code::SPEC_GAP`] |
//! | 示例失效→断言（复述） | [`D54Code::SAMPLE_BROKEN`] |
//! | 迷宫→2 跳（复用红线） | [`D54Code::MAZE`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3654 诊断码段（独占 0x66xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct D54Code(pub u16);

impl D54Code {
    /// 文档漂移（对账）。
    pub const DOC_DRIFT: D54Code = D54Code(0x6601);
    /// 规范教学缺项。
    pub const SPEC_GAP: D54Code = D54Code(0x6602);
    /// 文档示例失效。
    pub const SAMPLE_BROKEN: D54Code = D54Code(0x6603);
    /// 超 2 跳迷宫（复用红线）。
    pub const MAZE: D54Code = D54Code(0x6604);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            D54Code::DOC_DRIFT => "文档漂移。建议：对账清单修正。",
            D54Code::SPEC_GAP => "规范教学缺项（教学红线）。建议：补网格/色彩/精度规范。",
            D54Code::SAMPLE_BROKEN => "文档示例失效。建议：示例即测试，修复重跑。",
            D54Code::MAZE => "跳数超 2 跳（复用红线）。建议：压平文档地图。",
            _ => "未知诊断码。",
        }
    }
}

// ── 五段手册 ────────────────────────────────────────────────────────

/// 手册章节（五段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chapter {
    /// 壁纸工坊章。
    Wallpaper,
    /// 图标工坊章。
    Icon,
    /// 光标工坊章。
    Cursor,
    /// 锁屏工坊章。
    LockScreen,
    /// 共享核章。
    SharedCore,
}

impl Chapter {
    pub const ALL: [Chapter; 5] = [
        Chapter::Wallpaper,
        Chapter::Icon,
        Chapter::Cursor,
        Chapter::LockScreen,
        Chapter::SharedCore,
    ];
}

/// 规范条目（创作指南）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Spec {
    /// 网格规范。
    Grid,
    /// 色彩规范。
    Color,
    /// 精度规范。
    Precision,
}

impl Spec {
    pub const ALL: [Spec; 3] = [Spec::Grid, Spec::Color, Spec::Precision];
}

// ── 文档节点（2 跳地图） ────────────────────────────────────────────

/// 文档页。
#[derive(Clone, Copy)]
pub struct DocPage {
    pub id: u16,
    /// 到根的跳数（须 ≤2）。
    pub hops: u8,
    /// 示例是否可跑。
    pub sample_ok: bool,
}

// ── 文档体系 ────────────────────────────────────────────────────────

/// 视觉创作文档体系。
pub struct VisualDocs {
    chapters: Vec<Chapter>,
    specs: Vec<Spec>,
    pages: Vec<DocPage>,
    sop_steps: u32,
    last_code: D54Code,
    has_error: bool,
}

impl VisualDocs {
    pub fn new() -> Self {
        Self {
            chapters: Vec::new(),
            specs: Vec::new(),
            pages: Vec::new(),
            sop_steps: 0,
            last_code: D54Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> D54Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: D54Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 成章。
    pub fn add_chapter(&mut self, c: Chapter) -> usize {
        if !self.chapters.contains(&c) {
            self.chapters.push(c);
        }
        self.chapters.len()
    }

    /// 五段齐。
    pub fn manual_complete(&self) -> bool {
        Chapter::ALL.iter().all(|c| self.chapters.contains(c))
    }

    /// 登记规范教学条目。
    pub fn add_spec(&mut self, s: Spec) -> usize {
        if !self.specs.contains(&s) {
            self.specs.push(s);
        }
        self.specs.len()
    }

    /// 规范教学齐（网格/色彩/精度）。
    pub fn specs_complete(&mut self) -> Result<bool, D54Code> {
        if Spec::ALL.iter().all(|s| self.specs.contains(s)) {
            self.last_code = D54Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(D54Code::SPEC_GAP);
            Err(D54Code::SPEC_GAP)
        }
    }

    /// 挂文档页；跳数超 2 判迷宫。
    pub fn add_page(&mut self, page: DocPage) -> Result<usize, D54Code> {
        if page.hops > 2 {
            self.fail(D54Code::MAZE);
            return Err(D54Code::MAZE);
        }
        self.pages.push(page);
        self.last_code = D54Code(0);
        self.has_error = false;
        Ok(self.pages.len())
    }

    /// 2 跳地图总评。
    pub fn map_2hop(&self) -> bool {
        !self.pages.is_empty() && self.pages.iter().all(|p| p.hops <= 2)
    }

    /// 示例断言：全部页示例可跑。
    pub fn samples_green(&mut self) -> Result<bool, D54Code> {
        if self.pages.iter().all(|p| p.sample_ok) {
            self.last_code = D54Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(D54Code::SAMPLE_BROKEN);
            Err(D54Code::SAMPLE_BROKEN)
        }
    }

    /// SOP 复述：发布步骤数登记（≥3 步）。
    pub fn sop(&mut self, steps: u32) -> Result<u32, D54Code> {
        self.sop_steps = steps;
        if steps >= 3 {
            self.last_code = D54Code(0);
            self.has_error = false;
            Ok(steps)
        } else {
            self.fail(D54Code::SPEC_GAP);
            Err(D54Code::SPEC_GAP)
        }
    }

    /// 同源对账：章数 + 规范数 + 页数与清单一致。
    pub fn reconcile(&mut self, ch: usize, sp: usize, pg: usize) -> Result<bool, D54Code> {
        if self.chapters.len() == ch && self.specs.len() == sp && self.pages.len() == pg {
            self.last_code = D54Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(D54Code::DOC_DRIFT);
            Err(D54Code::DOC_DRIFT)
        }
    }

    pub fn chapter_count(&self) -> usize {
        self.chapters.len()
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn sop_steps(&self) -> u32 {
        self.sop_steps
    }

    /// 家族管道总评。
    pub fn pipeline_green(&mut self) -> bool {
        self.manual_complete() && self.specs_complete().is_ok() && self.map_2hop()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves54_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves54_visualdocs");
    {
        let d = VisualDocs::new();
        chk(&mut set, "C3654-01", d.chapter_count() == 0 && d.page_count() == 0, "err");
    }
    {
        let mut d = VisualDocs::new();
        d.add_chapter(Chapter::Wallpaper);
        chk(&mut set, "C3654-02", d.chapter_count() == 1 && !d.manual_complete(), "err");
    }
    {
        let mut d = VisualDocs::new();
        for c in Chapter::ALL.iter() {
            d.add_chapter(*c);
        }
        chk(&mut set, "C3654-03", d.manual_complete(), "err");
    }
    {
        let mut d = VisualDocs::new();
        d.add_spec(Spec::Grid);
        let r = d.specs_complete();
        chk(&mut set, "C3654-04", r.err() == Some(D54Code::SPEC_GAP), "err");
    }
    {
        let mut d = VisualDocs::new();
        for s in Spec::ALL.iter() {
            d.add_spec(*s);
        }
        let r = d.specs_complete();
        chk(&mut set, "C3654-05", r == Ok(true), "err");
    }
    {
        let mut d = VisualDocs::new();
        let r = d.add_page(DocPage { id: 1, hops: 1, sample_ok: true });
        chk(&mut set, "C3654-06", r == Ok(1), "err");
    }
    {
        let mut d = VisualDocs::new();
        let r = d.add_page(DocPage { id: 1, hops: 5, sample_ok: true });
        chk(&mut set, "C3654-07", r.err() == Some(D54Code::MAZE), "err");
    }
    {
        let mut d = VisualDocs::new();
        d.add_page(DocPage { id: 1, hops: 2, sample_ok: true });
        chk(&mut set, "C3654-08", d.map_2hop(), "err");
    }
    {
        let mut d = VisualDocs::new();
        d.add_page(DocPage { id: 1, hops: 1, sample_ok: false });
        let r = d.samples_green();
        chk(&mut set, "C3654-09", r.err() == Some(D54Code::SAMPLE_BROKEN), "err");
    }
    {
        let mut d = VisualDocs::new();
        let r = d.sop(5);
        chk(&mut set, "C3654-10", r == Ok(5) && d.sop_steps() == 5, "err");
    }
    {
        let mut d = VisualDocs::new();
        d.add_chapter(Chapter::SharedCore);
        let r = d.reconcile(1, 0, 0);
        chk(&mut set, "C3654-11", r == Ok(true), "err");
    }
    {
        let mut d = VisualDocs::new();
        for c in Chapter::ALL.iter() {
            d.add_chapter(*c);
        }
        for s in Spec::ALL.iter() {
            d.add_spec(*s);
        }
        d.add_page(DocPage { id: 1, hops: 1, sample_ok: true });
        chk(&mut set, "C3654-12", d.pipeline_green(), "err");
    }
    set
}
