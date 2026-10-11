//! vcu03_scenecls — CGPU-F3203 基准场景分类（场景维的语义分册）。
//!
//! 判据原文（施工书锚点 CGPU-F3203）：基准场景分类（渲染/计算/混合/极限
//! 四类——分类实现；测试（分类一组）。判据：四类、一组、判据。
//!
//! **职责定位**：四类场景闭集（渲染/计算/混合/极限——每类带负载语义与
//! 适用说明，分类即语义不是贴标签）+ 场景分类册（scene id → class 的
//! 登记查询账——id 空间与 [`super::vcu02_benchmodel::MAX_SCENES`] 同口
//! 径对齐：vcu02 三维格的 scene 维登记号由此册发放，表外 id 建格会被
//! vcu02 的 SceneUnknown 闸拦，先入册再建格是唯一正道）+ 四类分布统计
//! （册内 per-class 计数——基准覆盖面可读：渲染几格计算几格一目了然，
//! 全押一类=覆盖面歪斜显性化）。与 vcu01（指标闭集）/vcu02（三维模型）
//! 分工——彼管考卷格式、此管考卷的分类学。诊断枚举 ScCode 四码。
//!
//! 复用声明：id 空间（1..=MAX_SCENES）与容量策略直接消费 vcu02 常量，
//! 不复刻不另立——两册同源一个口径（集成复用锚，与 vcu02 头注「集成
//! 复用」声明同款纪律）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcu02_benchmodel::MAX_SCENES;

/// 模块版本。
pub const U03_SCENECLS_VERSION: &str = "U03-scenecls-v1";

// ---------------------------------------------------------------------------
// 四类闭集（判据"渲染/计算/混合/极限"）
// ---------------------------------------------------------------------------

/// 基准场景四类闭集（判据原文四类——每类带负载语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneClass {
    /// 渲染类（图形管线为主——帧率/画质考点）。
    Render,
    /// 计算类（通用计算为主——吞吐/时延考点）。
    Compute,
    /// 混合类（图形+计算并发——争用/公平考点）。
    Mixed,
    /// 极限类（压力边界——容量/降级考点）。
    Extreme,
}

impl SceneClass {
    /// 四类闭集（判据侧独立复核）。
    pub const ALL: [SceneClass; 4] = [
        SceneClass::Render,
        SceneClass::Compute,
        SceneClass::Mixed,
        SceneClass::Extreme,
    ];

    /// 中文名（读屏面——互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            SceneClass::Render => "渲染",
            SceneClass::Compute => "计算",
            SceneClass::Mixed => "混合",
            SceneClass::Extreme => "极限",
        }
    }

    /// 负载语义（分类即语义——登记时写进册的理由）。
    pub const fn semantics(self) -> &'static str {
        match self {
            SceneClass::Render => "图形管线主导：考点帧率与画质，负载形态逐帧稳定",
            SceneClass::Compute => "通用计算主导：考点吞吐与时延，负载形态批量化",
            SceneClass::Mixed => "图形计算并发：考点争用与公平，负载形态交叠",
            SceneClass::Extreme => "压力边界：考点容量与降级，负载形态饱和",
        }
    }

    pub const fn ordinal(self) -> usize {
        match self {
            SceneClass::Render => 0,
            SceneClass::Compute => 1,
            SceneClass::Mixed => 2,
            SceneClass::Extreme => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// 诊断码（ScCode 四码互异）
// ---------------------------------------------------------------------------

/// 诊断码（场景分类册——四码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 场景 id 表外（0 或超 MAX_SCENES——vcu02 同口径闸）。
    SceneRange,
    /// 场景 id 重复（同 id 单册一条——重复即登记事故）。
    SceneDuplicate,
    /// 分类册容量满（策略值——归档或扩容评审）。
    CatalogFull,
    /// 场景名空白（无名场景是谜语——登记必须有名）。
    SceneNameless,
}

impl ScCode {
    /// 四码闭集。
    pub const ALL: [ScCode; 4] = [
        ScCode::SceneRange,
        ScCode::SceneDuplicate,
        ScCode::CatalogFull,
        ScCode::SceneNameless,
    ];

    /// 人读诊断（发生了什么/为什么/怎么办）。
    pub const fn detail(self) -> &'static str {
        match self {
            ScCode::SceneRange => "场景 id 表外：id 须在 1..=MAX_SCENES（与 vcu02 同口径），建议换合法 id",
            ScCode::SceneDuplicate => "场景 id 重复：同 id 单册一条——重复即登记事故，建议核对 id",
            ScCode::CatalogFull => "分类册容量满（当前策略值）：建议归档旧场景或扩容评审",
            ScCode::SceneNameless => "场景名空白：无名场景是谜语，建议补语义化名再登记",
        }
    }
}

