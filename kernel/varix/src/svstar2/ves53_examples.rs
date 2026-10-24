//! VE-F3653 · 视觉创作示例（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3653`
//!
//! **职责定位（锚点原文）**：视觉创作示例库（10 个：壁纸 2/动态壁纸 2/
//! 图标套 2/光标 2/锁屏 2——五主题）——三段文档 + 反模式对照
//! （每主题一反例：功耗破门壁纸/越格图标/伪 4K 导出）；示例即测试复述；
//! 完整工程复述；素材许可自证复用（F3617 红线）。
//!
//! **判据（锚点原文六条）**：10 示例、许可自证、完整工程、
//! 反例诊断、家族十用、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 示例失效→CI（复述） | [`X53Code::EXAMPLE_BROKEN`] |
//! | 许可不清→下架（复述红线实测） | [`X53Code::LICENSE_UNCLEAR`] |
//! | 工程缺件→断言（复述） | [`X53Code::PROJECT_INCOMPLETE`] |
//! | 漂移→对账（复述） | [`X53Code::DRIFT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3653 诊断码段（独占 0x65xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct X53Code(pub u16);

impl X53Code {
    /// 示例失效（CI 拦截）。
    pub const EXAMPLE_BROKEN: X53Code = X53Code(0x6501);
    /// 素材许可不清（下架红线）。
    pub const LICENSE_UNCLEAR: X53Code = X53Code(0x6502);
    /// 完整工程缺件。
    pub const PROJECT_INCOMPLETE: X53Code = X53Code(0x6503);
    /// 同源清单漂移。
    pub const DRIFT: X53Code = X53Code(0x6504);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            X53Code::EXAMPLE_BROKEN => "示例失效（CI 拦截）。建议：修复后重挂 CI。",
            X53Code::LICENSE_UNCLEAR => "素材许可不清（下架红线实测）。建议：补许可自证。",
            X53Code::PROJECT_INCOMPLETE => "完整工程缺件。建议：补齐工程五件套。",
            X53Code::DRIFT => "同源清单漂移。建议：对账修正。",
            _ => "未知诊断码。",
        }
    }
}

// ── 五主题 × 反例 ───────────────────────────────────────────────────

/// 示例主题（五主题各 2 例 = 10）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    /// 壁纸。
    Wallpaper,
    /// 动态壁纸。
    MotionWall,
    /// 图标套。
    IconSet,
    /// 光标。
    Cursor,
    /// 锁屏。
    LockScreen,
}

impl Theme {
    pub const ALL: [Theme; 5] = [
        Theme::Wallpaper,
        Theme::MotionWall,
        Theme::IconSet,
        Theme::Cursor,
        Theme::LockScreen,
    ];

    /// 每主题反例名（反模式对照）。
    pub fn counter_example(self) -> &'static str {
        match self {
            Theme::Wallpaper => "功耗破门壁纸",
            Theme::MotionWall => "盖住仍动动态壁纸",
            Theme::IconSet => "越格图标",
            Theme::Cursor => "热点偏移光标",
            Theme::LockScreen => "伪 4K 导出",
        }
    }
}

// ── 示例条目 ────────────────────────────────────────────────────────

/// 示例注册项。
#[derive(Clone, Copy)]
pub struct Example {
    pub id: u32,
    pub theme: Theme,
    /// 许可自证（F3617 红线：必须非空）。
    pub license_ok: bool,
    /// 工程五件套位掩码（bit0..4 齐 = 0x1F）。
    pub project_bits: u8,
    /// CI 通过位。
    pub ci_green: bool,
}

impl Example {
    /// 完整工程审计。
    pub fn project_complete(&self) -> bool {
        self.project_bits == 0x1F
    }
}

// ── 示例库 ──────────────────────────────────────────────────────────

/// 视觉创作示例库。
pub struct ExampleLib {
    items: Vec<Example>,
    counter_logged: Vec<Theme>,
    last_code: X53Code,
    has_error: bool,
}

impl ExampleLib {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            counter_logged: Vec::new(),
            last_code: X53Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> X53Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: X53Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 注册示例；许可不清即红线。
    pub fn register(&mut self, ex: Example) -> Result<usize, X53Code> {
        if !ex.license_ok {
            self.fail(X53Code::LICENSE_UNCLEAR);
            return Err(X53Code::LICENSE_UNCLEAR);
        }
        self.items.push(ex);
        self.last_code = X53Code(0);
        self.has_error = false;
        Ok(self.items.len())
    }

    /// 10 例齐装。
    pub fn ten_examples(&self) -> bool {
        self.items.len() == 10
    }

    /// 五主题各 2 例。
    pub fn theme_balance(&self) -> bool {
        Theme::ALL
            .iter()
            .all(|t| self.items.iter().filter(|e| e.theme == *t).count() == 2)
    }

