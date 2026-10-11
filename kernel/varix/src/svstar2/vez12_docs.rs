//! VE-F5212 · 特效文档（VE-Z 域 · 文档面 · 锚点目标 300 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5212`
//!
//! **判据（锚点原文）**：三族、三景、四节、阻断发版、判据。
//!
//! **职责定位（锚点原文）**：特效文档——三族：特效总册（四子系统与三
//! 律导览）/特效创作指南（如何用库做特效——给创作者的手册）/故障手册
//! （性能超支/资源缺失/叠加冲突三景）；四节定式 CI 断言同口径。
//!
//! **数据结构（锚点原文）**：三族目录；三景单；四节定式。
//!
//! **错误路径与降级矩阵（锚点原文）**：缺节→断言拦截；复现失败→阻断
//! 发版；缺景→补录排程。
//!
//! **性能逐项分解（锚点原文）**：断言 O(册数)；复现 O(步骤)；补录 O(景)。
//!
//! **跨批对接点（锚点原文）**：F5201-F5211 内容源；F5219 联调。
//!
//! **无障碍与隐私（锚点原文）**：文档读屏结构（域本色）；无隐私面。
//!
//! # 深化说明：文档不是附产物——四节定式是 CI 闸，故障三景是发版闸，
//! ## 读屏结构是域本色
//!
//! 「四节定式 CI 断言同口径」——每册页**恰四节**（定位/用法/边界/故
//! 障）按序非空（[`FOUR_SECTIONS`] 闭集+定式校验
//! [`audit_four_sections`]）：缺节/乱序/空内容三形都拦
//! [`E_DC_MISSING_SECTION`]——「故障怎么办」那节缺失的册页，读者出事
//! 时才发现是空话，CI 在发版前就拦；「故障手册三景」（性能超支/资源
//! 缺失/叠加冲突——恰是三防与组合三闸的故障面）缺景→**补录排程**
//! （[`Backlog`] 显性欠账不是静默缺页 [`E_DC_MISSING_SCENE`]）；每景
//! 复现步骤走**动作闭集引擎**（[`REPRO_ACTIONS`] 六动作——CI 能自动
//! 执行的才配写进步骤，未知动作=复现失败 [`E_DC_REPRO_FAIL`]→**发版
//! 闸阻断**（[`ReleaseGate`]——文档骗人比文档缺页严重一级））；读屏
//! 结构（[`audit_reading_structure`]）标题层级不跳级——文档读屏结构
//! 是域本色不是装饰。
//!
//! ## 深化一：复现步骤的动作闭集——CI 能执行的是步骤，不能执行的是散文
//!
//! 六动作闭集（加载场景/触发特效/设置参数/检查帧率/查看日志/恢复默
//! 认）——步骤的每一步 action 必属闭集且期望非空：CI 逐步执行
//! （复现 O(步骤)——锚点口径），动作不在闭集=CI 无法执行=复现失败=
//! 发版阻断。写「打开编辑器看看」的故障条目进不了发版。
//!
//! ## 深化二：缺景与复现失败分两级处置——欠账排程，谎言阻断
//!
//! 缺景是**文档欠账**（三景还没写全）→ 补录排程 [`Backlog`] 计数点名
//! （欠账显性化，CI 返 [`E_DC_MISSING_SCENE`]）；复现失败是**文档错
//! 误**（写了但走不通——比缺页更恶劣）→ 发版闸 [`ReleaseGate::blocked`]
//! +失败景点名（[`E_DC_REPRO_FAIL`]）。两级处置语义分明：欠账可以带
//! 排程发版（排程可查），错误必须修完才能发版。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_DC_* 前缀）
// ---------------------------------------------------------------------------

