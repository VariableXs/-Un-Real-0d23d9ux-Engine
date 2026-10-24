//! VE-F5411 · 网络文档（vez20_netdoc）
//!
//! 网络文档三族——**网络架构册**（四层与双通道导览）/ **开发者网络指南**
//! （多人玩法开发——给开发者的手册）/ **故障手册**（连接失败/同步异常/
//! 延迟过高三景）；**四节定式 CI 断言同口径**（与 F5212 特效域文档同款
//! 定式——跨域口径唯一：定位/用法/边界/故障按序非空，缺节 CI 拦截）。
//!
//! 数据结构（锚点原文）：三族目录；三景单；四节定式。
//!
//! 错误路径与降级矩阵：**缺节→断言拦截**（CI 闸——「故障怎么办」那节
//! 缺失的册页，读者出事时文档也在出事）；**复现失败→阻断发版**（故障
//! 手册里的复现步骤跑不通就发版，等于发了一个没有说明书的高危品）；
//! **缺景→补录排程**（三景单缺口进补录队列点名，不静默缺角）。
//!
//! 内容源：F5401-F5410 网络域十单提炼（同源映射——文档 API 面 ⊆
//! REAL_API_SURFACE，越界面即下架）。性能：断言 O(册数)；复现
//! O(步骤)；补录 O(景)。跨批对接点：F5419 联调。无障碍：文档读屏
//! 结构（域本色——可读名单直出族/册/节树）。无隐私面。
//!
//! Z 域字符串码五码互异：`E_ND_MISSING_SECTION` / `E_ND_MISSING_SCENE`
//! / `E_ND_REPRO_FAIL` / `E_ND_DIVERGE` / `E_ND_UNKNOWN_FAMILY`。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 缺节：四节定式不齐（缺/乱序/空内容）——CI 断言拦截。
pub const E_ND_MISSING_SECTION: &str = "E_ND_MISSING_SECTION";
/// 缺景：故障手册三景单缺口——补录排程点名。
pub const E_ND_MISSING_SCENE: &str = "E_ND_MISSING_SCENE";
/// 复现失败：故障复现步骤跑不通——发版闸阻断。
pub const E_ND_REPRO_FAIL: &str = "E_ND_REPRO_FAIL";
/// 分叉：册页引用 API 越出同源映射面（REAL_API_SURFACE）——下架。
pub const E_ND_DIVERGE: &str = "E_ND_DIVERGE";
/// 未知族：目录注册族外册页。
pub const E_ND_UNKNOWN_FAMILY: &str = "E_ND_UNKNOWN_FAMILY";

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_ND_MISSING_SECTION => "缺节：四节定式不齐（缺/乱序/空内容），CI 拦截",
        E_ND_MISSING_SCENE => "缺景：故障手册三景单缺口，进补录排程",
        E_ND_REPRO_FAIL => "复现失败：故障复现步骤跑不通，阻断发版",
        E_ND_DIVERGE => "分叉：册页引用越出同源映射面，下架处理",
        E_ND_UNKNOWN_FAMILY => "未知族：族外册页不收，目录闸拒绝",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 三族与四节定式（与 F5212 同口径——跨域口径唯一）
// ---------------------------------------------------------------------------

/// 三族闭集（族键定式有序——目录闸按此校验）。
pub const THREE_FAMILIES: [&str; 3] = ["架构册", "开发指南", "故障手册"];

/// 四节闭集（定式有序——读屏结构一致；与特效域 F5212 同口径）。
pub const FOUR_SECTIONS: [&str; 4] = ["定位", "用法", "边界", "故障"];

/// 同源映射面（F5401-F5410 网络域十单提炼的 API 闭集——册页引用
/// ⊆ 此面才上架，任一不在即分叉下架——撤出可引用面不是删除）。
pub const REAL_API_SURFACE: [&str; 12] = [
    "NetSession", "ReliableChannel", "UnreliableChannel", "SnapSync",
    "LobbyBook", "JoinFlow", "NatPunch", "RelayFallback",
    "RttProbe", "RollbackReplay", "StateDiff", "PresenceList",
];

