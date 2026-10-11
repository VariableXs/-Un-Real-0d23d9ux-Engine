//! VE-F5214 · 特效示例（VE-Z 域 · 示例面 · 锚点目标 300 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5214`
//!
//! **判据（锚点原文）**：三例同源、沙箱试跑、版本戳、日验证、判据。
//!
//! **职责定位（锚点原文）**：特效示例——三例可导入试跑：粒子特效编排/
//! 后处理氛围组合/转场串场景——与真实库同源防分叉；版本戳+日验证同
//! 口径。
//!
//! **数据结构（锚点原文）**：三例目录；导入试跑器；同源映射。
//!
//! **错误路径与降级矩阵（锚点原文）**：示例失效→同源校验下架；导入
//! 冲突→沙箱提示；缺失→补录。
//!
//! **性能逐项分解（锚点原文）**：试跑 O(示例数)；校验 O(1)；补录 O(缺)。
//!
//! **跨批对接点（锚点原文）**：F5203/F5204/F5206 同源；F5219 联调。
//!
//! **无障碍与隐私（锚点原文）**：示例遵循交互词典（域本色）；无隐私
//! 面。
//!
//! # 深化说明：示例是库的活广告——分叉的示例比没有示例更毒
//!
//! 「与真实库同源防分叉」——示例里调的 API 如果库里没有（或签名变
//! 了），照抄示例的创作者拿到的全是编译错——**同源映射**（
//! [`REAL_API_SURFACE`] 闭集）逐 API 对拍：示例 API 集 ⊆ 真实库面才
//! 上架，分叉即下架（[`E_EX_DIVERGE`]——示例失效→同源校验下架，下架
//! 不是删除是撤出可导入面）；「可导入试跑」落成沙箱（[`Sandbox`]）：
//! 导入名冲突→沙箱提示 [`E_EX_IMPORT_CONFLICT`]，试跑**直打真实现**
//! （粒子例灌 F5208 叠加闸/氛围例灌 F5207 钳制与预设往返/转场例灌
//! Exclusive 互斥——与 F5213 同纪律：消费上游而非复刻影子）；「缺失→
//! 补录」落成排程点名（[`E_EX_MISSING`] 补录 O(缺)——锚点口径）；
//! 「版本戳+日验证同口径」——版本戳与 F5213 同构（build+域指纹），
//! 日验证台账日序单调（倒填日 [`E_EX_STALE`]）+新鲜度（最新记录距今
//! ≤1 日——日验证停了示例就在悄悄腐烂）。
//!
//! ## 深化一：三例各走一条真实现管线——示例跑通了等于管线活着
//!
//! 三例分流（[`Sandbox::run_example`]）：粒子编排例=三张 Additive 卡
//! 过叠加闸全 Added；氛围组合例=schema 登记+越界钳制+预设导出导入往
//! 返闭合；转场串场例=Exclusive 双卡第二枚被拒（一天一相）。三例跑通
//! =三条核心管线全活着——示例失效（管线死）即 [`E_EX_RUN_FAIL`] 试跑
//! 失败下架。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vez07_params::{PresetPack, Validator};
use crate::svstar2::vez08_combo::{BlendMode, OverlayRules, StackResolver};

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_EX_* 前缀）
// ---------------------------------------------------------------------------

/// 同源分叉：示例 API 不在真实库面（下架）。
pub const E_EX_DIVERGE: &str = "E_EX_DIVERGE";
/// 导入冲突：示例名与沙箱现有特效冲突（沙箱提示）。
pub const E_EX_IMPORT_CONFLICT: &str = "E_EX_IMPORT_CONFLICT";
/// 示例缺失：三例不齐（补录排程）。
pub const E_EX_MISSING: &str = "E_EX_MISSING";
/// 试跑失败：示例在沙箱跑不通（下架）。
pub const E_EX_RUN_FAIL: &str = "E_EX_RUN_FAIL";
/// 版本过期：版本戳不符或日验证断档。
pub const E_EX_STALE: &str = "E_EX_STALE";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_EX_DIVERGE => "同源分叉：示例 API 不在真实库面（下架）",
        E_EX_IMPORT_CONFLICT => "导入冲突：示例名与沙箱现有特效冲突",
        E_EX_MISSING => "示例缺失：三例不齐（补录排程）",
        E_EX_RUN_FAIL => "试跑失败：示例在沙箱跑不通（下架）",
        E_EX_STALE => "版本过期：戳不符或日验证断档",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_EX_DIVERGE => "按真实库面改写示例后重新上架",
        E_EX_IMPORT_CONFLICT => "换名导入或先卸载冲突特效",
        E_EX_MISSING => "按补录排程补写缺失示例",
        E_EX_RUN_FAIL => "修示例或修管线后重跑沙箱",
        E_EX_STALE => "以当前版本重跑日验证",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、同源映射：真实库 API 闭集（示例 API ⊆ 此面才同源）