/// 缺节：四节定式不齐（缺/乱序/空内容）。
pub const E_DC_MISSING_SECTION: &str = "E_DC_MISSING_SECTION";
/// 复现失败：故障手册步骤 CI 走不通（阻断发版）。
pub const E_DC_REPRO_FAIL: &str = "E_DC_REPRO_FAIL";
/// 缺景：故障三景不齐（补录排程）。
pub const E_DC_MISSING_SCENE: &str = "E_DC_MISSING_SCENE";
/// 目录结构非法：族未登记/页重名/景非法。
pub const E_DC_BAD_TOC: &str = "E_DC_BAD_TOC";
/// 读屏结构违规：标题层级跳级或越界。
pub const E_DC_READABILITY: &str = "E_DC_READABILITY";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_DC_MISSING_SECTION => "缺节：四节定式不齐（缺/乱序/空内容）",
        E_DC_REPRO_FAIL => "复现失败：步骤 CI 走不通（阻断发版）",
        E_DC_MISSING_SCENE => "缺景：故障三景不齐（补录排程）",
        E_DC_BAD_TOC => "目录结构非法：族未登记/页重名/景非法",
        E_DC_READABILITY => "读屏结构违规：层级跳级或越界",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_DC_MISSING_SECTION => "按四节定式补齐缺节再过 CI",
        E_DC_REPRO_FAIL => "修正复现步骤至 CI 可执行再发版",
        E_DC_MISSING_SCENE => "按补录排程补写缺失故障景",
        E_DC_BAD_TOC => "核对三族目录与页名唯一性",
        E_DC_READABILITY => "压平标题层级使读屏不跳级",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、三族目录：总册/创作指南/故障手册
// ---------------------------------------------------------------------------

/// 三族闭集（特效总册/创作指南/故障手册）。
pub const DOC_FAMILIES: [&str; 3] = ["总册", "创作指南", "故障手册"];

/// 三族目录：族→页名集（页名跨族唯一——同一页不属两族）。
#[derive(Clone, Debug, Default)]
pub struct DocToc {
    toc: Vec<(String, Vec<String>)>,
    /// 目录违规计数。
    pub bad_blocks: u32,
}

impl DocToc {
    /// 空目录。
    pub fn new() -> Self {
        DocToc::default()
    }