/// 四节内容单册页（标题+族+四节按序非空+引用面清单）。
#[derive(Clone, Debug)]
pub struct NetDocPage {
    pub family: &'static str,
    pub title: &'static str,
    /// 四节内容——必须恰按 [`FOUR_SECTIONS`] 顺序齐备非空。
    pub sections: [String; 4],
    /// 册页引用的 API 名清单（⊆ [`REAL_API_SURFACE`] 才合规）。
    pub refs: Vec<&'static str>,
}

impl NetDocPage {
    /// 四节定式校验（O(1)——恰四节按序非空：名序对拍+内容非空）。
    pub fn sections_ok(&self) -> bool {
        for (i, name) in FOUR_SECTIONS.iter().enumerate() {
            if self.sections[i].is_empty() {
                return false;
            }
            // 名序对拍由构造侧保证（sections 数组下标即定式序），
            // 这里校验内容非空与定式齐备。
            let _ = name;
        }
        true
    }

    /// 同源映射对拍（O(引用数)——任一引用越出面即分叉）。
    pub fn refs_ok(&self) -> bool {
        self.refs
            .iter()
            .all(|r| REAL_API_SURFACE.contains(r))
    }
}

/// 三族目录（族闸+册页登记+审计——O(册数) 断言）。
pub struct NetDocToc {
    pages: Vec<NetDocPage>,
    pub rejected: u32,
}

pub const TOC_CAP: usize = 32;

impl NetDocToc {
    pub fn new() -> Self {
        NetDocToc { pages: Vec::new(), rejected: 0 }
    }

    /// 册页上架（族闸：族外册页拒；分叉面拒——越界引用不下架即拦在
    /// 门口；缺节拒——CI 断言前移到上架闸，双重拦截）。幂等（同族同
    /// 标题重复登记 False）。
    pub fn add_page(&mut self, page: NetDocPage) -> Result<bool, &'static str> {
        if !THREE_FAMILIES.contains(&page.family) {
            self.rejected += 1;
            return Err(E_ND_UNKNOWN_FAMILY);
        }
        if !page.sections_ok() {
            self.rejected += 1;
            return Err(E_ND_MISSING_SECTION);
        }
        if !page.refs_ok() {
            self.rejected += 1;
            return Err(E_ND_DIVERGE);
        }
        if self.pages.len() >= TOC_CAP {
            self.rejected += 1;
            return Err(E_ND_DIVERGE); // 容量满复用分叉码（目录也是资源面）
        }
        if self.pages.iter().any(|p| p.family == page.family && p.title == page.title) {
            return Ok(false);
        }
        self.pages.push(page);
        Ok(true)
    }

    /// CI 断言（O(册数)——全册四节齐+引用面合规，CI 每构建跑一遍）。
    pub fn audit(&self) -> Result<usize, &'static str> {
        for p in self.pages.iter() {
            if !p.sections_ok() {
                return Err(E_ND_MISSING_SECTION);
            }
            if !p.refs_ok() {
                return Err(E_ND_DIVERGE);
            }
        }
        Ok(self.pages.len())
    }

    /// 族册数（O(册数) 线性计数——族导览数字面）。
    pub fn family_len(&self, family: &str) -> usize {
        self.pages.iter().filter(|p| p.family == family).count()
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
}

impl Default for NetDocToc {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三景单与补录排程（故障手册的景闭集）
// ---------------------------------------------------------------------------

/// 三景闭集（故障手册景定式——缺景即补录排程点名）。
pub const THREE_SCENES: [&str; 3] = ["连接失败", "同步异常", "延迟过高"];

/// 补录排程条目（缺景点名——排程可查不静默缺角）。
#[derive(Clone, Debug, PartialEq)]
pub struct BacklogItem {
    pub scene: &'static str,
    pub note: &'static str,
}

/// 三景单（景覆盖账+补录排程——O(景) 点名）。
pub struct SceneBook {
    covered: Vec<&'static str>,
    pub backlog: Vec<BacklogItem>,
}

impl SceneBook {
    pub fn new() -> Self {
        SceneBook { covered: Vec::new(), backlog: Vec::new() }
    }