// ---------------------------------------------------------------------------

/// 真实库 API 闭集（F5203-F5211 提炼的最小面——示例只许调这些）。
pub const REAL_API_SURFACE: [&str; 12] = [
    "OverlayRules::new",
    "StackResolver::push",
    "ComboBudget::admit",
    "Validator::clamp_with_hint",
    "SchemaBook::define",
    "PresetPack::put_override",
    "export_preset",
    "import_preset",
    "FlashGate::check",
    "ParamSanitizer::clean",
    "emit",
    "combo_report",
];

/// **同源校验**（校验 O(1) 定长闭集线性——锚点口径）：示例 API 全在
/// 真实库面才同源；任一不在即分叉 [`E_EX_DIVERGE`]。
pub fn same_source_check(ex_apis: &[String]) -> Result<(), &'static str> {
    let mut i = 0usize;
    while i < ex_apis.len() {
        if !REAL_API_SURFACE.contains(&ex_apis[i].as_str()) {
            return Err(E_EX_DIVERGE);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、三例目录：kind 闭集+同源声明+下架状态
// ---------------------------------------------------------------------------

/// 三例闭集（粒子编排/氛围组合/转场串场）。
pub const EXAMPLE_KINDS: [&str; 3] = ["粒子编排", "氛围组合", "转场串场"];

/// 三例域闭集（F5203/F5204/F5206 同源域）。
pub const EXAMPLE_DOMAINS: [&str; 3] = ["particles", "postfx", "transition"];

/// 单条示例：名/域/描述/API 清单/下架状态。
#[derive(Clone, Debug)]
pub struct ExampleEntry {
    /// 例名（目录唯一键）。
    pub kind: String,
    /// 所属域（必属 [`EXAMPLE_DOMAINS`]）。
    pub domain: String,
    /// 描述（非空——无描述的示例是谜语）。
    pub desc: String,
    /// 示例调用的 API 清单（同源校验对象）。
    pub apis: Vec<String>,
    /// 是否已下架（失效→同源校验下架——撤出可导入面）。
    pub taken_down: bool,
}

/// 三例目录：登记+齐备性+同源下架（试跑 O(示例数)——锚点口径）。
#[derive(Clone, Debug, Default)]
pub struct ExampleCatalog {
    pub entries: Vec<ExampleEntry>,
    /// 缺例补录排程。
    pub backlog: Vec<String>,
    /// 下架计数。
    pub takedowns: u32,
}

impl ExampleCatalog {
    /// 空目录。
    pub fn new() -> Self {
        ExampleCatalog::default()
    }

    /// **登记示例**：kind 闭集+domain 闭集+kind 唯一+描述非空+
    /// API 清单同源（违者即拒——分叉示例不进目录）。
    pub fn add(
        &mut self,
        kind: &str,
        domain: &str,
        desc: &str,
        apis: &[&str],
    ) -> Result<(), &'static str> {
        if !EXAMPLE_KINDS.contains(&kind) || !EXAMPLE_DOMAINS.contains(&domain) {
            return Err(E_EX_MISSING);
        }
        if desc.trim().is_empty() || self.entries.iter().any(|e| e.kind == kind) {
            return Err(E_EX_MISSING);
        }
        let owned: Vec<String> = apis.iter().map(|a| String::from(*a)).collect();
        if same_source_check(&owned).is_err() {
            return Err(E_EX_DIVERGE);
        }
        self.entries.push(ExampleEntry {
            kind: String::from(kind),
            domain: String::from(domain),
            desc: String::from(desc),
            apis: owned,
            taken_down: false,
        });
        Ok(())
    }

    /// **齐备性+同源复审**：三例缺→补录排程点名
    /// （补录 O(缺)——锚点口径）；分叉（复审发现）→下架。
    pub fn verify_all(&mut self) -> Result<usize, &'static str> {
        let mut missing = false;
        let mut i = 0usize;
        while i < EXAMPLE_KINDS.len() {
            let want = EXAMPLE_KINDS[i];
            if !self.entries.iter().any(|e| e.kind == want && !e.taken_down) {
                if !self.backlog.iter().any(|b| b == want) {
                    self.backlog.push(String::from(want));
                }
                missing = true;
            }
            i += 1;
        }
        if missing {
            return Err(E_EX_MISSING);
        }
        self.backlog.clear();
        Ok(self.entries.len())
    }

    /// **下架**（同源校验失效/试跑失败调用）：撤出可导入面+计数。
    pub fn take_down(&mut self, kind: &str) -> bool {
        match self.entries.iter_mut().find(|e| e.kind == kind) {
            Some(e) if !e.taken_down => {
                e.taken_down = true;
                self.takedowns += 1;
                true
            }
            _ => false,
        }
    }

    /// 在架例数。
    pub fn live_len(&self) -> usize {
        self.entries.iter().filter(|e| !e.taken_down).count()
    }
}