    /// 完整工程总评。
    pub fn all_projects_complete(&mut self) -> Result<bool, X53Code> {
        if self.items.iter().all(|e| e.project_complete()) {
            self.last_code = X53Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(X53Code::PROJECT_INCOMPLETE);
            Err(X53Code::PROJECT_INCOMPLETE)
        }
    }

    /// 登记反例（每主题一反例）。
    pub fn log_counter(&mut self, theme: Theme) -> usize {
        if !self.counter_logged.contains(&theme) {
            self.counter_logged.push(theme);
        }
        self.counter_logged.len()
    }

    /// 反例覆盖（五主题全登记）。
    pub fn counters_complete(&self) -> bool {
        Theme::ALL.iter().all(|t| self.counter_logged.contains(t))
    }

    /// CI 巡检：任一失效即拦截。
    pub fn ci_sweep(&mut self) -> Result<u32, X53Code> {
        let broken = self.items.iter().filter(|e| !e.ci_green).count();
        if broken > 0 {
            self.fail(X53Code::EXAMPLE_BROKEN);
            return Err(X53Code::EXAMPLE_BROKEN);
        }
        self.last_code = X53Code(0);
        self.has_error = false;
        Ok(self.items.len() as u32)
    }

    /// 同源对账：示例数与登记数一致。
    pub fn reconcile(&mut self, expected: usize) -> Result<bool, X53Code> {
        if self.items.len() == expected {
            self.last_code = X53Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(X53Code::DRIFT);
            Err(X53Code::DRIFT)
        }
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn license_clean(&self) -> bool {
        self.items.iter().all(|e| e.license_ok)
    }

    /// 家族十用总评：10 例 + 反例齐 + 许可齐。
    pub fn family_green(&self) -> bool {
        self.ten_examples() && self.counters_complete() && self.license_clean()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves53_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves53_examples");
    let mk = |id: u32, theme: Theme| Example {
        id,
        theme,
        license_ok: true,
        project_bits: 0x1F,
        ci_green: true,
    };
    {
        let l = ExampleLib::new();
        chk(&mut set, "C3653-01", l.count() == 0 && !l.ten_examples(), "err");
    }
    {
        let mut l = ExampleLib::new();
        let r = l.register(mk(1, Theme::Wallpaper));
        chk(&mut set, "C3653-02", r == Ok(1), "err");
    }
    {
        let mut l = ExampleLib::new();
        let bad = Example { id: 1, theme: Theme::Wallpaper, license_ok: false, project_bits: 0x1F, ci_green: true };
        let r = l.register(bad);
        chk(&mut set, "C3653-03", r.err() == Some(X53Code::LICENSE_UNCLEAR), "err");
    }
    {
        let mut l = ExampleLib::new();
        for (i, t) in Theme::ALL.iter().enumerate() {
            l.register(mk(i as u32 * 2, *t));
            l.register(mk(i as u32 * 2 + 1, *t));
        }
        chk(&mut set, "C3653-04", l.ten_examples() && l.theme_balance(), "err");
    }
    {
        let mut l = ExampleLib::new();
        let weak = Example { id: 1, theme: Theme::IconSet, license_ok: true, project_bits: 0x01, ci_green: true };
        l.register(weak);
        let r = l.all_projects_complete();
        chk(&mut set, "C3653-05", r.err() == Some(X53Code::PROJECT_INCOMPLETE), "err");
    }
    {
        let mut l = ExampleLib::new();
        for t in Theme::ALL.iter() {
            l.log_counter(*t);
        }
        chk(&mut set, "C3653-06", l.counters_complete(), "err");
    }
    {
        let mut l = ExampleLib::new();
        l.register(mk(1, Theme::Wallpaper));
        let r = l.ci_sweep();
        chk(&mut set, "C3653-07", r == Ok(1), "err");
    }
    {
        let mut l = ExampleLib::new();
        let broken = Example { id: 1, theme: Theme::Cursor, license_ok: true, project_bits: 0x1F, ci_green: false };
        l.register(broken);
        let r = l.ci_sweep();
        chk(&mut set, "C3653-08", r.err() == Some(X53Code::EXAMPLE_BROKEN), "err");
    }
    {
        let mut l = ExampleLib::new();
        l.register(mk(1, Theme::Wallpaper));
        let r = l.reconcile(1);
        chk(&mut set, "C3653-09", r == Ok(true), "err");
    }
    {
        let mut l = ExampleLib::new();
        l.register(mk(1, Theme::Wallpaper));
        let r = l.reconcile(5);
        chk(&mut set, "C3653-10", r.err() == Some(X53Code::DRIFT), "err");
    }
    {
        let t = Theme::Wallpaper;
        chk(&mut set, "C3653-11", !t.counter_example().is_empty(), "err");
    }
    {
        let mut l = ExampleLib::new();
        for (i, t) in Theme::ALL.iter().enumerate() {
            l.register(mk(i as u32 * 2, *t));
            l.register(mk(i as u32 * 2 + 1, *t));
            l.log_counter(*t);
        }
        chk(&mut set, "C3653-12", l.family_green(), "err");
    }
    set
}