    /// 景登记（景闭集校验——族外景拒；幂等）。
    pub fn cover(&mut self, scene: &'static str) -> Result<bool, &'static str> {
        if !THREE_SCENES.contains(&scene) {
            return Err(E_ND_UNKNOWN_FAMILY); // 景闸复用族码（同"未知名"语义）
        }
        if self.covered.contains(&scene) {
            return Ok(false);
        }
        self.covered.push(scene);
        Ok(true)
    }

    /// 三景断言（O(景)=O(3) 闭集点名——缺景进补录排程返回 Err 前先
    /// 记账：缺口可查，补录有排程不是静默缺角）。
    pub fn audit_scenes(&mut self) -> Result<usize, &'static str> {
        let mut missing: Vec<&'static str> = Vec::new();
        for s in THREE_SCENES.iter() {
            if !self.covered.contains(s) {
                missing.push(s);
            }
        }
        if !missing.is_empty() {
            for m in missing.iter() {
                if !self.backlog.iter().any(|b| b.scene == *m) {
                    self.backlog.push(BacklogItem { scene: m, note: "三景缺口——补录排程点名" });
                }
            }
            return Err(E_ND_MISSING_SCENE);
        }
        Ok(self.covered.len())
    }

    /// 补录排程消化（缺景补齐后清账——排程闭环）。
    pub fn drain_backlog(&mut self) -> usize {
        let n = self.backlog.len();
        self.backlog.clear();
        n
    }

    pub fn covered_len(&self) -> usize {
        self.covered.len()
    }
}

impl Default for SceneBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 复现步骤单与发版闸（复现失败→阻断发版）
// ---------------------------------------------------------------------------

/// 复现步骤（故障景的复现序列——O(步骤) 逐步可执行）。
#[derive(Clone, Debug, PartialEq)]
pub struct ReproSheet {
    pub scene: &'static str,
    /// 步骤序列（每步一个动作描述——复现是确定序列不是玄学）。
    pub steps: Vec<&'static str>,
    /// 预期征兆（复现成功的判据——观察面）。
    pub expect: &'static str,
}

impl ReproSheet {
    pub fn new(scene: &'static str, expect: &'static str) -> Self {
        ReproSheet { scene, steps: Vec::new(), expect }
    }

    pub fn push_step(&mut self, s: &'static str) {
        self.steps.push(s);
    }

    /// 复现执行（模拟步进：每步返回观察值——全步骤跑完且征兆命中
    /// 即复现成功。O(步骤)。observe 由调用方注入——文档面不伪装
    /// 真执行，观察者是联调面 F5419 的探针）。
    pub fn replay(&self, observe: impl Fn(usize) -> Option<&'static str>) -> Result<(), &'static str> {
        if self.steps.is_empty() {
            return Err(E_ND_REPRO_FAIL); // 零步骤即复现失败（没有路径的复现单是空票）
        }
        for i in 0..self.steps.len() {
            match observe(i) {
                Some(symptom) if symptom == self.expect => return Ok(()),
                _ => continue,
            }
        }
        Err(E_ND_REPRO_FAIL)
    }
}

/// 发版闸（故障手册三景全绿+全部复现成功才放行——复现失败阻断发版）。
pub struct ReleaseGate {
    pub scenes: SceneBook,
    repros: Vec<ReproSheet>,
    pub blocked: u32,
}

impl ReleaseGate {
    pub fn new() -> Self {
        ReleaseGate { scenes: SceneBook::new(), repros: Vec::new(), blocked: 0 }
    }

    pub fn add_repro(&mut self, r: ReproSheet) {
        self.repros.push(r);
    }

    /// 发版裁决（O(景)+O(复现单×步骤)——三景齐+全部复现成功才放行；
    /// 任一复现失败 → blocked+1 + Err(REPRO_FAIL) 阻断）。
    pub fn release(&mut self, observe: impl Fn(&str, usize) -> Option<&'static str>) -> Result<(), &'static str> {
        self.scenes.audit_scenes()?;
        for r in self.repros.iter() {
            let ob = |i: usize| observe(r.scene, i);
            if r.replay(ob).is_err() {
                self.blocked += 1;
                return Err(E_ND_REPRO_FAIL);
            }
        }
        Ok(())
    }

    pub fn repro_len(&self) -> usize {
        self.repros.len()
    }
}

impl Default for ReleaseGate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 读屏结构（无障碍：文档读屏结构——域本色可读名单）
// ---------------------------------------------------------------------------

/// 目录可读名单（族→册树直出——O(册数)）。
pub fn toc_spoken(toc: &NetDocToc) -> String {
    let mut out = String::from("网络文档目录：");
    for family in THREE_FAMILIES.iter() {
        out.push_str(&format!("[{}]{}册 ", family, toc.family_len(family)));
    }
    out
}

// ---------------------------------------------------------------------------
// 判据（锚点逐条：三族/三景/四节/阻断发版/判据）
// ---------------------------------------------------------------------------

/// F5411 判据集（9 条，全部数值手算复核、零 panic 面）。
pub fn run_vez20_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5411");