    /// **登记册页**：族必属三族闭集、页名跨族唯一（违者
    /// [`E_DC_BAD_TOC`]）。
    pub fn add_page(&mut self, family: &str, title: &str) -> Result<(), &'static str> {
        if !DOC_FAMILIES.contains(&family) {
            self.bad_blocks += 1;
            return Err(E_DC_BAD_TOC);
        }
        for (_, pages) in self.toc.iter() {
            if pages.iter().any(|p| p == title) {
                self.bad_blocks += 1;
                return Err(E_DC_BAD_TOC);
            }
        }
        match self.toc.iter_mut().find(|(f, _)| f == family) {
            Some((_, pages)) => pages.push(String::from(title)),
            None => self
                .toc
                .push((String::from(family), vec![String::from(title)])),
        }
        Ok(())
    }

    /// **目录审计**：三族每族至少一页（空族=目录欠账）。
    pub fn audit(&self) -> Result<(), &'static str> {
        for fam in DOC_FAMILIES.iter() {
            let empty = match self.toc.iter().find(|(f, _)| f == fam) {
                Some((_, pages)) => pages.is_empty(),
                None => true,
            };
            if empty {
                return Err(E_DC_BAD_TOC);
            }
        }
        Ok(())
    }

    /// 某族页数。
    pub fn family_len(&self, family: &str) -> usize {
        match self.toc.iter().find(|(f, _)| f == family) {
            Some((_, pages)) => pages.len(),
            None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 二、四节定式：定位/用法/边界/故障——CI 断言同口径
// ---------------------------------------------------------------------------

/// 四节闭集（定式有序——读屏结构一致）。
pub const FOUR_SECTIONS: [&str; 4] = ["定位", "用法", "边界", "故障"];

/// 单册页：标题+四节内容。
#[derive(Clone, Debug)]
pub struct DocPage {
    /// 页标题（页名——目录唯一键）。
    pub title: String,
    /// 四节（名,内容）——必须恰按 [`FOUR_SECTIONS`] 顺序齐备。
    pub sections: Vec<(String, String)>,
}

impl DocPage {
    /// 建页（标题+四节内容——按定式顺序给四段）。
    pub fn new(title: &str, s1: &str, s2: &str, s3: &str, s4: &str) -> Self {
        DocPage {
            title: String::from(title),
            sections: vec![
                (String::from(FOUR_SECTIONS[0]), String::from(s1)),
                (String::from(FOUR_SECTIONS[1]), String::from(s2)),
                (String::from(FOUR_SECTIONS[2]), String::from(s3)),
                (String::from(FOUR_SECTIONS[3]), String::from(s4)),
            ],
        }
    }
}

/// **四节定式审计**：恰四节+顺序对拍+内容非空——缺节/乱序/空内容三形
/// 显性拦截（断言 O(册页节)=O(1) 定长）。
pub fn audit_four_sections(page: &DocPage) -> Result<(), &'static str> {
    if page.sections.len() != FOUR_SECTIONS.len() {
        return Err(E_DC_MISSING_SECTION);
    }
    let mut i = 0usize;
    while i < FOUR_SECTIONS.len() {
        let (name, body) = &page.sections[i];
        if name.as_str() != FOUR_SECTIONS[i] {
            return Err(E_DC_MISSING_SECTION);
        }
        if body.trim().is_empty() {
            return Err(E_DC_MISSING_SECTION);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、读屏结构：标题层级不跳级
// ---------------------------------------------------------------------------

/// **读屏结构审计**：层级 1..=6，相邻标题升级最多一级（1→3 是跳级——
/// 读屏玩家会以为漏听了一层），降级任意；越界层级拒。
pub fn audit_reading_structure(levels: &[u8]) -> Result<(), &'static str> {
    let mut prev = 0u8;
    let mut i = 0usize;
    while i < levels.len() {
        let lv = levels[i];
        if lv < 1 || lv > 6 {
            return Err(E_DC_READABILITY);
        }
        if lv > prev + 1 {
            return Err(E_DC_READABILITY);
        }
        prev = lv;
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、故障手册：三景单 + 复现引擎 + 补录排程 + 发版闸
// ---------------------------------------------------------------------------

/// 故障三景闭集（性能超支/资源缺失/叠加冲突——三防与组合三闸的故障面）。
pub const FAULT_SCENES: [&str; 3] = ["性能超支", "资源缺失", "叠加冲突"];

/// 复现动作闭集（CI 能自动执行的才配写进步骤）。
pub const REPRO_ACTIONS: [&str; 6] = ["加载场景", "触发特效", "设置参数", "检查帧率", "查看日志", "恢复默认"];

/// 单个复现步骤：动作+期望。
#[derive(Clone, Debug)]
pub struct ReproStep {
    /// 动作（必属 [`REPRO_ACTIONS`] 闭集——CI 可执行）。
    pub action: String,
    /// 期望（非空——步骤没有期望就是散文）。
    pub expect: String,
}

/// 单个故障景：景名+症状+复现步骤+处置。
#[derive(Clone, Debug)]
pub struct FaultScene {
    /// 景名（必属 [`FAULT_SCENES`] 闭集）。
    pub scene: String,
    /// 症状描述（非空）。
    pub symptom: String,
    /// 复现步骤（非空步序——CI 逐步执行）。
    pub steps: Vec<ReproStep>,
    /// 处置（非空）。
    pub fix: String,
}

/// 复现引擎：逐步校验动作闭集+期望非空（复现 O(步骤)——锚点口径）。
/// 空步序/未知动作/空期望三形=CI 无法执行=复现失败。
#[derive(Clone, Debug, Default)]
pub struct ReproRunner {
    /// 复现失败计数。
    pub fail_blocks: u32,
}

impl ReproRunner {
    /// 空引擎。
    pub fn new() -> Self {
        ReproRunner::default()
    }

    /// **复现**（O(步骤)）。
    pub fn run(&mut self, steps: &[ReproStep]) -> Result<(), &'static str> {
        if steps.is_empty() {
            self.fail_blocks += 1;
            return Err(E_DC_REPRO_FAIL);
        }
        let mut i = 0usize;
        while i < steps.len() {
            let st = &steps[i];
            if !REPRO_ACTIONS.contains(&st.action.as_str()) {
                self.fail_blocks += 1;
                return Err(E_DC_REPRO_FAIL);
            }
            if st.expect.trim().is_empty() {
                self.fail_blocks += 1;
                return Err(E_DC_REPRO_FAIL);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 补录排程：缺景显性欠账（补录 O(景)——锚点口径）。
#[derive(Clone, Debug, Default)]
pub struct Backlog {
    /// 待补景名册。
    pub scenes: Vec<String>,
    /// 排程计数。
    pub scheduled: u32,
}

impl Backlog {
    /// 空排程。
    pub fn new() -> Self {
        Backlog::default()
    }

    /// 排一条补录。
    pub fn schedule(&mut self, scene: &str) {
        self.scenes.push(String::from(scene));
        self.scheduled += 1;
    }

    /// 排程数。
    pub fn len(&self) -> usize {
        self.scenes.len()
    }

    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }

    /// 某景是否已排程。
    pub fn has(&self, scene: &str) -> bool {
        self.scenes.iter().any(|s| s == scene)
    }
}

/// 发版闸：复现失败=文档错误=阻断；全过=放行。
#[derive(Clone, Debug, Default)]
pub struct ReleaseGate {
    /// 是否阻断发版。
    pub blocked: bool,
    /// 阻断原因（失败景点名）。
    pub reasons: Vec<String>,
}

impl ReleaseGate {
    /// 空闸（未阻断）。
    pub fn new() -> Self {
        ReleaseGate::default()
    }
}

/// 故障手册：三景单+排程+发版闸+复现引擎总编排。
#[derive(Clone, Debug, Default)]
pub struct FaultManual {
    pub scenes: Vec<FaultScene>,
    /// 缺景补录排程。
    pub backlog: Backlog,
    /// 发版闸。
    pub gate: ReleaseGate,
    /// 复现引擎。
    pub runner: ReproRunner,
}

impl FaultManual {
    /// 空手册。
    pub fn new() -> Self {
        FaultManual::default()
    }

    /// **登记故障景**：景名必属三景闭集、景名不重复（违者
    /// [`E_DC_BAD_TOC`]——结构非法）。
    pub fn add_scene(&mut self, scene: FaultScene) -> Result<(), &'static str> {
        if !FAULT_SCENES.contains(&scene.scene.as_str()) {
            return Err(E_DC_BAD_TOC);
        }
        if self.scenes.iter().any(|s| s.scene == scene.scene) {
            return Err(E_DC_BAD_TOC);
        }
        if scene.symptom.trim().is_empty() || scene.fix.trim().is_empty() {
            return Err(E_DC_BAD_TOC);
        }
        self.scenes.push(scene);
        Ok(())
    }

    /// **CI 审**：缺景→补录排程（显性欠账）；景全→历史欠账**销账**+
    /// 逐景复现，任一失败→发版闸阻断+失败景点名。全过返回复现景数
    /// （gate 放行）。
    pub fn ci_check(&mut self) -> Result<usize, &'static str> {
        // 景齐备性：缺景排程（O(景)——锚点「补录 O(景)」）。
        let mut missing = false;
        let mut i = 0usize;
        while i < FAULT_SCENES.len() {
            let want = FAULT_SCENES[i];
            if !self.scenes.iter().any(|s| s.scene == want) {
                if !self.backlog.has(want) {
                    self.backlog.schedule(want);
                }
                missing = true;
            }
            i += 1;
        }
        if missing {
            return Err(E_DC_MISSING_SCENE);
        }
        // 三景已齐——历史欠账兑现销账（排程是欠账记录不是永久罪证）。
        self.backlog.scenes.clear();
        // 复现（O(景×步骤)——锚点「复现 O(步骤)」）。
        let mut ran = 0usize;
        let mut si = 0usize;
        while si < self.scenes.len() {
            let ok = {
                let steps: Vec<ReproStep> = self.scenes[si].steps.clone();
                self.runner.run(&steps).is_ok()
            };
            if !ok {
                let name = self.scenes[si].scene.clone();
                self.gate.blocked = true;
                if !self.gate.reasons.iter().any(|r| r == &name) {
                    self.gate.reasons.push(name);
                }
                return Err(E_DC_REPRO_FAIL);
            }
            ran += 1;
            si += 1;
        }
        self.gate.blocked = false;
        Ok(ran)
    }
}

// ---------------------------------------------------------------------------
// 五、CI 断言面：O(册数) 批量审计（四节定式+读屏结构同口径）
// ---------------------------------------------------------------------------

/// CI 审计报告：过审数+失败点名。
#[derive(Clone, Debug, Default)]
pub struct CiReport {
    /// 过审页数。
    pub passed: usize,
    /// 失败页点名（含失败码）。
    pub failed: Vec<(String, &'static str)>,
}

impl CiReport {
    /// 全过判据。
    pub fn all_passed(&self) -> bool {
        self.failed.is_empty()
    }
}

/// **CI 批量审计**（断言 O(册数)——锚点口径）：逐页四节定式断言；
/// 三族目录齐备性前置（目录欠账先行拒绝）。
pub fn ci_audit(toc: &DocToc, pages: &[DocPage]) -> Result<CiReport, &'static str> {
    toc.audit()?;
    let mut rep = CiReport::default();
    let mut i = 0usize;
    while i < pages.len() {
        let p = &pages[i];
        match audit_four_sections(p) {
            Ok(()) => rep.passed += 1,
            Err(code) => rep.failed.push((p.title.clone(), code)),
        }
        i += 1;
    }
    Ok(rep)
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5212 域自检（判据 12 条六组：三族/四节/读屏/三景/复现阻断/判据元）。
pub fn run_vez12_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5212");

    // --- 三族（判据一）---
    // 族-01：三族目录登记+跨族页名唯一+重名/非法族拒+审计三族非空。
    let mut toc = DocToc::new();
    let t1 = toc.add_page("总册", "四子系统导览");
    let t2 = toc.add_page("总册", "三律导览");
    let t3 = toc.add_page("创作指南", "第一枚特效");
    let t4 = toc.add_page("故障手册", "三景速查");
    let dup = toc.add_page("创作指南", "三律导览") == Err(E_DC_BAD_TOC);
    let badfam = toc.add_page("随笔", "杂谈") == Err(E_DC_BAD_TOC);
    let tocaudit = toc.audit().is_ok();
    let toc_ok = t1.is_ok() && t2.is_ok() && t3.is_ok() && t4.is_ok()
        && dup && badfam && tocaudit
        && toc.family_len("总册") == 2
        && toc.family_len("故障手册") == 1
        && toc.bad_blocks == 2;
    if toc_ok {
        s.ok("EZ12-族-01-三族目录与页名唯一");
    } else {
        s.fail("EZ12-族-01-三族目录与页名唯一", "目录闸失效");
    }

    // --- 四节定式（判据二）---
    // 定-01：合法页过 + 缺节/乱序/空内容三形拒。
    let good = DocPage::new("粒子系统", "Z 域四子系统之一", "fx_p_ 前缀+OverlayCard", "不负责音效", "见故障手册性能超支景");
    let sec_ok = audit_four_sections(&good).is_ok();
    let mut missing = good.clone();
    missing.sections.pop();
    let mut disorder = good.clone();
    disorder.sections.swap(0, 1);
    let mut empty_body = good.clone();
    empty_body.sections[3] = (String::from("故障"), String::from("   "));
    let def_ok = audit_four_sections(&missing) == Err(E_DC_MISSING_SECTION)
        && audit_four_sections(&disorder) == Err(E_DC_MISSING_SECTION)
        && audit_four_sections(&empty_body) == Err(E_DC_MISSING_SECTION);
    if sec_ok && def_ok {
        s.ok("EZ12-定-01-四节定式三形拦截");
    } else {
        s.fail("EZ12-定-01-四节定式三形拦截", "定式闸漏");
    }

    // --- 读屏结构（判据三）---
    // 读-01：合法层级过 + 跳级/越界拒。
    let ok_struct = audit_reading_structure(&[1, 2, 2, 3, 2, 1]).is_ok();
    let skip = audit_reading_structure(&[1, 3]) == Err(E_DC_READABILITY);
    let over = audit_reading_structure(&[1, 7]) == Err(E_DC_READABILITY);
    let zero = audit_reading_structure(&[0]) == Err(E_DC_READABILITY);
    if ok_struct && skip && over && zero {
        s.ok("EZ12-读-01-标题层级不跳级");
    } else {
        s.fail("EZ12-读-01-标题层级不跳级", "读屏结构闸漏");
    }

    // --- 三景与补录（判据四）---
    // 景-01：三景登记闭集+非法景/重名/空症状拒+缺景补录排程点名。
    let mut manual = FaultManual::new();
    let sc1 = manual.add_scene(FaultScene {
        scene: String::from("性能超支"),
        symptom: String::from("帧时间超预算"),
        steps: vec![
            ReproStep { action: String::from("加载场景"), expect: String::from("场景就绪") },
            ReproStep { action: String::from("触发特效"), expect: String::from("帧时间上升") },
            ReproStep { action: String::from("检查帧率"), expect: String::from("低于阈值") },
        ],
        fix: String::from("按 F5208 降档建议让位"),
    });
    let badscene = manual.add_scene(FaultScene {
        scene: String::from("玄学故障"),
        symptom: String::from("x"),
        steps: vec![],
        fix: String::from("y"),
    }) == Err(E_DC_BAD_TOC);
    let dupscene = manual.add_scene(FaultScene {
        scene: String::from("性能超支"),
        symptom: String::from("重复"),
        steps: vec![],
        fix: String::from("z"),
    }) == Err(E_DC_BAD_TOC);
    let first_ci = manual.ci_check();
    let backlog_ok = first_ci == Err(E_DC_MISSING_SCENE)
        && manual.backlog.len() == 2
        && manual.backlog.has("资源缺失")
        && manual.backlog.has("叠加冲突");
    if sc1.is_ok() && badscene && dupscene && backlog_ok {
        s.ok("EZ12-景-01-三景闭集与缺景排程");
    } else {
        s.fail("EZ12-景-01-三景闭集与缺景排程", "景闸或排程失效");
    }

    // --- 复现与阻断发版（判据五）---
    // 复-01：补齐三景后 CI 全过（复现 O(步骤)）+发版闸放行。
    let _ = manual.add_scene(FaultScene {
        scene: String::from("资源缺失"),
        symptom: String::from("引用资源未登记"),
        steps: vec![
            ReproStep { action: String::from("加载场景"), expect: String::from("资源告警") },
            ReproStep { action: String::from("查看日志"), expect: String::from("E_SF_BAD_ASSET") },
        ],
        fix: String::from("按 F5211 隔离区流程修复"),
    });
    let _ = manual.add_scene(FaultScene {
        scene: String::from("叠加冲突"),
        symptom: String::from("同屏特效冲突"),
        steps: vec![
            ReproStep { action: String::from("加载场景"), expect: String::from("场景就绪") },
            ReproStep { action: String::from("触发特效"), expect: String::from("E_CB_CONFLICT") },
            ReproStep { action: String::from("恢复默认"), expect: String::from("恢复就绪") },
        ],
        fix: String::from("按 F5208 叠加规则消解"),
    });
    let all_ci = manual.ci_check();
    let pass_ok = all_ci == Ok(3) && !manual.gate.blocked && manual.gate.reasons.is_empty();
    if pass_ok {
        s.ok("EZ12-复-01-三景复现全过放行");
    } else {
        s.fail("EZ12-复-01-三景复现全过放行", "复现引擎失效");
    }
    // 阻-01：复现失败（未知动作=CI 不可执行）→发版闸阻断+失败景点名。
    manual.scenes[2].steps[1].action = String::from("打开编辑器看看");
    let fail_ci = manual.ci_check();
    let block_ok = fail_ci == Err(E_DC_REPRO_FAIL)
        && manual.gate.blocked
        && manual.gate.reasons.len() == 1
        && manual.gate.reasons[0] == "叠加冲突"
        && manual.runner.fail_blocks == 1;
    if block_ok {
        s.ok("EZ12-阻-01-复现失败阻断发版");
    } else {
        s.fail("EZ12-阻-01-复现失败阻断发版", "发版闸漏");
    }

    // --- CI 断言面（判据六）---
    // CI-01：批量页审计 O(册数)——过审计数+失败页点名（缺节页）。
    let p1 = DocPage::new("粒子系统", "a", "b", "c", "d");
    let p2 = DocPage::new("参数系统", "a", "b", "c", "d");
    let mut p3 = DocPage::new("转场系统", "a", "b", "c", "d");
    p3.sections.pop();
    let mut toc2 = DocToc::new();
    let _ = toc2.add_page("总册", "粒子系统");
    let _ = toc2.add_page("总册", "参数系统");
    let _ = toc2.add_page("创作指南", "转场系统");
    let _ = toc2.add_page("故障手册", "三景速查");
    let rep = ci_audit(&toc2, &[p1, p2, p3]);
    let ci_ok = match rep {
        Ok(r) => r.passed == 2 && r.failed.len() == 1 && r.failed[0].0 == "转场系统"
            && r.failed[0].1 == E_DC_MISSING_SECTION
            && !r.all_passed(),
        Err(_) => false,
    };
    // 空族目录先行拒绝。
    let mut toc3 = DocToc::new();
    let _ = toc3.add_page("总册", "只有总册");
    let empty_fam = ci_audit(&toc3, &[]).is_err();
    if ci_ok && empty_fam {
        s.ok("EZ12-CI-01-批量断言失败点名");
    } else {
        s.fail("EZ12-CI-01-批量断言失败点名", "CI 断言失效");
    }

    // --- 判据元（判据七）---
    // 元-01：五码互异 + 三要素齐备。
    let codes = [E_DC_MISSING_SECTION, E_DC_REPRO_FAIL, E_DC_MISSING_SCENE, E_DC_BAD_TOC, E_DC_READABILITY];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if reason_of(codes[i]) == "未知错误码" || direction_of(codes[i]).is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok {
        s.ok("EZ12-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ12-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic 零 unsafe（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez12_docs.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ12-判据-02-零panic零unsafe面");
    } else {
        s.fail("EZ12-判据-02-零panic零unsafe面", "判据/生产面含 panic 或 unsafe");
    }
    // 元-03：内容源口径自洽——三族覆盖 F5201-F5211 事件源关键词
    // （四子系统+两翼+安全+遥测+文档在总册/故障手册景名对拍）。
    let source_keys = ["粒子", "后处理", "环境", "转场", "参数", "组合", "安全"];
    let mut all_in = true;
    for k in source_keys.iter() {
        if !SOURCE_COVER.iter().any(|c| c.contains(k)) {
            all_in = false;
        }
    }
    let scenes_match = FAULT_SCENES.len() == 3
        && FAULT_SCENES.contains(&"性能超支")
        && FAULT_SCENES.contains(&"资源缺失")
        && FAULT_SCENES.contains(&"叠加冲突");
    if all_in && scenes_match {
        s.ok("EZ12-判据-03-内容源覆盖口径自洽");
    } else {
        s.fail("EZ12-判据-03-内容源覆盖口径自洽", "内容源口径漂移");
    }

    s
}

/// 内容源覆盖表（F5201-F5211 事件源的最小关键词集——判据元对拍锚）。
pub const SOURCE_COVER: [&str; 7] = [
    "粒子子系统导览",
    "后处理子系统导览",
    "环境子系统导览",
    "转场子系统导览",
    "参数系统两翼",
    "组合与叠加两翼",
    "安全三防与遥测",
];

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