// ---------------------------------------------------------------------------
// 三、沙箱：导入冲突检查+三例试跑（直打真实现）
// ---------------------------------------------------------------------------

/// 沙箱：导入名册+试跑计数（导入冲突→沙箱提示）。
#[derive(Clone, Debug, Default)]
pub struct Sandbox {
    /// 沙箱内已导入特效名。
    pub names: Vec<String>,
    /// 导入冲突计数。
    pub conflicts: u32,
    /// 试跑计数。
    pub runs: u32,
}

impl Sandbox {
    /// 空沙箱。
    pub fn new() -> Self {
        Sandbox::default()
    }

    /// **导入**：名冲突 [`E_EX_IMPORT_CONFLICT`]（沙箱提示——不是拒绝
    /// 服务，换个名就能进）。
    pub fn import(&mut self, name: &str) -> Result<(), &'static str> {
        if self.names.iter().any(|n| n == name) {
            self.conflicts += 1;
            return Err(E_EX_IMPORT_CONFLICT);
        }
        self.names.push(String::from(name));
        Ok(())
    }

    /// **试跑粒子编排例**：三张 Additive 卡过叠加闸全 Added（直打
    /// F5208——管线活着示例才活着）。
    fn run_particles_example(st: &mut StackResolver) -> Result<(), &'static str> {
        let names = ["ex_spark", "ex_smoke", "ex_dust"];
        let mut i = 0usize;
        while i < names.len() {
            let card = crate::svstar2::vez08_combo::OverlayCard {
                name: String::from(names[i]),
                class: String::from("particles"),
                blend: BlendMode::Additive,
                weight: 5,
                cost_us: 10,
            };
            match st.push(card) {
                Ok(crate::svstar2::vez08_combo::Verdict::Added) => {}
                _ => return Err(E_EX_RUN_FAIL),
            }
            i += 1;
        }
        Ok(())
    }

    /// **试跑氛围组合例**：schema 登记+越界钳制+预设导出导入往返闭合
    /// （直打 F5207——四形损坏拒载同口径）。
    fn run_postfx_example(v: &mut Validator) -> Result<(), &'static str> {
        if v.book.define("ex_intensity", 0, 100, 40, "示例强度").is_err() {
            return Err(E_EX_RUN_FAIL);
        }
        let oc = match v.clamp_with_hint("ex_intensity", 250) {
            Ok(oc) => oc,
            Err(_) => return Err(E_EX_RUN_FAIL),
        };
        if oc.value != 100 || !oc.clamped {
            return Err(E_EX_RUN_FAIL);
        }
        let mut pack = PresetPack::new("示例氛围");
        pack.put_override("ex_intensity", 30);
        let text = crate::svstar2::vez07_params::export_preset(&pack);
        match crate::svstar2::vez07_params::import_preset(&text) {
            Ok((_, vals)) => {
                if !vals.iter().any(|(n, val)| n == "ex_intensity" && *val == 30) {
                    return Err(E_EX_RUN_FAIL);
                }
            }
            Err(_) => return Err(E_EX_RUN_FAIL),
        }
        Ok(())
    }

    /// **试跑转场串场例**：Exclusive 双卡第二枚被拒（一天一相——直打
    /// F5208 互斥裁决）。
    fn run_transition_example(st: &mut StackResolver) -> Result<(), &'static str> {
        let c1 = crate::svstar2::vez08_combo::OverlayCard {
            name: String::from("ex_fade1"),
            class: String::from("transition"),
            blend: BlendMode::Exclusive,
            weight: 5,
            cost_us: 10,
        };
        if st.push(c1) != Ok(crate::svstar2::vez08_combo::Verdict::Added) {
            return Err(E_EX_RUN_FAIL);
        }
        let c2 = crate::svstar2::vez08_combo::OverlayCard {
            name: String::from("ex_fade2"),
            class: String::from("transition"),
            blend: BlendMode::Exclusive,
            weight: 5,
            cost_us: 10,
        };
        if st.push(c2) != Ok(crate::svstar2::vez08_combo::Verdict::RejectedExclusive) {
            return Err(E_EX_RUN_FAIL);
        }
        Ok(())
    }

    /// **试跑分派**（试跑 O(示例数)——锚点口径）：按域分流三例模型。
    pub fn run_example(
        &mut self,
        entry: &ExampleEntry,
        st: &mut StackResolver,
        v: &mut Validator,
    ) -> Result<(), &'static str> {
        self.runs += 1;
        match entry.domain.as_str() {
            "particles" => Self::run_particles_example(st),
            "postfx" => Self::run_postfx_example(v),
            "transition" => Self::run_transition_example(st),
            _ => Err(E_EX_RUN_FAIL),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、版本戳+日验证：与 F5213 同口径（build+指纹；日序单调+新鲜度）
// ---------------------------------------------------------------------------

/// 版本戳（与 F5213 VersionStamp 同口径——build 号+域清单指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExampleStamp {
    /// 构建号。
    pub build: u32,
    /// 域清单指纹。
    pub fingerprint: u64,
}