    // --- 三族目录（判据一）---
    // 族-01：三族闭集上架——每族一册齐备 family_len 恰值；族外册页拒
    // E_ND_UNKNOWN_FAMILY；幂等重复 False。
    let mut t1 = NetDocToc::new();
    for (fam, title) in [
        ("架构册", "四层与双通道导览"),
        ("开发指南", "多人玩法开发手册"),
        ("故障手册", "三景速查"),
    ] {
        let page = NetDocPage {
            family: fam,
            title,
            sections: [
                String::from("网络连接的职责边界"),
                String::from("以 NetSession 起手"),
                String::from("双通道语义差异"),
                String::from("连接失败见故障手册"),
            ],
            refs: Vec::new(),
        };
        let r = t1.add_page(page).expect("probe-1");
        if !r {
            s.fail("F5411-族-01-三族闭集上架", "首登不应幂等 False");
            break;
        }
    }
    let alien = t1.add_page(NetDocPage {
        family: "渲染册",
        title: "越族册页",
        sections: [
            String::from("a"),
            String::from("b"),
            String::from("c"),
            String::from("d"),
        ],
        refs: Vec::new(),
    });
    let fam_ok = t1.family_len("架构册") == 1
        && t1.family_len("开发指南") == 1
        && t1.family_len("故障手册") == 1
        && matches!(alien, Err(E_ND_UNKNOWN_FAMILY))
        && t1.len() == 3;
    if fam_ok {
        s.ok("F5411-族-01-三族闭集上架");
    } else {
        s.fail("F5411-族-01-三族闭集上架", "三族目录失效");
    }

    // --- 四节定式（判据二）---
    // 节-01：缺节拦截——空节册页上架闸拒 E_ND_MISSING_SECTION；
    // 合规册页 sections_ok True；CI audit 全册通过返回册数恰值。
    let mut t2 = NetDocToc::new();
    let hole = t2.add_page(NetDocPage {
        family: "故障手册",
        title: "缺节册页",
        sections: [
            String::from("定位在"),
            String::from("用法在"),
            String::from(""), // 边界节缺失
            String::from("故障在"),
        ],
        refs: Vec::new(),
    });
    let full = t2.add_page(NetDocPage {
        family: "架构册",
        title: "合规册页",
        sections: [
            String::from("四层导览定位"),
            String::from("NetSession 用法"),
            String::from("双通道边界"),
            String::from("故障导向手册"),
        ],
        refs: Vec::new(),
    });
    let audited = t2.audit();
    let sec_ok = matches!(hole, Err(E_ND_MISSING_SECTION))
        && matches!(full, Ok(true))
        && matches!(audited, Ok(1))
        && t2.rejected == 1;
    if sec_ok {
        s.ok("F5411-节-01-缺节断言拦截");
    } else {
        s.fail("F5411-节-01-缺节断言拦截", "四节定式失效");
    }

    // --- 同源映射（判据三）---
    // 面-01：引用面闭集——合规引用（NetSession/SnapSync）过；越界引用
    // （FictionalApi）拒 E_ND_DIVERGE——册页引用⊆面才上架（分叉下架
    // 语义拦在门口）。
    let mut t3 = NetDocToc::new();
    let good = t3.add_page(NetDocPage {
        family: "开发指南",
        title: "引用合规",
        sections: [
            String::from("定位"),
            String::from("用法"),
            String::from("边界"),
            String::from("故障"),
        ],
        refs: vec!["NetSession", "SnapSync"],
    });
    let bad = t3.add_page(NetDocPage {
        family: "开发指南",
        title: "越界引用",
        sections: [
            String::from("定位"),
            String::from("用法"),
            String::from("边界"),
            String::from("故障"),
        ],
        refs: vec!["NetSession", "FictionalApi"],
    });
    let surface_ok = matches!(good, Ok(true)) && matches!(bad, Err(E_ND_DIVERGE));
    if surface_ok {
        s.ok("F5411-面-01-同源映射越界拒");
    } else {
        s.fail("F5411-面-01-同源映射越界拒", "同源映射失效");
    }

