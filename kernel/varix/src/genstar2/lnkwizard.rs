//! F451 快捷方式创建向导（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **四类目标创建用例；校验提示；图标继承；完成落位；断链衔接。**
//!
//! 功能定义（主册 I-1 批次三）：右键「新建→快捷方式」向导三步——输入或
//! 浏览目标（文件/文件夹/应用/网页 URL 四类皆可）、命名（默认取目标名+
//! 「- 快捷方式」）、完成（图标落桌面首个空位 F084 网格）；目标合法性
//! 即时校验（路径不存在给黄色提示但允许创建——先建后修的场景尊重用户）；
//! 与 F292 断链自愈衔接（建完目标改名也能救）。
//!
//! 无感标准：建快捷方式三步内完成、四类目标统一处理；路径暂不存在时系统
//! 不装死（允许建、坏了会修）；图标自动取目标图标（无图标的给通用形+角标）。
//!
//! 零堆纪律：定长目标缓冲与定长网格位图，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 快捷方式命名后缀（主册原文：「目标名+『- 快捷方式』」）。
pub const LNK_SUFFIX: &str = " - 快捷方式";
/// 桌面网格列数（F084 网格口径，落位扫描用）。
pub const GRID_COLS: usize = 8;
/// 桌面网格行数（F084 网格口径）。
pub const GRID_ROWS: usize = 6;
/// 向导最大步数（三步：目标/命名/完成）。
pub const WIZARD_STEPS: usize = 3;
/// 断链自愈重试上限（与 F292 衔接：单链接最多重挂 3 次）。
pub const HEAL_RETRY_CAP: u32 = 3;

/// 四类目标（主册原文：文件/文件夹/应用/网页 URL 四类统一处理）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetKind {
    File,
    Folder,
    App,
    Url,
}

impl TargetKind {
    pub fn name(self) -> &'static str {
        match self {
            TargetKind::File => "file",
            TargetKind::Folder => "folder",
            TargetKind::App => "app",
            TargetKind::Url => "url",
        }
    }
}

/// 目标合法性校验结论（黄提示≠拒绝——先建后修场景尊重用户）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValidateHint {
    /// 目标在位，绿。
    Ok,
    /// 目标暂不存在，黄提示但允许创建（主册原文）。
    YellowMissing,
    /// 输入为空，不允许完成。
    Empty,
}

/// 一条快捷方式的落位与继承信息。
#[derive(Clone, Copy, Debug)]
pub struct ShortcutEntry {
    pub kind: TargetKind,
    /// 桌面网格槽位（first_free 扫描所得）。
    pub slot: usize,
    /// 图标继承：true=取目标图标；false=通用形+角标（主册无感标准）。
    pub icon_inherited: bool,
    /// 目标暂缺（黄提示创建）标记——断链自愈 F292 的输入。
    pub target_missing: bool,
    /// 断链自愈已重试次数。
    pub heal_retries: u32,
}

/// 桌面网格占用位图（F084 落位：首个空位）。
pub struct GridMap {
    occupied: [bool; GRID_COLS * GRID_ROWS],
}

impl GridMap {
    pub const fn new() -> Self {
        GridMap {
            occupied: [false; GRID_COLS * GRID_ROWS],
        }
    }

    pub fn occupy(&mut self, slot: usize) -> bool {
        if slot >= self.occupied.len() || self.occupied[slot] {
            return false;
        }
        self.occupied[slot] = true;
        true
    }

    pub fn free(&mut self, slot: usize) {
        if slot < self.occupied.len() {
            self.occupied[slot] = false;
        }
    }

    /// 首个空位（主册判据「落桌面首个空位」）：行优先扫描。
    pub fn first_free(&self) -> Option<usize> {
        (0..self.occupied.len()).find(|&i| !self.occupied[i])
    }
}

// ---------------------------------------------------------------------------
// 向导核心
// ---------------------------------------------------------------------------

/// 三步向导状态机（目标→命名→完成）。
pub struct Lnkwizard {
    step: usize,
    done: bool,
}

impl Lnkwizard {
    pub const fn new() -> Self {
        Lnkwizard { step: 0, done: false }
    }