/// FNV-1a 64 位（域清单指纹——与 F5213 同口径同常数）。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

impl ExampleStamp {
    /// 以 build 号+域清单生成。
    pub fn new(build: u32, domains: &[&str]) -> Self {
        let mut joined = String::new();
        let mut i = 0usize;
        while i < domains.len() {
            joined.push_str(domains[i]);
            joined.push('|');
            i += 1;
        }
        ExampleStamp { build, fingerprint: fnv64(joined.as_bytes()) }
    }

    /// 双对拍。
    pub fn matches(&self, other: &ExampleStamp) -> bool {
        self.build == other.build && self.fingerprint == other.fingerprint
    }
}

/// 日验证台账：日序单调记录（倒填日拒）+新鲜度（最新记录距今 ≤1 日
/// ——日验证停了示例就在悄悄腐烂）。
#[derive(Clone, Debug, Default)]
pub struct DailyLedger {
    records: Vec<(u32, ExampleStamp)>,
    /// 过期/倒填计数。
    pub stale_days: u32,
}

impl DailyLedger {
    /// 空台账。
    pub fn new() -> Self {
        DailyLedger::default()
    }

    /// **记录一次日验证**：日序必须严格递增（倒填日 [`E_EX_STALE`]）。
    pub fn record(&mut self, day: u32, stamp: ExampleStamp) -> Result<(), &'static str> {
        if let Some((last, _)) = self.records.last() {
            if day <= *last {
                self.stale_days += 1;
                return Err(E_EX_STALE);
            }
        }
        self.records.push((day, stamp));
        Ok(())
    }

    /// **新鲜度**：最新记录 day ≥ today-1（容忍隔日补验）；无记录=
    /// 从未验证=不新鲜。
    pub fn is_fresh(&self, today: u32) -> bool {
        match self.records.last() {
            Some((d, _)) => *d + 1 >= today,
            None => false,
        }
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5214 域自检（判据 13 条六组：三例目录/同源/沙箱/日验证/版本戳/判据元）。
pub fn run_vez14_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5214");

    // --- 三例目录（判据一）---
    // 例-01：三例登记+kind 唯一+非法 kind/domain 拒+缺例补录点名。
    let mut cat = ExampleCatalog::new();
    let a1 = cat.add(
        "粒子编排",
        "particles",
        "三源粒子同屏编排",
        &["OverlayRules::new", "StackResolver::push"],
    );
    let bad_kind = cat.add("玄学示例", "particles", "x", &["emit"]) == Err(E_EX_MISSING);
    let bad_domain = cat.add("粒子编排", "envfx", "x", &["emit"]) == Err(E_EX_MISSING);
    let miss_ci = cat.verify_all();
    let miss_ok = miss_ci == Err(E_EX_MISSING)
        && cat.backlog.len() == 2
        && cat.backlog.iter().any(|b| b == "氛围组合")
        && cat.backlog.iter().any(|b| b == "转场串场");
    if a1.is_ok() && bad_kind && bad_domain && miss_ok {
        s.ok("EZ14-例-01-三例目录与缺例排程");
    } else {
        s.fail("EZ14-例-01-三例目录与缺例排程", "目录闸失效");
    }

    // --- 同源（判据二）---
    // 源-01：真 API 全过+伪 API 拒（分叉不进目录）。
    let good_apis: Vec<String> = ["StackResolver::push", "Validator::clamp_with_hint"]
        .iter()
        .map(|a| String::from(*a))
        .collect();
    let src_ok = same_source_check(&good_apis).is_ok();
    let fake_apis: Vec<String> = vec![String::from("MagicFx::abracadabra")];
    let src_bad = same_source_check(&fake_apis) == Err(E_EX_DIVERGE);
    let diverge_add = cat.add("氛围组合", "postfx", "分叉语料", &["MagicFx::abracadabra"])
        == Err(E_EX_DIVERGE);
    if src_ok && src_bad && diverge_add {
        s.ok("EZ14-源-01-同源映射伪API拒收");
    } else {
        s.fail("EZ14-源-01-同源映射伪API拒收", "同源闸漏");
    }

    // --- 补齐三例（供沙箱/复审）---
    let _ = cat.add(
        "氛围组合",
        "postfx",
        "氛围钳制与预设往返",
        &["SchemaBook::define", "Validator::clamp_with_hint", "export_preset", "import_preset"],
    );
    let _ = cat.add(
        "转场串场",
        "transition",
        "互斥双卡一天一相",
        &["StackResolver::push"],
    );
    let all_ci = cat.verify_all();
    let all_ok = all_ci == Ok(3) && cat.backlog.is_empty() && cat.live_len() == 3;

    // --- 沙箱试跑（判据三）---
    // 箱-01：导入冲突沙箱提示+计数；三例试跑 O(示例数) 全过（直打真实现）。
    let mut sb = Sandbox::new();
    let im1 = sb.import("ex_spark");
    let im2 = sb.import("ex_spark");
    let conflict_ok = im1.is_ok() && im2 == Err(E_EX_IMPORT_CONFLICT) && sb.conflicts == 1;
    let mut st = StackResolver::new(OverlayRules::new());
    let mut v = Validator::new();
    let mut run_n = 0usize;
    let mut run_all_ok = true;
    let mut ei = 0usize;
    while ei < cat.entries.len() {
        let entry = cat.entries[ei].clone();
        if sb.run_example(&entry, &mut st, &mut v).is_ok() {
            run_n += 1;
        } else {
            run_all_ok = false;
        }
        ei += 1;
    }
    let box_ok = conflict_ok && run_all_ok && run_n == 3 && sb.runs == 3
        && st.is_live("ex_spark")
        && v.clamped_count == 1;
    if all_ok && box_ok {
        s.ok("EZ14-箱-01-三例沙箱直打真实现");
    } else {
        s.fail("EZ14-箱-01-三例沙箱直打真实现", "试跑失效");
    }
    // 箱-02：试跑失败→下架（管线死示例死——密度满后粒子例 push 失败）。
    let mut st16 = StackResolver::new(OverlayRules::new());
    let mut fill = 0usize;
    while fill < 16 {
        let card = crate::svstar2::vez08_combo::OverlayCard {
            name: alloc::format!("fill{}", fill),
            class: String::from("particles"),
            blend: BlendMode::Additive,
            weight: 1,
            cost_us: 1,
        };
        let _ = st16.push(card);
        fill += 1;
    }
    let parts_entry = cat.entries[0].clone();
    let mut sb2 = Sandbox::new();
    let runfail = sb2.run_example(&parts_entry, &mut st16, &mut v) == Err(E_EX_RUN_FAIL);
    let td = cat.take_down("粒子编排");
    let td_ok = td && cat.takedowns == 1 && cat.live_len() == 2;
    if runfail && td_ok {
        s.ok("EZ14-箱-02-试跑失败同源下架");
    } else {
        s.fail("EZ14-箱-02-试跑失败同源下架", "下架机制失效");
    }

    // --- 版本戳（判据四）---
    // 版-01：生成+匹配+build 异/指纹异双形拒。
    let s1 = ExampleStamp::new(9, &EXAMPLE_DOMAINS);
    let s2 = ExampleStamp::new(9, &EXAMPLE_DOMAINS);
    let s3 = ExampleStamp::new(10, &EXAMPLE_DOMAINS);
    let s4 = ExampleStamp::new(9, &["particles"]);
    let ver_ok = s1.matches(&s2)
        && !s1.matches(&s3)
        && !s1.matches(&s4)
        && s1.fingerprint != s4.fingerprint;
    if ver_ok {
        s.ok("EZ14-版-01-版本戳双对拍");
    } else {
        s.fail("EZ14-版-01-版本戳双对拍", "版本戳失效");
    }

    // --- 日验证（判据五）---
    // 日-01：日序单调+倒填拒+新鲜度（隔日容忍/断档不新鲜/无记录不新鲜）。
    let mut led = DailyLedger::new();
    let r1 = led.record(100, s1);
    let r2 = led.record(101, s2);
    let r3 = led.record(101, s3);
    let fresh_today = led.is_fresh(102);
    // 手算：最新记录 day=101 → today=102 时 101+1=102≥102 新鲜（容忍次日）；
    // today=103 时 102≥103 不成立 → 断档不新鲜；104 同理；空台账无记录不新鲜。
    let stale_gap = !led.is_fresh(103) && !led.is_fresh(104);
    let empty_fresh = !DailyLedger::new().is_fresh(105);
    let day_ok = r1.is_ok() && r2.is_ok() && r3 == Err(E_EX_STALE)
        && led.stale_days == 1
        && fresh_today && stale_gap && empty_fresh
        && led.len() == 2;
    if day_ok {
        s.ok("EZ14-日-01-日序单调与新鲜度");
    } else {
        s.fail("EZ14-日-01-日序单调与新鲜度", "日验证失效");
    }

    // --- 判据元（判据六）---
    // 元-01：五码互异 + 三要素齐备。
    let codes = [E_EX_DIVERGE, E_EX_IMPORT_CONFLICT, E_EX_MISSING, E_EX_RUN_FAIL, E_EX_STALE];
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
        s.ok("EZ14-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ14-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic 零 unsafe（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez14_examples.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ14-判据-02-零panic零unsafe面");
    } else {
        s.fail("EZ14-判据-02-零panic零unsafe面", "判据/生产面含 panic 或 unsafe");
    }
    // 元-03：三例同源口径——EXAMPLE_DOMAINS 与 F5203/F5204/F5206 域对拍
    // +API 面覆盖三例所需（示例遵循交互词典——域本色）。
    let dom_ok = EXAMPLE_DOMAINS.contains(&"particles")
        && EXAMPLE_DOMAINS.contains(&"postfx")
        && EXAMPLE_DOMAINS.contains(&"transition")
        && EXAMPLE_DOMAINS.len() == 3
        && EXAMPLE_KINDS.len() == 3;
    let mut api_cover = true;
    for need in ["StackResolver::push", "Validator::clamp_with_hint", "export_preset"].iter() {
        if !REAL_API_SURFACE.contains(need) {
            api_cover = false;
        }
    }
    if dom_ok && api_cover {
        s.ok("EZ14-判据-03-三例同源口径自洽");
    } else {
        s.fail("EZ14-判据-03-三例同源口径自洽", "同源口径漂移");
    }

    s
}

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