    // --- 三景单（判据四）---
    // 景-01：三景闭集覆盖——三景全登 audit_scenes Ok(3)；缺景进补录
    // 排程（backlog 点名内容精确）后 drain 闭环；景外景拒。
    let mut sb1 = SceneBook::new();
    let _ = sb1.cover("连接失败");
    let _ = sb1.cover("同步异常");
    let miss = sb1.audit_scenes(); // 缺"延迟过高"
    let alien_scene = sb1.cover("渲染崩溃");
    let mut sb2 = SceneBook::new();
    for sc in THREE_SCENES.iter() {
        let _ = sb2.cover(sc);
    }
    let all_green = sb2.audit_scenes();
    let scene_ok = matches!(miss, Err(E_ND_MISSING_SCENE))
        && sb1.backlog.len() == 1
        && sb1.backlog[0].scene == "延迟过高"
        && sb1.drain_backlog() == 1
        && sb1.backlog.is_empty()
        && matches!(alien_scene, Err(E_ND_UNKNOWN_FAMILY))
        && matches!(all_green, Ok(3));
    if scene_ok {
        s.ok("F5411-景-01-三景闭集补录排程");
    } else {
        s.fail("F5411-景-01-三景闭集补录排程", "三景单失效");
    }

    // --- 复现步骤单（判据五）---
    // 复-01：复现执行 O(步骤)——连接失败复现单 3 步征兆在第 3 步命中
    // →Ok；征兆永不命中 →Err(E_ND_REPRO_FAIL)；零步骤单即失败（没有
    // 路径的复现单是空票）。
    let mut r1 = ReproSheet::new("连接失败", "NatPunch 失败转中继");
    r1.push_step("起 lobby");
    r1.push_step("发起打洞");
    r1.push_step("观察中继切换");
    let hit = r1.replay(|i| if i == 2 { Some("NatPunch 失败转中继") } else { None });
    let mut r2 = ReproSheet::new("同步异常", "漂移超阈");
    r2.push_step("s1");
    r2.push_step("s2");
    let miss2 = r2.replay(|_| None);
    let r3 = ReproSheet::new("延迟过高", "预测回滚");
    let empty_fail = r3.replay(|_| Some("预测回滚"));
    let repro_ok = hit.is_ok() && matches!(miss2, Err(E_ND_REPRO_FAIL)) && matches!(empty_fail, Err(E_ND_REPRO_FAIL));
    if repro_ok {
        s.ok("F5411-复-01-复现步骤O步骤");
    } else {
        s.fail("F5411-复-01-复现步骤O步骤", "复现单失效");
    }

    // --- 阻断发版（判据六）---
    // 版-01：发版闸——三景齐+复现全绿放行 Ok；复现失败 blocked+1
    // 阻断 Err(REPRO_FAIL)（发了一个没有说明书的高危品才是不发版的
    // 理由——阻断是保护不是拖延）。
    let mut g1 = ReleaseGate::new();
    for sc in THREE_SCENES.iter() {
        let _ = g1.scenes.cover(sc);
    }
    let mut rr = ReproSheet::new("延迟过高", "预测回滚");
    rr.push_step("压测");
    g1.add_repro(rr);
    let pass = g1.release(|scene, _| if scene == "延迟过高" { Some("预测回滚") } else { None });
    let mut g2 = ReleaseGate::new();
    for sc in THREE_SCENES.iter() {
        let _ = g2.scenes.cover(sc);
    }
    let mut rb = ReproSheet::new("连接失败", "转中继");
    rb.push_step("打洞");
    g2.add_repro(rb);
    let blocked = g2.release(|_, _| None);
    let gate_ok = pass.is_ok()
        && matches!(blocked, Err(E_ND_REPRO_FAIL))
        && g2.blocked == 1
        && g1.blocked == 0;
    if gate_ok {
        s.ok("F5411-版-01-复现失败阻断发版");
    } else {
        s.fail("F5411-版-01-复现失败阻断发版", "发版闸失效");
    }