    pub fn step(&self) -> usize {
        self.step
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// 三步内完成校验（无感标准「三步内完成」：step 推进不超 WIZARD_STEPS）。
    pub fn advance(&mut self) -> bool {
        if self.done || self.step + 1 >= WIZARD_STEPS {
            return false;
        }
        self.step += 1;
        true
    }

    pub fn finish(&mut self) {
        if self.step == WIZARD_STEPS - 1 {
            self.done = true;
        }
    }
}

/// 目标分类（四类统一处理）：URL 前缀 / .exe 扩展名 / 尾路径分隔符。
pub fn classify_target(input: &str) -> TargetKind {
    let bytes = input.as_bytes();
    if input.starts_with("http://") || input.starts_with("https://") {
        return TargetKind::Url;
    }
    if bytes.len() >= 4 && &bytes[bytes.len() - 4..] == b".exe" {
        return TargetKind::App;
    }
    if input.ends_with('/') || input.ends_with('\\') {
        return TargetKind::Folder;
    }
    TargetKind::File
}

/// 目标合法性即时校验（主册：路径不存在给黄色提示但允许创建）。
pub fn validate_target(input: &str, exists: bool) -> ValidateHint {
    if input.is_empty() {
        ValidateHint::Empty
    } else if exists {
        ValidateHint::Ok
    } else {
        ValidateHint::YellowMissing
    }
}

/// 黄提示是否仍允许完成（先建后修场景尊重用户；空输入永不放行）。
pub fn allow_finish(hint: ValidateHint) -> bool {
    !matches!(hint, ValidateHint::Empty)
}

/// 默认命名（主册：目标名+「- 快捷方式」；URL 取域名段）。
pub fn default_name(input: &str, kind: TargetKind) -> &str {
    let base = match kind {
        TargetKind::Url => {
            // 域名段：跳过 scheme，取到第一个 '/'。
            let rest = match input.find("://") {
                Some(p) => &input[p + 3..],
                None => input,
            };
            match rest.find('/') {
                Some(p) => &rest[..p],
                None => rest,
            }
        }
        _ => {
            let stripped = input.trim_end_matches(['/', '\\']);
            match stripped.rfind(['/', '\\']) {
                Some(p) => &stripped[p + 1..],
                None => stripped,
            }
        }
    };
    // 返回静态缓冲不可行（零分配），调用方拼接 LNK_SUFFIX；
    // 此处返回基础名，命名规则由 naming() 承接。
    let _ = base;
    base
}

/// 命名规则校验：基础名非空即合法（后缀由界面层拼接 LNK_SUFFIX）。
pub fn naming_ok(base: &str) -> bool {
    !base.is_empty()
}

/// 图标继承判定（主册：图标自动取目标图标；无图标的给通用形+角标）。
pub fn icon_inherited(kind: TargetKind, has_icon: bool) -> bool {
    has_icon && kind != TargetKind::Url
}

/// 完成落位：首个空位占用成功才允许完成（占满则失败——诚实边界）。
pub fn finish_placement(grid: &mut GridMap) -> Option<usize> {
    let slot = grid.first_free()?;
    if grid.occupy(slot) {
        Some(slot)
    } else {
        None
    }
}

/// 断链自愈衔接（F292）：目标缺失的链接进入自愈队列；重试超上限降级
/// 为「诚实标注断链」而非无限重试。
pub fn heal_tick(entry: &mut ShortcutEntry, target_exists: bool) -> bool {
    if !entry.target_missing {
        return true;
    }
    if target_exists {
        entry.target_missing = false;
        entry.heal_retries = 0;
        return true;
    }
    if entry.heal_retries < HEAL_RETRY_CAP {
        entry.heal_retries += 1;
        return false; // 继续排队重试
    }
    false // 超限：保持断链标注，不再自愈
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_lnkwizard_checks() -> CheckSet {
    let mut cs = CheckSet::new("F451-lnkwizard");
    // 1) 四类目标分类用例。
    cs.add("classify_url", classify_target("https://varix.os/start") == TargetKind::Url, "");
    cs.add("classify_app", classify_target("C:\\tools\\notepad2.exe") == TargetKind::App, "");
    cs.add("classify_folder", classify_target("C:\\work\\") == TargetKind::Folder, "");
    cs.add("classify_file", classify_target("C:\\work\\report.docx") == TargetKind::File, "");
    // 2) 校验提示三态：绿/黄（允许创建）/空（拒绝）。
    cs.add("validate_ok", validate_target("C:\\a.txt", true) == ValidateHint::Ok, "");
    cs.add("validate_yellow_allows", allow_finish(validate_target("C:\\gone.txt", false)), "");
    cs.add("validate_empty_rejected", !allow_finish(validate_target("", false)), "");
    // 3) 默认命名：目标名 + 「- 快捷方式」后缀常量。
    cs.add("naming_base", default_name("C:\\work\\报告.docx", TargetKind::File) == "报告.docx", "");
    cs.add("naming_url_domain", default_name("https://varix.os/docs/x", TargetKind::Url) == "varix.os", "");
    cs.add("naming_suffix_const", LNK_SUFFIX == " - 快捷方式", "");
    // 4) 图标继承：有图标继承；无图标/URL 通用形+角标。
    cs.add("icon_inherit_yes", icon_inherited(TargetKind::App, true), "");
    cs.add("icon_inherit_generic", !icon_inherited(TargetKind::File, false) && !icon_inherited(TargetKind::Url, true), "");
    // 5) 完成落位：首个空位（F084）。
    let mut g = GridMap::new();
    g.occupy(0);
    g.occupy(1);
    cs.add("first_free_skip", finish_placement(&mut g) == Some(2), "");
    // 6) 三步内完成。
    let mut w = Lnkwizard::new();
    let mut in_three = w.advance() && w.advance();
    w.finish();
    in_three &= w.is_done();
    cs.add("three_steps_done", in_three, "");
    // 7) 断链衔接：目标暂缺→建；改名后自愈成功；超限诚实标注。
    let mut e = ShortcutEntry {
        kind: TargetKind::File,
        slot: 2,
        icon_inherited: true,
        target_missing: true,
        heal_retries: 0,
    };
    cs.add("heal_pending_until_target", !heal_tick(&mut e, false), "");
    cs.add("heal_success_on_exist", heal_tick(&mut e, true) && !e.target_missing, "");
    let mut e2 = ShortcutEntry {
        kind: TargetKind::File,
        slot: 3,
        icon_inherited: false,
        target_missing: true,
        heal_retries: HEAL_RETRY_CAP,
    };
    cs.add("heal_cap_honest", !heal_tick(&mut e2, false) && e2.heal_retries == HEAL_RETRY_CAP, "");
    // 8) 网格占满时落位诚实失败（不静默挤位）。
    let mut full = GridMap::new();
    for i in 0..GRID_COLS * GRID_ROWS {
        full.occupy(i);
    }
    cs.add("grid_full_honest_fail", finish_placement(&mut full).is_none(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_target_classes_flow() {
        let mut g = GridMap::new();
        for (input, exists, kind) in [
            ("https://edge.collect/fav", true, TargetKind::Url),
            ("D:\\apps\\tool.exe", true, TargetKind::App),
            ("D:\\projects\\", false, TargetKind::Folder),
            ("D:\\notes.txt", true, TargetKind::File),
        ] {
            let k = classify_target(input);
            assert_eq!(k, kind);
            // 黄提示也允许完成（先建后修）。
            assert!(allow_finish(validate_target(input, exists)));
            let slot = finish_placement(&mut g).expect("slot");
            // 图标继承：有图标且非 URL；URL 恒通用形+角标；无图标目标同。
            assert!(icon_inherited(k, exists) || !exists || k == TargetKind::Url);
            let _ = slot;
        }
    }

    #[test]
    fn yellow_missing_never_blocks_finish() {
        for path in ["C:\\x\\y.txt", "\\\\srv\\share\\", "Z:\\moved\\a.exe"] {
            let hint = validate_target(path, false);
            assert_eq!(hint, ValidateHint::YellowMissing);
            assert!(allow_finish(hint));
        }
    }

    #[test]
    fn heal_retry_cap_is_bounded() {
        let mut e = ShortcutEntry {
            kind: TargetKind::App,
            slot: 0,
            icon_inherited: true,
            target_missing: true,
            heal_retries: 0,
        };
        for _ in 0..HEAL_RETRY_CAP * 10 {
            heal_tick(&mut e, false);
        }
        assert!(e.heal_retries <= HEAL_RETRY_CAP);
    }
}