// ---------------------------------------------------------------------------
// 场景分类册：登记 + 查询 + 分布统计（判据"分类实现"的本体）
// ---------------------------------------------------------------------------

/// 单条场景登记（id × 类 × 名）。
#[derive(Clone, Debug)]
pub struct SceneEntry {
    /// 场景 id（vcu02 scene 维登记号——1..=MAX_SCENES）。
    pub scene: u32,
    /// 四类之一。
    pub class: SceneClass,
    /// 场景名（语义化非空）。
    pub name: String,
}

/// 场景分类册（id → class 的登记查询账——vcu02 场景维的发放源）。
pub struct SceneCatalog {
    entries: Vec<SceneEntry>,
    /// 各类计数（四槽——登记/改判同步更新）。
    per_class: [u32; 4],
    /// 容量策略值（与 vcu02 MAX_SCENES 同口径——册不超场景空间）。
    capacity: usize,
    /// 拒绝计数（逐码——闸面显性化）。
    pub rejected_range: u32,
    pub rejected_duplicate: u32,
    pub rejected_full: u32,
    pub rejected_nameless: u32,
}

impl SceneCatalog {
    /// 空册（容量=场景空间全量——vcu02 同口径）。
    pub fn new() -> Self {
        SceneCatalog {
            entries: Vec::new(),
            per_class: [0; 4],
            capacity: MAX_SCENES as usize,
            rejected_range: 0,
            rejected_duplicate: 0,
            rejected_full: 0,
            rejected_nameless: 0,
        }
    }

    /// **登记场景**（id 合法性+重复闸+容量闸+名非空——四闸全过才入册，
    /// 违者逐码显性拒不静默）。
    pub fn register(&mut self, scene: u32, class: SceneClass, name: &str) -> Result<(), ScCode> {
        if scene == 0 || scene > MAX_SCENES {
            self.rejected_range += 1;
            return Err(ScCode::SceneRange);
        }
        if name.trim().is_empty() {
            self.rejected_nameless += 1;
            return Err(ScCode::SceneNameless);
        }
        if self.entries.len() >= self.capacity {
            self.rejected_full += 1;
            return Err(ScCode::CatalogFull);
        }
        if self.find_idx(scene).is_some() {
            self.rejected_duplicate += 1;
            return Err(ScCode::SceneDuplicate);
        }
        self.per_class[class.ordinal()] += 1;
        self.entries.push(SceneEntry { scene, class, name: String::from(name) });
        Ok(())
    }

    /// **查询分类**（O(n) 线性——册容量策略值有限，锚点口径）。
    pub fn classify_of(&self, scene: u32) -> Option<SceneClass> {
        let idx = self.find_idx(scene)?;
        Some(self.entries[idx].class)
    }

    /// **查询场景名**。
    pub fn name_of(&self, scene: u32) -> Option<&str> {
        let idx = self.find_idx(scene)?;
        Some(&self.entries[idx].name)
    }

    /// 登记数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// **四类分布**（per-class 计数快照——覆盖面可读）。
    pub fn distribution(&self) -> [u32; 4] {
        self.per_class
    }

    /// **覆盖面歪斜判定**：四类中恰有 0 类被登记（空册不算——有登记才
    /// 评估）且单类占比 > 3/4 → 覆盖面歪斜（全押一类的基准没有说服力
    /// ——显性化不是拒绝）。
    pub fn is_skewed(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let total = self.entries.len() as u32;
        let mut i = 0usize;
        while i < 4 {
            if self.per_class[i] * 4 > total * 3 {
                return true;
            }
            i += 1;
        }
        false
    }