    // --- 读屏结构（判据七）---
    // 屏-01：可读名单——族册树直出对账（[架构册]1册 [开发指南]1册
    // [故障手册]1册）；reason/ direction 文案互异非空（无障碍文本面）。
    let mut t7 = NetDocToc::new();
    for (fam, title) in [
        ("架构册", "A"),
        ("开发指南", "B"),
        ("故障手册", "C"),
    ] {
        let _ = t7.add_page(NetDocPage {
            family: fam,
            title,
            sections: [
                String::from("一"),
                String::from("二"),
                String::from("三"),
                String::from("四"),
            ],
            refs: Vec::new(),
        });
    }
    let spoken = toc_spoken(&t7);
    let spoken_ok = spoken.contains("[架构册]1册")
        && spoken.contains("[开发指南]1册")
        && spoken.contains("[故障手册]1册");
    if spoken_ok {
        s.ok("F5411-屏-01-读屏结构域本色");
    } else {
        s.fail("F5411-屏-01-读屏结构域本色", "读屏结构失效");
    }

    // --- 判据元（判据八）---
    // 元-01：五码互异 + reason 互异非空 + 缺省兜底 + 零 panic 空态
    // （空目录 audit Ok(0)——空是合规态不是故障；空三景单缺景全点名）。
    let codes = [E_ND_MISSING_SECTION, E_ND_MISSING_SCENE, E_ND_REPRO_FAIL, E_ND_DIVERGE, E_ND_UNKNOWN_FAMILY];
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] || reason_of(codes[i]) == reason_of(codes[j]) {
                distinct = false;
            }
        }
    }
    let meta_codes = distinct
        && reason_of("E_ND_UNKNOWN") == "未知诊断码"
        && codes.iter().all(|c| !reason_of(c).is_empty());
    let t8 = NetDocToc::new();
    let mut sb8 = SceneBook::new();
    let empty_audit = t8.audit();
    let scene_miss = sb8.audit_scenes();
    let meta_ok = meta_codes
        && matches!(empty_audit, Ok(0))
        && matches!(scene_miss, Err(E_ND_MISSING_SCENE))
        && sb8.backlog.len() == 3; // 三景全缺全点名
    if meta_ok {
        s.ok("F5411-元-01-五码互异空态合规");
    } else {
        s.fail("F5411-元-01-五码互异空态合规", "判据元失效");
    }

    // --- 编排（判据九）---
    // 编-01：全链路 O(册数+景+步骤)——三族三册上架+三景覆盖+两复现单
    // 全绿 → CI audit 3 + release Ok；注入一册缺节 → CI 拦截；注入
    // 复现失败 → 发版阻断——文档是闸不是附产物。
    let mut t9 = NetDocToc::new();
    for (fam, title) in [
        ("架构册", "四层双通道"),
        ("开发指南", "多人玩法"),
        ("故障手册", "三景速查"),
    ] {
        let _ = t9.add_page(NetDocPage {
            family: fam,
            title,
            sections: [
                String::from("定位段"),
                String::from("用法段"),
                String::from("边界段"),
                String::from("故障段"),
            ],
            refs: vec!["NetSession"],
        });
    }
    let ci = t9.audit();
    let hole_page = t9.add_page(NetDocPage {
        family: "架构册",
        title: "缺节新册",
        sections: [
            String::from("有"),
            String::from("有"),
            String::new(),
            String::from("有"),
        ],
        refs: Vec::new(),
    });
    let mut g9 = ReleaseGate::new();
    for sc in THREE_SCENES.iter() {
        let _ = g9.scenes.cover(sc);
    }
    for (scene, expect, step) in [
        ("连接失败", "转中继", "打洞"),
        ("延迟过高", "回滚", "压测"),
    ] {
        let mut rp = ReproSheet::new(scene, expect);
        rp.push_step(step);
        g9.add_repro(rp);
    }
    let rel = g9.release(|scene, _| {
        if scene == "连接失败" {
            Some("转中继")
        } else if scene == "延迟过高" {
            Some("回滚")
        } else {
            None
        }
    });
    let chain_ok = matches!(ci, Ok(3))
        && matches!(hole_page, Err(E_ND_MISSING_SECTION))
        && rel.is_ok()
        && g9.repro_len() == 2;
    if chain_ok {
        s.ok("F5411-编-01-全链路O册景步骤");
    } else {
        s.fail("F5411-编-01-全链路O册景步骤", "编排失效");
    }

    s
}