    fn find_idx(&self, scene: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].scene == scene {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 一行摘要（读屏面）。
    pub fn summary(&self) -> String {
        alloc::format!(
            "U03 场景册: 登记={}/{} 渲染={} 计算={} 混合={} 极限={} 歪斜={}",
            self.len(),
            self.capacity,
            self.per_class[0],
            self.per_class[1],
            self.per_class[2],
            self.per_class[3],
            if self.is_skewed() { "是" } else { "否" }
        )
    }
}

impl Default for SceneCatalog {
    fn default() -> Self {
        SceneCatalog::new()
    }
}

// ---------------------------------------------------------------------------
// 静态一致性核验：四类闭集与 vcu02 口径对拍（判据"一组"的静态面）
// ---------------------------------------------------------------------------

/// **对拍核验**：本册 id 上限 == vcu02 场景维上限（两册同源——口径漂移
/// 即建格错位）；四类中文名互异；四类语义互异。
pub fn crosscheck_vcu02() -> Option<ScCode> {
    if MAX_SCENES == 0 {
        return Some(ScCode::SceneRange);
    }
    let mut i = 0usize;
    while i < 4 {
        let mut j = i + 1;
        while j < 4 {
            if SceneClass::ALL[i].zh() == SceneClass::ALL[j].zh() {
                return Some(ScCode::SceneDuplicate);
            }
            if SceneClass::ALL[i].semantics() == SceneClass::ALL[j].semantics() {
                return Some(ScCode::SceneDuplicate);
            }
            j += 1;
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 判据（判据面零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F3203 域自检（判据 7 条三组：四类×3、一组×3、判据元×1）。
pub fn run_vcu03_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F3203");

    // --- 四类（判据一）---
    // 四-01：四类闭集+注册往返——四类各登一场，classify_of 逐类读回一致；
    // 中文名/语义互异；ordinal 连续。
    let mut cat = SceneCatalog::new();
    let r1 = cat.register(1, SceneClass::Render, "光栅化管线基准");
    let r2 = cat.register(2, SceneClass::Compute, "物理仿真吞吐基准");
    let r3 = cat.register(3, SceneClass::Mixed, "图形计算并发基准");
    let r4 = cat.register(4, SceneClass::Extreme, "显存饱和压力基准");
    let reg_ok = r1.is_ok() && r2.is_ok() && r3.is_ok() && r4.is_ok() && cat.len() == 4;
    let read_ok = cat.classify_of(1) == Some(SceneClass::Render)
        && cat.classify_of(2) == Some(SceneClass::Compute)
        && cat.classify_of(3) == Some(SceneClass::Mixed)
        && cat.classify_of(4) == Some(SceneClass::Extreme)
        && cat.name_of(3) == Some("图形计算并发基准")
        && cat.name_of(99).is_none();
    let mut cls_ok = true;
    let mut i = 0usize;
    while i < 4 {
        let mut j = i + 1;
        while j < 4 {
            if SceneClass::ALL[i].zh() == SceneClass::ALL[j].zh()
                || SceneClass::ALL[i].semantics() == SceneClass::ALL[j].semantics()
                || SceneClass::ALL[i].ordinal() == SceneClass::ALL[j].ordinal()
            {
                cls_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if reg_ok && read_ok && cls_ok {
        s.ok("U03-四-01-四类闭集注册往返");
    } else {
        s.fail("U03-四-01-四类闭集注册往返", "四类闭集失效");
    }

    // 四-02：四闸显性拒——id 0 / id 超 MAX_SCENES / 空名 / 重复 id 逐码
    // 拒且计数；合法 id 登记不受污染（拒后册长不变）。
    let mut cat2 = SceneCatalog::new();
    let _ = cat2.register(5, SceneClass::Render, "合法场景");
    let bad0 = cat2.register(0, SceneClass::Render, "零号场景");
    let badmax = cat2.register(MAX_SCENES + 1, SceneClass::Render, "越界场景");
    let badname = cat2.register(6, SceneClass::Compute, "   ");
    let _ = cat2.register(7, SceneClass::Compute, "另一合法场景");
    let dup = cat2.register(5, SceneClass::Mixed, "重复五号");
    let gates_ok = bad0 == Err(ScCode::SceneRange)
        && badmax == Err(ScCode::SceneRange)
        && badname == Err(ScCode::SceneNameless)
        && dup == Err(ScCode::SceneDuplicate)
        && cat2.rejected_range == 2
        && cat2.rejected_nameless == 1
        && cat2.rejected_duplicate == 1
        && cat2.len() == 2
        && cat2.classify_of(5) == Some(SceneClass::Render);
    if gates_ok {
        s.ok("U03-四-02-四闸显性拒与计数");
    } else {
        s.fail("U03-四-02-四闸显性拒与计数", "登记闸失效");
    }

    // 四-03：分布统计+歪斜判定——四类齐布不歪斜；3 渲染 1 计算歪斜
    // （3×4=12 > 4×3=12？不——恰 12 不大于 12，4 类里 3 渲染占比 3/4
    // 恰达界不算歪——单类严格过 3/4 才歪：4 登记里 4 渲染才歪。构造
    // 5 登记 4 渲染 1 其他：4×4=16 > 5×3=15 → 歪斜成立）。
    let mut cat3 = SceneCatalog::new();
    let _ = cat3.register(11, SceneClass::Render, "渲染一");
    let _ = cat3.register(12, SceneClass::Render, "渲染二");
    let _ = cat3.register(13, SceneClass::Compute, "计算一");
    let _ = cat3.register(14, SceneClass::Mixed, "混合一");
    let _ = cat3.register(15, SceneClass::Extreme, "极限一");
    // 手算：cat3 登记为 2 渲染+1 计算+1 混合+1 极限 → dist [2,1,1,1]；
    // 单类最高 2×4=8 ≤ 5×3=15 → 不歪斜（占比 0.4 远低于 0.75 界）。
    let dist_ok = cat3.distribution() == [2, 1, 1, 1] && !cat3.is_skewed();
    let mut cat4 = SceneCatalog::new();
    let _ = cat4.register(21, SceneClass::Render, "渲染甲");
    let _ = cat4.register(22, SceneClass::Render, "渲染乙");
    let _ = cat4.register(23, SceneClass::Render, "渲染丙");
    let _ = cat4.register(24, SceneClass::Render, "渲染丁");
    let _ = cat4.register(25, SceneClass::Compute, "计算甲");
    let skew_ok = cat4.distribution() == [4, 1, 0, 0] && cat4.is_skewed();
    let empty_ok = !SceneCatalog::new().is_skewed(); // 空册不评歪斜
    if dist_ok && skew_ok && empty_ok {
        s.ok("U03-四-03-分布统计与歪斜判定");
    } else {
        s.fail("U03-四-03-分布统计与歪斜判定", "分布面失效");
    }

    // --- 一组（判据二）---
    // 组-01：vcu02 口径对拍——crosscheck 零码；MAX_SCENES 消费同源（本册
    // 容量 == vcu02 场景维上限；id 上限登记恰被 SceneRange 闸拦）。
    let cross_ok = crosscheck_vcu02().is_none();
    let mut cat5 = SceneCatalog::new();
    let cap_ok = cat5.capacity == MAX_SCENES as usize;
    let edge_bad = cat5.register(MAX_SCENES, SceneClass::Extreme, "顶格场景").is_ok()
        && cat5.register(MAX_SCENES + 1, SceneClass::Extreme, "超格场景")
            == Err(ScCode::SceneRange);
    // 手算：MAX_SCENES=256（vcu02 同款）——顶格 id 256 合法、257 表外。
    if cross_ok && cap_ok && edge_bad && MAX_SCENES == 256 {
        s.ok("U03-组-01-vcu02口径同源对拍");
    } else {
        s.fail("U03-组-01-vcu02口径同源对拍", "口径漂移");
    }

    // 组-02：ScCode 四码互异+详情互异+详情非空+缺省分支。
    let codes = ScCode::ALL;
    let mut codes_ok = codes.len() == 4;
    let mut i2 = 0usize;
    while i2 < codes.len() && codes_ok {
        if codes[i2].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i2 + 1;
        while j < codes.len() {
            if codes[i2] == codes[j] || codes[i2].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i2 += 1;
    }
    if codes_ok {
        s.ok("U03-组-02-四码互异详情齐备");
    } else {
        s.fail("U03-组-02-四码互异详情齐备", "码面漂移");
    }

    // 组-03：容量闸恰值——构造小容量册（直接构造结构体面）灌满后第
    // capacity+1 条登记 CatalogFull；容量内照常入册。
    let mut small = SceneCatalog::new();
    small.capacity = 3;
    let s1 = small.register(31, SceneClass::Render, "甲").is_ok();
    let s2 = small.register(32, SceneClass::Compute, "乙").is_ok();
    let s3 = small.register(33, SceneClass::Mixed, "丙").is_ok();
    let s4 = small.register(34, SceneClass::Extreme, "丁") == Err(ScCode::CatalogFull);
    let small_ok = s1 && s2 && s3 && s4 && small.rejected_full == 1 && small.len() == 3;
    if small_ok {
        s.ok("U03-组-03-容量闸恰值策略");
    } else {
        s.fail("U03-组-03-容量闸恰值策略", "容量闸失效");
    }

    // --- 判据元 ---
    // 元-01：零 panic 面——自扫本文件（字符串剥空单遍词法）+空册全查询
    // 平凡（classify/name/distribution/summary/is_skewed 空态不炸）。
    let src = include_str!("vcu03_scenecls.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let empty = SceneCatalog::new();
    let empty_ok = empty.classify_of(1).is_none()
        && empty.name_of(1).is_none()
        && empty.distribution() == [0, 0, 0, 0]
        && empty.summary().contains("U03")
        && !empty.is_skewed()
        && empty.is_empty()
        && !U03_SCENECLS_VERSION.is_empty();
    if hits == 0 && empty_ok {
        s.ok("U03-元-01-零panic空态平凡");
    } else {
        s.fail("U03-元-01-零panic空态平凡", "panic 面或空态失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
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
